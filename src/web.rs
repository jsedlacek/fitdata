//! Dashboard: JSON API over the BigQuery tables plus a single page, behind
//! Google sign-in restricted to an allowlist of accounts.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::get;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::bq::Bq;
use crate::datatypes::READ_SCOPES;
use crate::oauth::{ClientCredentials, auth_request, exchange_code};
use crate::secrets::SecretRef;

const INDEX: &str = include_str!("../web/index.html");
/// Served by the app itself so the page works without access to a CDN.
const ECHARTS: &str = include_str!("../web/vendor/echarts-5.5.1.min.js");
const SESSION_COOKIE: &str = "fd_session";
const OAUTH_COOKIE: &str = "fd_oauth";
const SESSION_DAYS: i64 = 30;

pub struct Config {
    pub port: u16,
    pub public_url: String,
    pub allowed_emails: Vec<String>,
    pub session_key: Vec<u8>,
    pub auth_disabled: bool,
    pub creds: ClientCredentials,
    /// Where "Connect Fitbit" stores a new Health API refresh token.
    pub token_ref: SecretRef,
    pub bq: Bq,
    pub http: reqwest::Client,
}

type Shared = Arc<Config>;

pub async fn serve(cfg: Config) -> Result<()> {
    if !cfg.auth_disabled && cfg.allowed_emails.is_empty() {
        bail!("set ALLOWED_EMAILS (or AUTH_DISABLED=1 for local development)");
    }
    let port = cfg.port;
    let state = Arc::new(cfg);

    let protected = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/vendor/echarts-5.5.1.min.js", get(echarts))
        .route("/api/overview", get(overview))
        .route("/api/day/{date}", get(day))
        .route("/auth/fitbit", get(connect_fitbit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    let app = Router::new()
        .merge(protected)
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("dashboard listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn echarts() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            // Versioned file name, so it never changes.
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        ECHARTS,
    )
}

fn internal(e: anyhow::Error) -> Response {
    tracing::error!("{e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
}

// ---------------------------------------------------------------- sessions

/// `base64(json) . base64(hmac)`; the JSON carries an `exp` unix timestamp.
fn sign(key: &[u8], payload: &Value) -> String {
    let body = URL_SAFE_NO_PAD.encode(payload.to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(body.as_bytes());
    format!(
        "{body}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

fn verify(key: &[u8], token: &str) -> Option<Value> {
    let (body, sig) = token.split_once('.')?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).ok()?;
    mac.update(body.as_bytes());
    mac.verify_slice(&URL_SAFE_NO_PAD.decode(sig).ok()?).ok()?;
    let v: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
    (v["exp"].as_i64()? > chrono::Utc::now().timestamp()).then_some(v)
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(name)?.strip_prefix('='))
}

fn set_cookie(cfg: &Config, name: &str, value: &str, max_age: i64) -> String {
    let secure = if cfg.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
}

async fn require_session(State(cfg): State<Shared>, req: Request, next: Next) -> Response {
    let ok = cfg.auth_disabled
        || cookie(req.headers(), SESSION_COOKIE)
            .and_then(|t| verify(&cfg.session_key, t))
            .is_some();
    if ok {
        return next.run(req).await;
    }
    if req.uri().path().starts_with("/api/") {
        StatusCode::UNAUTHORIZED.into_response()
    } else {
        Redirect::to("/auth/login").into_response()
    }
}

/// Starts an OAuth flow; `purpose` is "login" (who are you) or "health"
/// (consent to read Health data, yielding a new refresh token).
fn start_oauth(cfg: &Config, purpose: &str, scopes: &[&str], offline: bool) -> Response {
    let redirect_uri = format!("{}/auth/callback", cfg.public_url);
    let req = match auth_request(&cfg.creds, &redirect_uri, scopes, offline) {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let pending = json!({
        "state": req.state, "verifier": req.verifier, "purpose": purpose,
        "exp": chrono::Utc::now().timestamp() + 600,
    });
    (
        [(
            header::SET_COOKIE,
            set_cookie(cfg, OAUTH_COOKIE, &sign(&cfg.session_key, &pending), 600),
        )],
        Redirect::to(req.url.as_str()),
    )
        .into_response()
}

async fn login(State(cfg): State<Shared>) -> Response {
    start_oauth(&cfg, "login", &["openid", "email"], false)
}

async fn connect_fitbit(State(cfg): State<Shared>) -> Response {
    start_oauth(&cfg, "health", READ_SCOPES, true)
}

async fn logout(State(cfg): State<Shared>) -> Response {
    (
        [(header::SET_COOKIE, set_cookie(&cfg, SESSION_COOKIE, "", 0))],
        Redirect::to("/auth/login"),
    )
        .into_response()
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(cfg): State<Shared>,
    headers: HeaderMap,
    Query(p): Query<CallbackParams>,
) -> Response {
    match handle_callback(&cfg, &headers, p).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("sign-in failed: {e:#}");
            (
                StatusCode::FORBIDDEN,
                Html(format!(
                    "<p>Sign-in failed: {e}</p><p><a href=\"/auth/login\">Try again</a></p>"
                )),
            )
                .into_response()
        }
    }
}

async fn handle_callback(cfg: &Config, headers: &HeaderMap, p: CallbackParams) -> Result<Response> {
    if let Some(e) = p.error {
        bail!("Google returned {e}");
    }
    let pending = cookie(headers, OAUTH_COOKIE)
        .and_then(|t| verify(&cfg.session_key, t))
        .context("sign-in expired")?;
    if p.state.as_deref() != pending["state"].as_str() {
        bail!("state mismatch");
    }
    let redirect_uri = format!("{}/auth/callback", cfg.public_url);
    let tokens = exchange_code(
        &cfg.http,
        &cfg.creds,
        p.code.as_deref().context("missing code")?,
        &redirect_uri,
        pending["verifier"].as_str().unwrap_or_default(),
    )
    .await?;
    let clear_oauth = set_cookie(cfg, OAUTH_COOKIE, "", 0);

    if pending["purpose"] == "health" {
        cfg.token_ref
            .write(&tokens.into_stored()?.to_json()?)
            .await?;
        tracing::info!("stored a new Health API refresh token");
        return Ok((
            [(header::SET_COOKIE, clear_oauth)],
            Redirect::to("/?fitbit=connected"),
        )
            .into_response());
    }

    // The ID token comes straight from Google's token endpoint over TLS, so
    // its claims can be trusted without verifying the signature.
    let id_token = tokens.id_token.context("no ID token")?;
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(id_token.split('.').nth(1).context("bad ID token")?)?,
    )?;
    let email = claims["email"].as_str().unwrap_or_default().to_lowercase();
    if claims["email_verified"] != json!(true) || !cfg.allowed_emails.contains(&email) {
        bail!("{email} is not allowed");
    }
    let session =
        json!({ "email": email, "exp": chrono::Utc::now().timestamp() + SESSION_DAYS * 86_400 });
    let mut resp = Redirect::to("/").into_response();
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, clear_oauth.parse()?);
    h.append(
        header::SET_COOKIE,
        set_cookie(
            cfg,
            SESSION_COOKIE,
            &sign(&cfg.session_key, &session),
            SESSION_DAYS * 86_400,
        )
        .parse()?,
    );
    Ok(resp)
}

// --------------------------------------------------------------------- API

async fn overview(State(cfg): State<Shared>) -> Response {
    match cfg
        .bq
        .query(format!(
            "SELECT * FROM {} ORDER BY date",
            cfg.bq.table("daily")
        ))
        .await
    {
        Ok(days) => Json(json!({ "days": days })).into_response(),
        Err(e) => internal(e),
    }
}

async fn day(State(cfg): State<Shared>, Path(date): Path<NaiveDate>) -> Response {
    match day_detail(&cfg.bq, date).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

fn dt(v: &Value) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(v.as_str()?, "%Y-%m-%dT%H:%M:%S").ok()
}

/// Rows of `[ts, value, ...]` arrays for the charts.
fn tuples(rows: Vec<Value>, cols: &[&str]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| Value::Array(cols.iter().map(|c| r[*c].clone()).collect()))
            .collect(),
    )
}

