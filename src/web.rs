//! Dashboard: JSON API over the users' Parquet tables (`warehouse.rs`) plus a single page, behind
//! Google sign-in restricted to an allowlist of accounts. The same sign-in
//! guards the MCP endpoint (`mcp.rs`) through a small OAuth server (`authz.rs`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{Extension, MatchedPath, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::{delete, get, post, put};
use chrono::{Duration, NaiveDate, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::{Layer, ServiceBuilder, ServiceExt};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::{SetResponseHeader, SetResponseHeaderLayer};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::claims::{self, Claims};
use crate::cloudrun::SyncJob;
use crate::datatypes::READ_SCOPES;
use crate::engine;
use crate::health::StageType;
use crate::kms::Kms;
use crate::model::{self, DailyMetrics, Exercise, NightRow, Time};
use crate::oauth::{ClientCredentials, auth_request, exchange_code};
use crate::status;
use crate::store::Store;
use crate::users::{self, Prefs, User};
use crate::warehouse::{Need, SESSION_MEMORY, Warehouse};
use crate::{api, authz, export, mcp};

const SESSION_COOKIE: &str = "fd_session";
const OAUTH_COOKIE: &str = "fd_oauth";
const SESSION_DAYS: i64 = 30;
/// Where questions and privacy requests go; also on the public pages (`frontend/*.html`).
const CONTACT_EMAIL: &str = "fitdata@jakub.contact";
/// Where the dashboard lives; `/` is always the home page.
const APP: &str = "/dash";
/// Files from `frontend/public` served without sign-in.
const PUBLIC_FILES: &[&str] = &[
    "favicon.ico",
    "favicon.svg",
    "apple-touch-icon.png",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
    "manifest.webmanifest",
    // The home page's link preview (Open Graph), fetched by social networks' crawlers.
    "og.jpg",
    // Shown by Android's install dialog (the manifest's screenshots).
    "screenshot-narrow.webp",
    "screenshot-wide.webp",
];

pub struct Config {
    pub port: u16,
    /// The built frontend (`frontend/dist`), served as a single-page app.
    pub static_dir: PathBuf,
    pub public_url: String,
    pub session_key: Vec<u8>,
    /// Local development: no sign-in, every request is this user.
    pub dev_user: Option<String>,
    pub creds: ClientCredentials,
    /// The user registry and the users' data.
    pub store: Arc<Store>,
    /// The users' tables.
    pub warehouse: Warehouse,
    /// Encrypts the refresh token "Connect Fitbit" stores.
    pub kms: Kms,
    /// The Cloud Run job behind the "sync now" button.
    pub sync_job: SyncJob,
    pub http: reqwest::Client,
}

pub(crate) type Shared = Arc<Config>;

pub async fn serve(cfg: Config) -> Result<()> {
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
        // Client-side routes (/dash/trends, ...) get index.html.
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

    // The home page, privacy policy and terms are public: Google's OAuth
    // verification and visitors who aren't signed in need them. Signed-in
    // users get the dashboard on `/` instead of the home page.
    for (path, file) in [("/privacy", "privacy.html"), ("/terms", "terms.html")] {
        public_files = public_files.route_service(path, page(&state.static_dir, file));
    }
    // The docs are public as well; their pages (/docs/hrv, ...) are routed in the browser.
    for path in ["/docs", "/docs/", "/docs/{*page}"] {
        public_files = public_files.route_service(path, page(&state.static_dir, "docs.html"));
    }
    // The demo is the dashboard on made-up data generated in the browser: public, no API.
    for path in ["/demo", "/demo/", "/demo/{*page}"] {
        public_files = public_files.route_service(path, page(&state.static_dir, "index.html"));
    }
    // The home page is for everyone; signed-in visitors get "Open dashboard" on it
    // (it asks /api/session). Links from before the dashboard moved to /dash go there.
    let home = page(&state.static_dir, "home.html");
    let root = get(move |req: Request| async move {
        match req.uri().query() {
            Some(q) if q.contains("date=") || q.contains("fitbit=") => {
                Redirect::permanent(&format!("{APP}?{q}")).into_response()
            }
            _ => home.oneshot(req).await.into_response(),
        }
    });
    let moved = |to: &'static str| {
        get(move |req: Request| async move {
            let q = req
                .uri()
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default();
            Redirect::permanent(&format!("{to}{q}"))
        })
    };

    let protected = Router::new()
        .route("/api/overview", get(overview))
        .route("/api/day/{date}", get(day))
        .route("/api/sync", get(sync_status).post(start_sync))
        .route("/api/account", delete(delete_account))
        .route("/api/settings", put(save_settings))
        .route("/api/export", get(export::download))
        .route("/api/{*rest}", get(|| async { StatusCode::NOT_FOUND }))
        .route("/auth/fitbit", get(connect_fitbit))
        .fallback_service(spa)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    let app = Router::new()
        .merge(protected)
        .merge(public_files)
        .route("/", root)
        .route("/trends", moved("/dash/trends"))
        .route("/settings", moved("/dash/settings"))
        .route("/api/session", get(session_status))
        // Build assets hold no user data; the public pages need the stylesheet.
        .nest_service("/assets", assets)
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout))
        .merge(authz::routes())
        .route("/mcp", post(mcp::handle).get(mcp::no_stream))
        .layer(middleware::from_fn(trace_request))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("dashboard listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// A built HTML page that is always revalidated, like the dashboard's index.html.
fn page(static_dir: &std::path::Path, file: &str) -> SetResponseHeader<ServeFile, HeaderValue> {
    SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .layer(ServeFile::new(static_dir.join(file)))
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

/// A signed-in user.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SignedIn {
    /// The Google Health user ID.
    pub uid: String,
}

/// The user behind signed claims, if their account still exists: a deleted
/// account's sessions and MCP tokens stop working.
pub(crate) async fn resolve(cfg: &Config, uid: &str) -> Option<SignedIn> {
    match users::get(&cfg.store, uid).await {
        Ok(Some(u)) if !u.deleted => Some(SignedIn { uid: u.uid }),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("reading user {uid}: {e:#}");
            None
        }
    }
}

