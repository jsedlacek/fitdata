//! Dashboard: JSON API over the users' Parquet tables (`warehouse.rs`) plus a single page, behind
//! Google sign-in restricted to an allowlist of accounts. The same sign-in
//! guards the MCP endpoint (`mcp.rs`) through a small OAuth server (`authz.rs`).

use std::collections::BTreeMap;
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
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::bq::Bq;
use crate::claims::{self, Claims, Purpose};
use crate::cloudrun::SyncJob;
use crate::datatypes::READ_SCOPES;
use crate::duck;
use crate::health::StageType;
use crate::model::{DailyMetrics, Exercise, NightRow};
use crate::oauth::{ClientCredentials, auth_request, exchange_code};
use crate::query::QueryClient;
use crate::secrets::SecretRef;
use crate::warehouse::{Need, Warehouse};
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
    /// Only for the sync history (`sync_runs`).
    pub bq: Bq,
    /// The users' tables.
    pub warehouse: Warehouse,
    /// Where MCP `query` SQL runs.
    pub query: QueryClient,
    /// The one user whose data is shown (until there are more).
    pub owner: String,
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

/// The last run of the sync job; the page offers "reconnect" only when the
/// token broke.
#[derive(Serialize, Deserialize)]
pub(crate) struct LastRun {
    /// Local time, `YYYY-MM-DDTHH:MM:SS`.
    pub run_at: String,
    pub ok: bool,
    pub auth_error: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    days: Vec<DailyMetrics>,
    last_sync: Option<LastRun>,
}

/// The newest run in `sync_runs` (only successful ones if `ok_only`).
pub(crate) async fn last_run(bq: &Bq, ok_only: bool) -> Result<Option<LastRun>> {
    let runs = bq
        .query_as(format!(
            "SELECT FORMAT_TIMESTAMP('%Y-%m-%dT%H:%M:%S', run_at, 'Europe/Prague') AS run_at, ok, auth_error
             FROM {} {} ORDER BY run_at DESC LIMIT 1",
            bq.table("sync_runs"),
            if ok_only { "WHERE ok" } else { "" },
        ))
        .await?;
    Ok(runs.into_iter().next())
}

