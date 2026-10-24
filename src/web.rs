//! Dashboard: JSON API over the BigQuery tables plus a single page, behind
//! Google sign-in restricted to an allowlist of accounts. The same sign-in
//! guards the MCP endpoint (`mcp.rs`) through a small OAuth server (`authz.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{MatchedPath, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::{get, post};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceBuilder;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::bq::Bq;
use crate::claims::{self, Claims, Purpose};
use crate::cloudrun::SyncJob;
use crate::datatypes::READ_SCOPES;
use crate::oauth::{ClientCredentials, auth_request, exchange_code};
use crate::secrets::SecretRef;
use crate::{authz, mcp};

const SESSION_COOKIE: &str = "fd_session";
const OAUTH_COOKIE: &str = "fd_oauth";
const SESSION_DAYS: i64 = 30;
/// Files from `frontend/public` served without sign-in.
const PUBLIC_FILES: &[&str] = &[
    "favicon.ico",
    "favicon.svg",
    "apple-touch-icon.png",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
    "manifest.webmanifest",
];

pub struct Config {
    pub port: u16,
    /// The built frontend (`frontend/dist`), served as a single-page app.
    pub static_dir: PathBuf,
    pub public_url: String,
    pub allowed_emails: Vec<String>,
    pub session_key: Vec<u8>,
    pub auth_disabled: bool,
    pub creds: ClientCredentials,
    /// Where "Connect Fitbit" stores a new Health API refresh token.
    pub token_ref: SecretRef,
    pub bq: Bq,
    /// The Cloud Run job behind the "sync now" button.
    pub sync_job: SyncJob,
    pub http: reqwest::Client,
}

pub(crate) type Shared = Arc<Config>;

pub async fn serve(cfg: Config) -> Result<()> {
    if !cfg.auth_disabled && cfg.allowed_emails.is_empty() {
        bail!("set ALLOWED_EMAILS (or AUTH_DISABLED=1 for local development)");
    }
    let port = cfg.port;
    let state = Arc::new(cfg);

    // Hashed build assets never change; index.html must always be revalidated.
    let assets = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ))
        .service(ServeDir::new(state.static_dir.join("assets")));
    let spa = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        // Client-side routes (/trends, ...) get index.html.
        .service(
            ServeDir::new(&state.static_dir)
                .fallback(ServeFile::new(state.static_dir.join("index.html"))),
        );

    // Icons and the web manifest are fetched without the session cookie
    // (manifest requests, connector icons in Claude/ChatGPT), so they're public.
    let mut public_files = Router::new();
    for name in PUBLIC_FILES {
        public_files = public_files.route_service(
            &format!("/{name}"),
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=86400"),
                ))
                .service(ServeFile::new(state.static_dir.join(name))),
        );
    }

    let protected = Router::new()
        .route("/api/overview", get(overview))
        .route("/api/day/{date}", get(day))
        .route("/api/sync", get(sync_status).post(start_sync))
        .route("/api/{*rest}", get(|| async { StatusCode::NOT_FOUND }))
        .route("/auth/fitbit", get(connect_fitbit))
        .nest_service("/assets", assets)
        .fallback_service(spa)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    let app = Router::new()
        .merge(protected)
        .merge(public_files)
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .merge(authz::routes())
        .route("/mcp", post(mcp::handle).get(mcp::no_stream))
        .layer(middleware::from_fn(trace_request))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("dashboard listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// One server span per request, continuing the trace Cloud Run started
/// (`traceparent`), so our spans and logs show up under its request.
async fn trace_request(req: Request, next: Next) -> Response {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned());
    let span = tracing::info_span!(
        "request",
        otel.name = format!("{} {}", req.method(), route.as_deref().unwrap_or("*")),
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        http.request.method = %req.method(),
        http.route = route,
        url.path = req.uri().path(),
        http.response.status_code = tracing::field::Empty,
    );
    let parent = opentelemetry::global::get_text_map_propagator(|p| {
        p.extract(&opentelemetry_http::HeaderExtractor(req.headers()))
    });
    let _ = span.set_parent(parent);
    let resp = next.run(req).instrument(span.clone()).await;
    let status = resp.status();
    span.record("http.response.status_code", status.as_u16());
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
    }
    resp
}