/// The user ID of a session cookie signed with `key`.
fn session_uid(key: &[u8], headers: &HeaderMap) -> Option<String> {
    match claims::verify(key, cookie(headers, SESSION_COOKIE)?)? {
        Claims::Session { uid } => Some(uid),
        _ => None,
    }
}

/// The signed-in user, if their account still exists.
pub(crate) async fn session_user(cfg: &Config, headers: &HeaderMap) -> Option<SignedIn> {
    if let Some(uid) = &cfg.dev_user {
        return Some(SignedIn { uid: uid.clone() });
    }
    resolve(cfg, &session_uid(&cfg.session_key, headers)?).await
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn require_session(State(cfg): State<Shared>, mut req: Request, next: Next) -> Response {
    if let Some(user) = session_user(&cfg, req.headers()).await {
        req.extensions_mut().insert(user);
        return next.run(req).await;
    }
    if req.uri().path().starts_with("/api/") {
        StatusCode::UNAUTHORIZED.into_response()
    } else {
        Redirect::to("/auth/login").into_response()
    }
}

/// Starts the Google sign-in. It asks only for the Health scopes: the user is
/// who the Health API says (`users/me/identity`), so there is no `openid` or
/// `email` and a single consent screen also connects Fitbit. `renew` forces the
/// consent screen for a new refresh token (Connect Fitbit, Restore access).
/// After the callback the browser returns to `next`.
fn start_oauth(cfg: &Config, renew: bool, next: &str, tz: Option<String>) -> Response {
    let redirect_uri = format!("{}/auth/callback", cfg.public_url);
    let req = match auth_request(&cfg.creds, &redirect_uri, READ_SCOPES, renew) {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let pending = Claims::Pending {
        state: req.state,
        verifier: req.verifier,
        next: next.to_string(),
        tz,
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
    /// The browser's time zone (the sign-in links on the public pages add it).
    tz: Option<String>,
}

/// Only paths on this site, so the login cannot be used as an open redirect.
fn local_path(next: Option<&str>) -> &str {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n,
        _ => APP,
    }
}

/// Whether the browser is signed in (204) or not (401), for the public pages'
/// "Open dashboard" links. Says nothing else.
async fn session_status(State(cfg): State<Shared>, headers: HeaderMap) -> StatusCode {
    if session_user(&cfg, &headers).await.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn login(State(cfg): State<Shared>, Query(p): Query<LoginParams>) -> Response {
    let next = local_path(p.next.as_deref());
    let tz = p.tz.filter(|tz| users::check_time_zone(tz).is_ok());
    start_oauth(&cfg, false, next, tz)
}

async fn connect_fitbit(State(cfg): State<Shared>) -> Response {
    start_oauth(&cfg, true, APP, None)
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
                    r#"<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign-in failed · Fitdata</title>
<style>body{{font:15px/1.5 system-ui,sans-serif;max-width:32rem;margin:15vh auto 0;padding:0 16px}}</style>
<p>Sign-in failed: {}</p>
<p>Questions? Write to <a href="mailto:{CONTACT_EMAIL}">{CONTACT_EMAIL}</a>.</p>
<p><a href="/auth/login">Try again</a> · <a href="/">Home</a></p></html>"#,
                    escape_html(&e.to_string())
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
    let Some(Claims::Pending {
        state,
        verifier,
        next,
        tz,
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

    // Who signed in. This needs at least one Health scope; without any
    // (all unticked on the consent screen) there is nothing to show anyway.
    let uid = match api::health_user_id(&cfg.http, &tokens.access_token).await {
        Ok(uid) => uid,
        Err(e) => {
            tracing::info!("no Health identity: {e:#}");
            bail!(
                "Fitdata needs access to at least some of your Health data. Sign in again and tick the boxes on Google's screen"
            );
        }
    };
    users::check_uid(&uid)?;
    match users::get(&cfg.store, &uid).await? {
        Some(u) if !u.deleted => {}
        // "Delete account" is final: signing in again does not cancel it,
        // the old data goes and only then can the user start afresh.
        Some(_) => bail!("your account is still being deleted. Try again in a minute"),
        None => {
            let mut user = User::new(&uid);
            if let Some(tz) = tz.filter(|tz| users::check_time_zone(tz).is_ok()) {
                user.time_zone = tz;
            }
            users::put(&cfg.store, &user).await?;
            tracing::info!(%uid, "new user");
        }
    }

    // A refresh token comes with the first consent (and every forced one);
    // a returning user goes straight through and keeps the stored one.
    let connected = tokens.refresh_token.is_some();
    // A user who has synced before is restoring lost access; a new user's
    // first sync has its own card on the dashboard.
    let reconnected = connected && status::get(&cfg.store, &uid).await?.last_run.is_some();
    if connected {
        let token = tokens.into_stored()?;
        users::save_token(&cfg.store, &cfg.kms, &uid, &token).await?;
        tracing::info!(%uid, "stored a new Health API refresh token");
        // Fetch the data now rather than at the next full hour; for a new
        // user that is their whole history.
        if let Err(e) = cfg.sync_job.run(Some(&uid)).await {
            tracing::warn!("starting the sync failed: {e:#}");
        }
    }
    let session = claims::sign(
        &cfg.session_key,
        Claims::Session { uid },
        now() + SESSION_DAYS * 86_400,
    );
    let next = match local_path(Some(&next)) {
        // The dashboard confirms restored access (ReconnectBanner).
        APP if reconnected => "/dash?fitbit=reconnected",
        n => n,
    };
    let mut resp = Redirect::to(next).into_response();
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, clear_oauth.parse()?);
    h.append(
        header::SET_COOKIE,
        set_cookie(cfg, SESSION_COOKIE, &session, SESSION_DAYS * 86_400).parse()?,
    );
    Ok(resp)
}

/// Signs out to the home page, which doesn't sign the browser back in.
async fn logout(State(cfg): State<Shared>) -> Response {
    (
        [(header::SET_COOKIE, set_cookie(&cfg, SESSION_COOKIE, "", 0))],
        Redirect::to("/"),
    )
        .into_response()
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
    user: Me,
}

/// The user's last sync run (or the last successful one).
pub(crate) async fn last_run(store: &Store, uid: &str, ok_only: bool) -> Result<Option<LastRun>> {
    let s = status::get(store, uid).await?;
    let rfc3339 = |t: chrono::DateTime<chrono::Utc>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
    Ok(if ok_only {
        s.last_ok.map(|t| LastRun {
            run_at: rfc3339(t),
            ok: true,
            auth_error: false,
        })
    } else {
        s.last_run.map(|r| LastRun {
            run_at: rfc3339(r.at),
            ok: r.ok,
            auth_error: r.auth_error,
        })
    })
}

/// Who is signed in, for the page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    /// The Google Health user ID, for support requests.
    id: String,
    /// Fitbit is connected (a refresh token is stored).
    connected: bool,
    settings: Settings,
    /// The preferences were chosen (or detected from the browser) at least once.
    settings_saved: bool,
}

/// The Settings page's values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    /// IANA name; local dates follow it.
    time_zone: String,
    #[serde(flatten)]
    prefs: Prefs,
}

impl Me {
    fn new(user: Option<User>, uid: &str) -> Self {
        let user = user.unwrap_or_else(|| User::new(uid));
        Me {
            id: user.uid.clone(),
            connected: user.token.is_some(),
            settings: Settings {
                time_zone: user.tz().name().to_string(),
                prefs: user.prefs.unwrap_or_default(),
            },
            settings_saved: user.prefs.is_some(),
        }
    }
}

async fn overview(State(cfg): State<Shared>, Extension(user): Extension<SignedIn>) -> Response {
    let days = async {
        let ctx = cfg.warehouse.session(&user.uid, Need::None).await?;
        engine::query_as::<DailyMetrics>(&ctx, "SELECT * FROM daily ORDER BY date").await
    };
    let record = users::get(&cfg.store, &user.uid);
    match tokio::try_join!(days, last_run(&cfg.store, &user.uid, false), record) {
        Ok((days, last_sync, record)) => Json(Overview {
            days,
            last_sync,
            user: Me::new(record, &user.uid),
        })
        .into_response(),
        Err(e) => internal(e),
    }
}

/// `{"running": bool, "progress": …}`: whether a sync of the user is in
/// progress, and how far a first sync has got.
async fn sync_status(State(cfg): State<Shared>, Extension(user): Extension<SignedIn>) -> Response {
    let result = async {
        let s = status::get(&cfg.store, &user.uid).await?;
        if s.running(chrono::Utc::now()) {
            return Ok(json!({ "running": true, "progress": s.progress }));
        }
        Ok::<_, anyhow::Error>(json!({ "running": job_running(&cfg, &s).await? }))
    };
    match result.await {
        Ok(state) => Json(state).into_response(),
        Err(e) => internal(e),
    }
}

async fn running(cfg: &Config, uid: &str) -> Result<bool> {
    let s = status::get(&cfg.store, uid).await?;
    Ok(s.running(chrono::Utc::now()) || job_running(cfg, &s).await?)
}

/// Whether a running job execution has yet to sync the user: one that was
/// just started has not written their status yet. Once their run in it has
/// finished, the execution may go on with others (or shut down) for a while,
/// but the user is done.
async fn job_running(cfg: &Config, s: &status::Status) -> Result<bool> {
    let since = cfg.sync_job.running_since().await?;
    Ok(since.is_some_and(|t| s.last_run.as_ref().is_none_or(|r| r.at < t)))
}

/// Starts a sync of the user unless one is already running. The session
/// cookie is SameSite=Lax, so other sites cannot trigger this.
async fn start_sync(State(cfg): State<Shared>, Extension(user): Extension<SignedIn>) -> Response {
    let result = async {
        if !running(&cfg, &user.uid).await? {
            cfg.sync_job.run(Some(&user.uid)).await?;
            tracing::info!(uid = %user.uid, "sync started from the dashboard");
        }
        Ok(())
    };
    match result.await {
        Ok(()) => Json(json!({ "running": true })).into_response(),
        Err(e) => internal(e),
    }
}

/// Saves the Settings page. A new time zone moves every local date, so the
/// sync job rebuilds the tables (`job::sync_user`); it is started now unless
/// a run is in progress, which leaves it to the next one.
async fn save_settings(
    State(cfg): State<Shared>,
    Extension(user): Extension<SignedIn>,
    Json(new): Json<Settings>,
) -> Response {
    let tz = match users::check_time_zone(&new.time_zone) {
        Ok(tz) => tz,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
    };
    let result = async {
        let mut u = users::get(&cfg.store, &user.uid)
            .await?
            .context("no such user")?;
        if u.deleted {
            bail!("the account is being deleted");
        }
        let moved = u.tz() != tz;
        u.time_zone = tz.name().to_string();
        u.prefs = Some(new.prefs);
        users::put(&cfg.store, &u).await?;
        if moved && u.token.is_some() {
            // Saved either way: if the job does not start, the next hourly run rebuilds.
            let start = async {
                if !running(&cfg, &user.uid).await? {
                    cfg.sync_job.run(Some(&user.uid)).await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            match start.await {
                Ok(()) => tracing::info!(uid = %user.uid, %tz, "time zone changed, sync started"),
                Err(e) => {
                    tracing::warn!("starting the sync after a time zone change failed: {e:#}")
                }
            }
        }
        Ok::<_, anyhow::Error>(Me::new(Some(u), &user.uid))
    };
    match result.await {
        Ok(me) => Json(me).into_response(),
        Err(e) => internal(e),
    }
}

/// "Delete account": marks the user for deletion and starts the job, which
/// revokes the Fitbit token and removes the data (the web may not).
async fn delete_account(
    State(cfg): State<Shared>,
    Extension(user): Extension<SignedIn>,
) -> Response {
    let result = async {
        if let Some(mut u) = users::get(&cfg.store, &user.uid).await? {
            u.deleted = true;
            users::put(&cfg.store, &u).await?;
        }
        cfg.sync_job.run(Some(&user.uid)).await?;
        tracing::info!(uid = %user.uid, "account deletion requested");
        Ok::<_, anyhow::Error>(())
    };
    match result.await {
        Ok(()) => (
            [(header::SET_COOKIE, set_cookie(&cfg, SESSION_COOKIE, "", 0))],
            Json(json!({ "deleted": true })),
        )
            .into_response(),
        Err(e) => internal(e),
    }
}

async fn day(
    State(cfg): State<Shared>,
    Extension(user): Extension<SignedIn>,
    Path(date): Path<NaiveDate>,
) -> Response {
    match day_detail(&cfg.warehouse, &user.uid, date).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

/// Heart rate in one minute: `[time, min, mean, max]` in JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(into = "(Time, f64, f64, f64)")]
pub(crate) struct HrMinute {
    pub t: Time,
    pub lo: f64,
    pub avg: f64,
    pub hi: f64,
}

impl From<HrMinute> for (Time, f64, f64, f64) {
    fn from(m: HrMinute) -> Self {
        (m.t, m.lo, m.avg, m.hi)
    }
}

/// A timed value: `[time, value]` in JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(into = "(Time, f64)")]
pub(crate) struct Reading {
    pub t: Time,
    pub v: f64,
}

impl From<Reading> for (Time, f64) {
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
    /// The user's time zone (IANA name), which the times' offsets follow.
    pub time_zone: String,
    /// The time axis: from midnight, or from the start of the night if earlier.
    pub range: [Time; 2],
    /// The midnight `date` starts at.
    pub midnight: Time,
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
    let snapshot = wh.snapshot(uid, Need::Range(prev, date)).await?;
    let c = &snapshot.session(SESSION_MEMORY).await?;
    let tz = snapshot.tz;
    let midnight = model::midnight(date, tz);
    // Not midnight + 24 h: the day clocks change is 23 or 25 hours long.
    let end = model::midnight(date + Duration::days(1), tz);

    let night = engine::query_as::<NightRow>(
        c,
        &format!("SELECT * FROM nights WHERE date = DATE '{date}'"),
    )
    .await?
    .into_iter()
    .next();
    let metrics = engine::query_as::<DailyMetrics>(
        c,
        &format!("SELECT * FROM daily WHERE date = DATE '{date}'"),
    )
    .await?
    .into_iter()
    .next();
    let start = night.as_ref().map_or(midnight, |n| n.start.min(midnight));
    // Night-only charts: the sleep window, or the small hours if no sleep was recorded.
    let (nf, nt) = night
        .as_ref()
        .map_or((midnight, midnight + Duration::hours(8)), |n| {
            (n.start, n.end)
        });
    let fmt = |t: Time| t.to_rfc3339();
    let (s, e, nf, nt) = (fmt(start), fmt(end), fmt(nf), fmt(nt));
    let days = format!("date BETWEEN DATE '{prev}' AND DATE '{date}'");

    let heart_rate = engine::query_as(
        c,
        &format!(
            "SELECT date_trunc('minute', ts) AS t, MIN(bpm) AS lo, ROUND(AVG(bpm), 1) AS avg, MAX(bpm) AS hi
             FROM heart_rate WHERE {days} AND ts >= '{s}' AND ts < '{e}'
             GROUP BY t ORDER BY t"
        ),
    )
    .await?;
    let steps = engine::query_as(
        c,
        &format!(
            "SELECT date_bin(INTERVAL '15 minutes', ts) AS t, SUM(steps) AS v
             FROM activity WHERE date = DATE '{date}' AND steps IS NOT NULL GROUP BY t ORDER BY t"
        ),
    )
    .await?;
    let spo2 = engine::query_as(
        c,
        &format!(
            "SELECT ts AS t, pct AS v FROM spo2
             WHERE {days} AND ts >= '{nf}' AND ts < '{nt}' ORDER BY ts"
        ),
    )
    .await?;
    let hrv = engine::query_as(
        c,
        &format!(
            "SELECT ts AS t, rmssd_ms AS v FROM hrv
             WHERE {days} AND ts >= '{nf}' AND ts < '{nt}' ORDER BY ts"
        ),
    )
    .await?;
    let exercises = engine::query_as(
        c,
        &format!(
            r#"SELECT * FROM exercises
             WHERE {days} AND "start" >= '{s}' AND "start" < '{e}' ORDER BY "start""#
        ),
    )
    .await?;

    Ok(DayDetail {
        date,
        time_zone: tz.name().to_string(),
        range: [start, end],
        midnight,
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
        let exp = now() + 60;
        let check = |c: Claims| {
            let mut h = HeaderMap::new();
            let c = format!("{SESSION_COOKIE}={}", claims::sign(key, c, exp));
            h.insert(header::COOKIE, c.parse().unwrap());
            session_uid(key, &h)
        };
        assert_eq!(check(Claims::Session { uid: "1".into() }), Some("1".into()));
        // OAuth tokens are signed with the same key.
        let access = Claims::Access {
            uid: "1".into(),
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
        assert_eq!(local_path(Some("//evil.example")), APP);
        assert_eq!(local_path(Some("https://evil.example")), APP);
        assert_eq!(local_path(Some("/\\evil.example")), APP);
        assert_eq!(local_path(None), APP);
    }

    #[test]
    fn reads_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; fd_session=xyz; b=2".parse().unwrap());
        assert_eq!(cookie(&h, SESSION_COOKIE), Some("xyz"));
        assert_eq!(cookie(&h, "fd_oauth"), None);
    }
}