async fn overview(State(cfg): State<Shared>) -> Response {
    let days = cfg.warehouse.with(&cfg.owner, Need::None, |c| {
        duck::query_as::<DailyMetrics>(c, "SELECT * FROM daily ORDER BY date")
    });
    match tokio::try_join!(days, last_run(&cfg.bq, false)) {
        Ok((days, last_sync)) => Json(Overview { days, last_sync }).into_response(),
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
    match day_detail(&cfg.warehouse, &cfg.owner, date).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

/// Heart rate in one minute: `[time, min, mean, max]` in JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(into = "(NaiveDateTime, f64, f64, f64)")]
pub(crate) struct HrMinute {
    pub t: NaiveDateTime,
    pub lo: f64,
    pub avg: f64,
    pub hi: f64,
}

impl From<HrMinute> for (NaiveDateTime, f64, f64, f64) {
    fn from(m: HrMinute) -> Self {
        (m.t, m.lo, m.avg, m.hi)
    }
}

/// A timed value: `[time, value]` in JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(into = "(NaiveDateTime, f64)")]
pub(crate) struct Reading {
    pub t: NaiveDateTime,
    pub v: f64,
}

impl From<Reading> for (NaiveDateTime, f64) {
    fn from(r: Reading) -> Self {
        (r.t, r.v)
    }
}

/// The night with its stage minutes keyed by stage, as the page shows them.
#[derive(Debug, Serialize)]
pub(crate) struct NightDetail {
    #[serde(flatten)]
    pub night: NightRow,
    pub stage_minutes: BTreeMap<StageType, Option<i64>>,
}

/// Detail of one day for the day page and the `get_day` MCP tool.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DayDetail {
    pub date: NaiveDate,
    /// The time axis: from midnight, or from the start of the night if earlier.
    pub range: [NaiveDateTime; 2],
    pub metrics: Option<DailyMetrics>,
    pub heart_rate: Vec<HrMinute>,
    /// Steps per 15 minutes.
    pub steps: Vec<Reading>,
    pub spo2: Vec<Reading>,
    pub hrv: Vec<Reading>,
    pub sleep: Option<NightDetail>,
    pub exercises: Vec<Exercise>,
}

/// Detail of one day. The time range starts at the beginning of the night
/// before (if there was one) so the sleep that ended that morning is shown.
pub(crate) async fn day_detail(wh: &Warehouse, uid: &str, date: NaiveDate) -> Result<DayDetail> {
    let prev = date - Duration::days(1);
    wh.with(uid, Need::Range(prev, date), move |c| {
        day_detail_sql(c, date)
    })
    .await
}

fn day_detail_sql(c: &duckdb::Connection, date: NaiveDate) -> Result<DayDetail> {
    let prev = date - Duration::days(1);
    let midnight = date.and_time(NaiveTime::MIN);
    let end = midnight + Duration::days(1);

    let night = duck::query_as::<NightRow>(
        c,
        &format!("SELECT * FROM nights WHERE date = DATE '{date}'"),
    )?
    .into_iter()
    .next();
    let metrics = duck::query_as::<DailyMetrics>(
        c,
        &format!("SELECT * FROM daily WHERE date = DATE '{date}'"),
    )?
    .into_iter()
    .next();
    let start = night.as_ref().map_or(midnight, |n| n.start.min(midnight));
    // Night-only charts: the sleep window, or the small hours if no sleep was recorded.
    let (nf, nt) = night
        .as_ref()
        .map_or((midnight, midnight + Duration::hours(8)), |n| {
            (n.start, n.end)
        });
    let fmt = |d: NaiveDateTime| d.format("%Y-%m-%d %H:%M:%S").to_string();
    let (s, e, nf, nt) = (fmt(start), fmt(end), fmt(nf), fmt(nt));
    let days = format!("date BETWEEN DATE '{prev}' AND DATE '{date}'");

    let heart_rate = duck::query_as(
        c,
        &format!(
            "SELECT date_trunc('minute', ts) AS t, MIN(bpm) AS lo, ROUND(AVG(bpm), 1) AS avg, MAX(bpm) AS hi
             FROM heart_rate WHERE {days} AND ts >= TIMESTAMP '{s}' AND ts < TIMESTAMP '{e}'
             GROUP BY t ORDER BY t"
        ),
    )?;
    let steps = duck::query_as(
        c,
        &format!(
            "SELECT time_bucket(INTERVAL 15 MINUTE, ts) AS t, SUM(steps) AS v
             FROM activity WHERE date = DATE '{date}' AND steps IS NOT NULL GROUP BY t ORDER BY t"
        ),
    )?;
    let spo2 = duck::query_as(
        c,
        &format!(
            "SELECT ts AS t, pct AS v FROM spo2
             WHERE {days} AND ts >= TIMESTAMP '{nf}' AND ts < TIMESTAMP '{nt}' ORDER BY ts"
        ),
    )?;
    let hrv = duck::query_as(
        c,
        &format!(
            "SELECT ts AS t, rmssd_ms AS v FROM hrv
             WHERE {days} AND ts >= TIMESTAMP '{nf}' AND ts < TIMESTAMP '{nt}' ORDER BY ts"
        ),
    )?;
    let exercises = duck::query_as(
        c,
        &format!(
            "SELECT * FROM exercises
             WHERE {days} AND start >= TIMESTAMP '{s}' AND start < TIMESTAMP '{e}' ORDER BY start"
        ),
    )?;

    Ok(DayDetail {
        date,
        range: [start, end],
        metrics,
        heart_rate,
        steps,
        spo2,
        hrv,
        sleep: night.map(|n| NightDetail {
            stage_minutes: BTreeMap::from([
                (StageType::Deep, n.deep_minutes),
                (StageType::Light, n.light_minutes),
                (StageType::Rem, n.rem_minutes),
                (StageType::Awake, n.awake_minutes),
            ]),
            night: n,
        }),
        exercises,
    })
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