fn internal(e: anyhow::Error) -> Response {
    tracing::error!("{e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
}

// ---------------------------------------------------------------- sessions

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

/// The signed-in user, if still allowed.
pub(crate) fn session_email(cfg: &Config, headers: &HeaderMap) -> Option<String> {
    session_of(&cfg.session_key, &cfg.allowed_emails, headers)
}

fn session_of(key: &[u8], allowed: &[String], headers: &HeaderMap) -> Option<String> {
    let Claims::Session { email } = claims::verify(key, cookie(headers, SESSION_COOKIE)?)? else {
        return None;
    };
    allowed.contains(&email).then_some(email)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn require_session(State(cfg): State<Shared>, req: Request, next: Next) -> Response {
    if cfg.auth_disabled || session_email(&cfg, req.headers()).is_some() {
        return next.run(req).await;
    }
    if req.uri().path().starts_with("/api/") {
        StatusCode::UNAUTHORIZED.into_response()
    } else {
        Redirect::to("/auth/login").into_response()
    }
}

/// Starts a Google OAuth flow. After a login the browser returns to `next`.
fn start_oauth(
    cfg: &Config,
    purpose: Purpose,
    scopes: &[&str],
    offline: bool,
    next: &str,
) -> Response {
    let redirect_uri = format!("{}/auth/callback", cfg.public_url);
    let req = match auth_request(&cfg.creds, &redirect_uri, scopes, offline) {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let pending = Claims::Pending {
        state: req.state,
        verifier: req.verifier,
        purpose,
        next: next.to_string(),
    };
    let pending = claims::sign(&cfg.session_key, pending, now() + 600);
    (
        [(
            header::SET_COOKIE,
            set_cookie(cfg, OAUTH_COOKIE, &pending, 600),
        )],
        Redirect::to(req.url.as_str()),
    )
        .into_response()
}

#[derive(Deserialize)]
struct LoginParams {
    next: Option<String>,
}

/// Only paths on this site, so the login cannot be used as an open redirect.
fn local_path(next: Option<&str>) -> &str {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n,
        _ => "/",
    }
}

async fn login(State(cfg): State<Shared>, Query(p): Query<LoginParams>) -> Response {
    let next = local_path(p.next.as_deref());
    start_oauth(&cfg, Purpose::Login, &["openid", "email"], false, next)
}

async fn connect_fitbit(State(cfg): State<Shared>) -> Response {
    start_oauth(&cfg, Purpose::Health, READ_SCOPES, true, "/")
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
                    "<p>Sign-in failed: {}</p><p><a href=\"/auth/login\">Try again</a></p>",
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
    }
}

/// The claims of a Google ID token that sign-in uses (scopes `openid email`).
#[derive(Deserialize)]
struct IdToken {
    email: String,
    #[serde(default)]
    email_verified: bool,
}

async fn handle_callback(cfg: &Config, headers: &HeaderMap, p: CallbackParams) -> Result<Response> {
    if let Some(e) = p.error {
        bail!("Google returned {e}");
    }
    let Some(Claims::Pending {
        state,
        verifier,
        purpose,
        next,
    }) = cookie(headers, OAUTH_COOKIE).and_then(|t| claims::verify(&cfg.session_key, t))
    else {
        bail!("sign-in expired");
    };
    if p.state.as_deref() != Some(state.as_str()) {
        bail!("state mismatch");
    }
    let redirect_uri = format!("{}/auth/callback", cfg.public_url);
    let tokens = exchange_code(
        &cfg.http,
        &cfg.creds,
        p.code.as_deref().context("missing code")?,
        &redirect_uri,
        &verifier,
    )
    .await?;
    let clear_oauth = set_cookie(cfg, OAUTH_COOKIE, "", 0);

    if purpose == Purpose::Health {
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
    let id: IdToken = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(id_token.split('.').nth(1).context("bad ID token")?)?,
    )
    .context("bad ID token")?;
    let email = id.email.to_lowercase();
    if !id.email_verified || !cfg.allowed_emails.contains(&email) {
        bail!("{email} is not allowed");
    }
    let session = claims::sign(
        &cfg.session_key,
        Claims::Session {
            email: email.clone(),
        },
        now() + SESSION_DAYS * 86_400,
    );
    let mut resp = Redirect::to(local_path(Some(&next))).into_response();
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, clear_oauth.parse()?);
    h.append(
        header::SET_COOKIE,
        set_cookie(cfg, SESSION_COOKIE, &session, SESSION_DAYS * 86_400).parse()?,
    );
    Ok(resp)
}

pub(crate) fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// --------------------------------------------------------------------- API

async fn overview(State(cfg): State<Shared>) -> Response {
    let days = cfg.bq.query(format!(
        "SELECT * FROM {} ORDER BY date",
        cfg.bq.table("daily")
    ));
    let runs = cfg.bq.query(format!(
        "SELECT FORMAT_TIMESTAMP('%Y-%m-%dT%H:%M:%S', run_at, 'Europe/Prague') AS run_at, ok, auth_error
         FROM {} ORDER BY run_at DESC LIMIT 1",
        cfg.bq.table("sync_runs")
    ));
    match tokio::try_join!(days, runs) {
        // `lastSync` lets the page offer "reconnect" only when the token broke.
        Ok((days, runs)) => {
            Json(json!({ "days": days, "lastSync": runs.into_iter().next() })).into_response()
        }
        Err(e) => internal(e),
    }
}

/// `{"running": bool}`: whether a sync (manual or scheduled) is in progress.
async fn sync_status(State(cfg): State<Shared>) -> Response {
    match cfg.sync_job.running().await {
        Ok(running) => Json(json!({ "running": running })).into_response(),
        Err(e) => internal(e),
    }
}