/// Detail of one day. The time range starts at the beginning of the night
/// before (if there was one) so the sleep that ended that morning is shown.
async fn day_detail(bq: &Bq, date: NaiveDate) -> Result<Value> {
    let prev = date - Duration::days(1);
    let midnight = date.and_time(NaiveTime::MIN);
    let end = midnight + Duration::days(1);
    let t = |name| bq.table(name);

    let (nights, metrics) = tokio::try_join!(
        bq.query(format!(
            "SELECT * FROM {} WHERE date = '{date}'",
            t("nights")
        )),
        bq.query(format!(
            "SELECT * FROM {} WHERE date = '{date}'",
            t("daily")
        )),
    )?;
    let night = nights.into_iter().next().map(|mut n| {
        n["stage_minutes"] = json!({
            "DEEP": n["deep_minutes"], "LIGHT": n["light_minutes"],
            "REM": n["rem_minutes"], "AWAKE": n["awake_minutes"],
        });
        n
    });
    let night_window = night
        .as_ref()
        .and_then(|n| Some((dt(&n["start"])?, dt(&n["end"])?)));
    let start = night_window.map_or(midnight, |(s, _)| s.min(midnight));
    // Night-only charts: the sleep window, or the small hours if no sleep was recorded.
    let (nf, nt) = night_window.unwrap_or((midnight, midnight + Duration::hours(8)));
    let fmt = |d: NaiveDateTime| d.format("%Y-%m-%dT%H:%M:%S").to_string();
    let (s, e, nf, nt) = (fmt(start), fmt(end), fmt(nf), fmt(nt));

    let (hr, steps, spo2, hrv, exercises) = tokio::try_join!(
        bq.query(format!(
            "SELECT DATETIME_TRUNC(ts, MINUTE) AS t, MIN(bpm) AS lo, ROUND(AVG(bpm), 1) AS avg, MAX(bpm) AS hi
             FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= '{s}' AND ts < '{e}'
             GROUP BY t ORDER BY t",
            t("heart_rate")
        )),
        bq.query(format!(
            "SELECT DATETIME_ADD(DATETIME_TRUNC(ts, HOUR), INTERVAL DIV(EXTRACT(MINUTE FROM ts), 15) * 15 MINUTE) AS t,
                    SUM(steps) AS steps
             FROM {} WHERE date = '{date}' AND steps IS NOT NULL GROUP BY t ORDER BY t",
            t("activity")
        )),
        bq.query(format!(
            "SELECT ts, pct FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= '{nf}' AND ts < '{nt}' ORDER BY ts",
            t("spo2")
        )),
        bq.query(format!(
            "SELECT ts, rmssd_ms FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= '{nf}' AND ts < '{nt}' ORDER BY ts",
            t("hrv")
        )),
        bq.query(format!(
            "SELECT * FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND start >= '{s}' AND start < '{e}' ORDER BY start",
            t("exercises")
        )),
    )?;

    Ok(json!({
        "date": date,
        "range": [s, e],
        "metrics": metrics.into_iter().next(),
        "heartRate": tuples(hr, &["t", "lo", "avg", "hi"]),
        "steps": tuples(steps, &["t", "steps"]),
        "spo2": tuples(spo2, &["ts", "pct"]),
        "hrv": tuples(hrv, &["ts", "rmssd_ms"]),
        "sleep": night,
        "exercises": exercises,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_tokens_round_trip_and_reject_tampering() {
        let key = b"k";
        let exp = chrono::Utc::now().timestamp() + 60;
        let t = sign(key, &json!({"email": "a@b.c", "exp": exp}));
        assert_eq!(verify(key, &t).unwrap()["email"], "a@b.c");
        assert!(verify(b"other", &t).is_none());
        let (body, sig) = t.split_once('.').unwrap();
        let forged = URL_SAFE_NO_PAD.encode(json!({"email": "x@y.z", "exp": exp}).to_string());
        assert!(verify(key, &format!("{forged}.{sig}")).is_none());
        assert!(verify(key, body).is_none());
        let expired = sign(key, &json!({"email": "a@b.c", "exp": 1}));
        assert!(verify(key, &expired).is_none());
    }

    #[test]
    fn reads_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; fd_session=xyz; b=2".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE), Some("xyz"));
        assert_eq!(cookie(&h, "fd_oauth"), None);
    }
}