/// Starts a sync unless one is already running. The session cookie is
/// SameSite=Lax, so other sites cannot trigger this.
async fn start_sync(State(cfg): State<Shared>) -> Response {
    let result = async {
        if !cfg.sync_job.running().await? {
            cfg.sync_job.run().await?;
            tracing::info!("sync started from the dashboard");
        }
        Ok(())
    };
    match result.await {
        Ok(()) => Json(json!({ "running": true })).into_response(),
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
///
/// All queries run at once: the ones that depend on the night's window read
/// it from the nights table themselves instead of waiting for it here.
pub(crate) async fn day_detail(bq: &Bq, date: NaiveDate) -> Result<Value> {
    let prev = date - Duration::days(1);
    let midnight = date.and_time(NaiveTime::MIN);
    let end = midnight + Duration::days(1);
    let t = |name| bq.table(name);
    let fmt = |d: NaiveDateTime| d.format("%Y-%m-%dT%H:%M:%S").to_string();
    let (m, e, m8) = (fmt(midnight), fmt(end), fmt(midnight + Duration::hours(8)));
    // Scalar subqueries for the shown range start (`s`) and the night-only
    // charts (`nf`..`nt`: the sleep window, or the small hours if none).
    let night = format!(
        "(SELECT AS STRUCT start, `end` FROM {} WHERE date = '{date}' ORDER BY start LIMIT 1)",
        t("nights")
    );
    let s = format!("IFNULL(LEAST({night}.start, DATETIME '{m}'), DATETIME '{m}')");
    let nf = format!("IFNULL({night}.start, DATETIME '{m}')");
    let nt = format!("IFNULL({night}.`end`, DATETIME '{m8}')");

    let (nights, metrics, hr, steps, spo2, hrv, exercises) = tokio::try_join!(
        bq.query(format!(
            "SELECT * FROM {} WHERE date = '{date}' ORDER BY start LIMIT 1",
            t("nights")
        )),
        bq.query(format!(
            "SELECT * FROM {} WHERE date = '{date}'",
            t("daily")
        )),
        bq.query(format!(
            "SELECT DATETIME_TRUNC(ts, MINUTE) AS t, MIN(bpm) AS lo, ROUND(AVG(bpm), 1) AS avg, MAX(bpm) AS hi
             FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= {s} AND ts < '{e}'
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
            "SELECT ts, pct FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= {nf} AND ts < {nt} ORDER BY ts",
            t("spo2")
        )),
        bq.query(format!(
            "SELECT ts, rmssd_ms FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND ts >= {nf} AND ts < {nt} ORDER BY ts",
            t("hrv")
        )),
        bq.query(format!(
            "SELECT * FROM {} WHERE date BETWEEN '{prev}' AND '{date}' AND start >= {s} AND start < '{e}' ORDER BY start",
            t("exercises")
        )),
    )?;

    let night = nights.into_iter().next().map(|mut n| {
        n["stage_minutes"] = json!({
            "DEEP": n["deep_minutes"], "LIGHT": n["light_minutes"],
            "REM": n["rem_minutes"], "AWAKE": n["awake_minutes"],
        });
        n
    });
    let start = night
        .as_ref()
        .and_then(|n| dt(&n["start"]))
        .map_or(midnight, |s| s.min(midnight));

    Ok(json!({
        "date": date,
        "range": [fmt(start), e],
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
    fn only_sessions_are_sessions() {
        let key = b"k";
        let allowed = ["a@b.c".to_string()];
        let exp = now() + 60;
        let check = |c: Claims| {
            let mut h = HeaderMap::new();
            let c = format!("{SESSION_COOKIE}={}", claims::sign(key, c, exp));
            h.insert(header::COOKIE, c.parse().unwrap());
            session_of(key, &allowed, &h)
        };
        let session = |email: &str| Claims::Session {
            email: email.into(),
        };
        assert_eq!(check(session("a@b.c")).as_deref(), Some("a@b.c"));
        assert!(check(session("x@y.z")).is_none());
        // OAuth tokens are signed with the same key.
        let access = Claims::Access {
            email: "a@b.c".into(),
            aud: "x".into(),
        };
        assert!(check(access).is_none());
    }

    #[test]
    fn login_returns_only_to_local_paths() {
        assert_eq!(
            local_path(Some("/oauth/authorize?x=1")),
            "/oauth/authorize?x=1"
        );
        assert_eq!(local_path(Some("//evil.example")), "/");
        assert_eq!(local_path(Some("https://evil.example")), "/");
        assert_eq!(local_path(Some("/\\evil.example")), "/");
        assert_eq!(local_path(None), "/");
    }

    #[test]
    fn reads_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; fd_session=xyz; b=2".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE), Some("xyz"));
        assert_eq!(cookie(&h, "fd_oauth"), None);
    }
}
