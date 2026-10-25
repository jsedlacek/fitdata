//! OAuth 2.1 authorization server for MCP clients (Claude, ChatGPT).
//!
//! Users sign in with the dashboard's Google login and then approve the
//! client on a consent page. Nothing is stored: client IDs, codes and tokens
//! are all signed claims (`claims.rs`), so rotating `session-key` revokes
//! every client and token at once. The price is that codes are not single
//! use and tokens cannot be revoked one by one.
//!
//! Metadata: RFC 9728 (protected resource), RFC 8414 (authorization server);
//! clients register themselves (RFC 7591) and must use PKCE (S256).

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::{get, post};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::claims::{self, Claims, ClientAuth};
use crate::web::{Config, Shared, SignedIn, escape_html, resolve, session_user};

const CODE_SECS: i64 = 300;
const ACCESS_SECS: i64 = 3600;
const REFRESH_SECS: i64 = 30 * 86_400;
/// Client registrations do not expire in practice.
const CLIENT_SECS: i64 = 10 * 365 * 86_400;
pub const SCOPE: &str = "health.read";

pub fn routes() -> Router<Shared> {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(server_metadata),
        )
        .route("/.well-known/openid-configuration", get(server_metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize).post(approve))
        .route("/oauth/token", post(token))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The `resource` (RFC 8707) that access tokens are issued for.
pub fn resource(cfg: &Config) -> String {
    format!("{}/mcp", cfg.public_url)
}

fn verify(cfg: &Config, token: &str) -> Option<Claims> {
    claims::verify(&cfg.session_key, token)
}

fn sign(cfg: &Config, c: Claims, exp: i64) -> String {
    claims::sign(&cfg.session_key, c, exp)
}

/// The registration behind a client ID.
struct Client {
    redirect_uris: Vec<String>,
    name: String,
    auth: ClientAuth,
}

fn verify_client(cfg: &Config, client_id: &str) -> Option<Client> {
    match verify(cfg, client_id)? {
        Claims::Client {
            redirect_uris,
            name,
            auth,
        } => Some(Client {
            redirect_uris,
            name,
            auth,
        }),
        _ => None,
    }
}

/// The user behind a bearer access token, if it is still valid.
pub async fn bearer_user(cfg: &Config, headers: &HeaderMap) -> Option<SignedIn> {
    if let Some(uid) = &cfg.dev_user {
        return Some(SignedIn { uid: uid.clone() });
    }
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let Claims::Access { uid, aud } = verify(cfg, token.trim())? else {
        return None;
    };
    if aud != resource(cfg) {
        return None;
    }
    resolve(cfg, &uid).await
}

/// 401 that tells an MCP client where to find the authorization server.
pub fn unauthorized(cfg: &Config) -> Response {
    let meta = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"",
        cfg.public_url
    );
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, meta)],
        Json(json!({ "error": "invalid_token" })),
    )
        .into_response()
}

async fn resource_metadata(State(cfg): State<Shared>) -> Json<Value> {
    Json(json!({
        "resource": resource(&cfg),
        "authorization_servers": [cfg.public_url],
        "scopes_supported": [SCOPE],
        "bearer_methods_supported": ["header"],
        "resource_name": "Fitdata",
    }))
}

async fn server_metadata(State(cfg): State<Shared>) -> Json<Value> {
    let u = &cfg.public_url;
    Json(json!({
        "issuer": u,
        "authorization_endpoint": format!("{u}/oauth/authorize"),
        "token_endpoint": format!("{u}/oauth/token"),
        "registration_endpoint": format!("{u}/oauth/register"),
        "scopes_supported": [SCOPE],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ClientAuth::ALL.map(ClientAuth::as_str),
    }))
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "error": error, "error_description": description })),
    )
        .into_response()
}

// ------------------------------------------------------------ registration

/// Redirect URIs must be HTTPS, or loopback HTTP for local clients.
fn valid_redirect_uri(uri: &str) -> bool {
    let Ok(u) = Url::parse(uri) else {
        return false;
    };
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    u.fragment().is_none() && (u.scheme() == "https" || (u.scheme() == "http" && loopback))
}

/// A confidential client's secret is derived from its ID, so it need not be stored.
fn client_secret(cfg: &Config, client_id: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&cfg.session_key).expect("any key length");
    mac.update(b"client-secret:");
    mac.update(client_id.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

#[derive(Deserialize)]
struct Registration {
    #[serde(default)]
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
}

async fn register(State(cfg): State<Shared>, Json(r): Json<Registration>) -> Response {
    if r.redirect_uris.is_empty() || !r.redirect_uris.iter().all(|u| valid_redirect_uri(u)) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "redirect URIs must be https (or http on localhost)",
        );
    }
    let Some(auth) = ClientAuth::parse(r.token_endpoint_auth_method.as_deref().unwrap_or("none"))
    else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "unsupported token_endpoint_auth_method",
        );
    };
    let name: String = r
        .client_name
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect();
    let issued = now();
    let client_id = sign(
        &cfg,
        Claims::Client {
            redirect_uris: r.redirect_uris.clone(),
            name: name.clone(),
            auth,
        },
        issued + CLIENT_SECS,
    );
    let mut body = json!({
        "client_id": client_id,
        "client_id_issued_at": issued,
        "client_name": name,
        "redirect_uris": r.redirect_uris,
        "token_endpoint_auth_method": auth.as_str(),
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "scope": SCOPE,
    });
    if auth != ClientAuth::None {
        body["client_secret"] = json!(client_secret(&cfg, &client_id));
        body["client_secret_expires_at"] = json!(0);
    }
    tracing::info!("registered MCP client {name:?}");
    (StatusCode::CREATED, Json(body)).into_response()
}

// ----------------------------------------------------------- authorization

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

/// A validated authorization request, before the user has approved it.
struct AuthRequest {
    client_name: String,
    redirect_uri: String,
    state: Option<String>,
    challenge: String,
    client_id: String,
}

/// Why an authorization request was refused: before the redirect URI is
/// trusted the user sees a page, afterwards the client gets the error.
enum Refusal {
    Page(&'static str),
    Redirect(String),
}

fn check_authorize(cfg: &Config, p: AuthorizeParams) -> Result<AuthRequest, Refusal> {
    let client_id = p.client_id.ok_or(Refusal::Page("missing client_id"))?;
    let client = verify_client(cfg, &client_id).ok_or(Refusal::Page(
        "unknown client; remove the connector and add it again",
    ))?;
    let uris = &client.redirect_uris;
    // With a single registered URI the parameter may be omitted.
    let redirect_uri = match p.redirect_uri {
        Some(u) if uris.contains(&u) => u,
        None if uris.len() == 1 => uris[0].clone(),
        _ => {
            return Err(Refusal::Page(
                "redirect_uri does not match the registration",
            ));
        }
    };
    let refuse = |error| {
        Refusal::Redirect(redirect_with(
            &redirect_uri,
            &[("error", error)],
            p.state.as_deref(),
        ))
    };
    if p.response_type.as_deref() != Some("code") {
        return Err(refuse("unsupported_response_type"));
    }
    let challenge = p.code_challenge.unwrap_or_default();
    if challenge.len() < 43 || p.code_challenge_method.as_deref() != Some("S256") {
        return Err(refuse("invalid_request"));
    }
    Ok(AuthRequest {
        client_name: client.name,
        redirect_uri,
        state: p.state,
        challenge,
        client_id,
    })
}

fn redirect_with(uri: &str, params: &[(&str, &str)], state: Option<&str>) -> String {
    let mut u = Url::parse(uri).expect("validated at registration");
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in params {
            q.append_pair(k, v);
        }
        if let Some(s) = state {
            q.append_pair("state", s);
        }
    }
    u.to_string()
}

fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Html(format!(
            "<p>Invalid authorization request: {}</p>",
            escape_html(msg)
        )),
    )
        .into_response()
}

async fn authorize(
    State(cfg): State<Shared>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    Query(p): Query<AuthorizeParams>,
) -> Response {
    let req = match check_authorize(&cfg, p) {
        Ok(r) => r,
        Err(Refusal::Page(msg)) => return bad_request(msg),
        Err(Refusal::Redirect(url)) => return Redirect::to(&url).into_response(),
    };
    let Some(user) = session_user(&cfg, &headers).await else {
        // Sign in with Google first, then come back here.
        let next = uri.path_and_query().map_or("/", |p| p.as_str());
        let login =
            Url::parse_with_params("http://x/auth/login", [("next", next)]).expect("static URL");
        return Redirect::to(&format!(
            "/auth/login?{}",
            login.query().unwrap_or_default()
        ))
        .into_response();
    };
    // The form carries the validated request, signed, so the POST needs no state.
    let consent = sign(
        &cfg,
        Claims::Consent {
            client_id: req.client_id,
            redirect_uri: req.redirect_uri.clone(),
            state: req.state,
            challenge: req.challenge,
            uid: user.uid.clone(),
        },
        now() + 600,
    );
    let host = Url::parse(&req.redirect_uri)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let name = if req.client_name.is_empty() {
        "An application".to_string()
    } else {
        req.client_name.clone()
    };
    consent_page(&name, &host, &consent)
}

fn consent_page(name: &str, host: &str, consent: &str) -> Response {
    let (name, host, consent) = (escape_html(name), escape_html(host), escape_html(consent));
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Access to your data · Fitdata</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<style>
  :root {{ color-scheme: light dark; --bg: #f6f7f9; --card: #fff; --fg: #111827; --muted: #6b7280; --accent: #2563eb; --border: #e5e7eb; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --bg: #0b0f17; --card: #131926; --fg: #e5e7eb; --muted: #9ca3af; --accent: #3b82f6; --border: #273042; }} }}
  body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg); color: var(--fg);
         font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }}
  main {{ background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 28px; margin: 16px; max-width: 420px; }}
  h1 {{ font-size: 18px; margin: 0 0 12px; }}
  p {{ margin: 0 0 12px; }}
  .muted {{ color: var(--muted); font-size: 13px; }}
  code {{ font-size: 13px; }}
  .buttons {{ display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }}
  button {{ font: inherit; padding: 8px 16px; border-radius: 8px; border: 1px solid var(--border); background: transparent; color: var(--fg); cursor: pointer; }}
  button.primary {{ background: var(--accent); border-color: var(--accent); color: #fff; }}
</style></head>
<body><main>
  <h1>{name} wants to read your Fitbit data</h1>
  <p>The app gets read-only access to the data on your Fitdata dashboard: daily metrics, sleep, heart rate and activities.</p>
  <p class="muted">Redirects to <code>{host}</code></p>
  <p class="muted">If you did not expect this connection, choose Deny.</p>
  <form method="post" action="/oauth/authorize">
    <input type="hidden" name="consent" value="{consent}">
    <div class="buttons">
      <button type="submit" name="decision" value="deny">Deny</button>
      <button type="submit" name="decision" value="allow" class="primary">Allow</button>
    </div>
  </form>
</main></body></html>"#
    );
    (
        [
            (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Html(html),
    )
        .into_response()
}

#[derive(Deserialize)]
struct Decision {
    consent: String,
    decision: String,
}

/// The consent form's POST. The session cookie is SameSite=Lax, so another
/// site cannot submit it on the user's behalf.
async fn approve(
    State(cfg): State<Shared>,
    headers: HeaderMap,
    Form(d): Form<Decision>,
) -> Response {
    let Some(Claims::Consent {
        client_id,
        redirect_uri,
        state,
        challenge,
        uid,
    }) = verify(&cfg, &d.consent)
    else {
        return bad_request("the request expired, start again from the app");
    };
    if session_user(&cfg, &headers).await.map(|u| u.uid) != Some(uid.clone()) {
        return bad_request("signed in as a different user");
    }
    if d.decision != "allow" {
        return Redirect::to(&redirect_with(
            &redirect_uri,
            &[("error", "access_denied")],
            state.as_deref(),
        ))
        .into_response();
    }
    tracing::info!(%uid, "MCP client authorized");
    let code = sign(
        &cfg,
        Claims::Code {
            client_id,
            redirect_uri: redirect_uri.clone(),
            challenge,
            uid,
        },
        now() + CODE_SECS,
    );
    Redirect::to(&redirect_with(
        &redirect_uri,
        &[("code", &code)],
        state.as_deref(),
    ))
    .into_response()
}

// ------------------------------------------------------------------- token

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

/// Client ID and secret from the body or from HTTP Basic auth.
fn client_auth(headers: &HeaderMap, t: &TokenRequest) -> (Option<String>, Option<String>) {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Basic "))
        .and_then(|b| {
            base64::engine::general_purpose::STANDARD
                .decode(b.trim())
                .ok()
        })
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            let (id, secret) = s.split_once(':')?;
            let dec = |x: &str| {
                url::form_urlencoded::parse(format!("x={x}").as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
            };
            Some((dec(id)?, dec(secret)?))
        });
    match basic {
        Some((id, secret)) => (Some(id), Some(secret)),
        None => (t.client_id.clone(), t.client_secret.clone()),
    }
}

/// Checks the client and returns its ID.
fn authenticate_client(cfg: &Config, headers: &HeaderMap, t: &TokenRequest) -> Result<String> {
    let (id, secret) = client_auth(headers, t);
    let id = id.context("missing client_id")?;
    let client = verify_client(cfg, &id).context("unknown client")?;
    if client.auth != ClientAuth::None {
        let expected = client_secret(cfg, &id);
        let ok = secret.is_some_and(|s| {
            // Comparing hashes keeps timing from leaking the secret.
            Sha256::digest(s.as_bytes()) == Sha256::digest(expected.as_bytes())
        });
        if !ok {
            bail!("bad client secret");
        }
    }
    Ok(id)
}

fn pkce_ok(verifier: &str, challenge: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
}

fn issue_tokens(cfg: &Config, client_id: &str, user: &SignedIn) -> Response {
    let t = now();
    let access = sign(
        cfg,
        Claims::Access {
            uid: user.uid.clone(),
            aud: resource(cfg),
        },
        t + ACCESS_SECS,
    );
    let refresh = sign(
        cfg,
        Claims::Refresh {
            client_id: client_id.to_string(),
            uid: user.uid.clone(),
        },
        t + REFRESH_SECS,
    );
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": ACCESS_SECS,
            "refresh_token": refresh,
            "scope": SCOPE,
        })),
    )
        .into_response()
}

async fn token(
    State(cfg): State<Shared>,
    headers: HeaderMap,
    Form(t): Form<TokenRequest>,
) -> Response {
    let client_id = match authenticate_client(&cfg, &headers, &t) {
        Ok(id) => id,
        Err(e) => return oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", &e.to_string()),
    };
    let grant = match t.grant_type.as_str() {
        "authorization_code" => match t.code.as_deref().and_then(|c| verify(&cfg, c)) {
            Some(Claims::Code {
                client_id: for_client,
                redirect_uri,
                challenge,
                uid,
            }) => (for_client == client_id
                && t.redirect_uri.as_ref().is_none_or(|u| *u == redirect_uri)
                && pkce_ok(t.code_verifier.as_deref().unwrap_or_default(), &challenge))
            .then_some(uid),
            _ => None,
        },
        "refresh_token" => match t.refresh_token.as_deref().and_then(|r| verify(&cfg, r)) {
            Some(Claims::Refresh {
                client_id: for_client,
                uid,
            }) => (for_client == client_id).then_some(uid),
            _ => None,
        },
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "use authorization_code or refresh_token",
            );
        }
    };
    let user = match grant {
        Some(uid) => resolve(&cfg, &uid).await,
        None => None,
    };
    match user {
        Some(user) => issue_tokens(&cfg, &client_id, &user),
        None => oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "the code or refresh token is invalid or expired",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_https_or_loopback_redirects() {
        assert!(valid_redirect_uri(
            "https://claude.ai/api/mcp/auth_callback"
        ));
        assert!(valid_redirect_uri("http://localhost:33418/callback"));
        assert!(valid_redirect_uri("http://127.0.0.1/cb"));
        assert!(!valid_redirect_uri("http://evil.example/cb"));
        assert!(!valid_redirect_uri("javascript:alert(1)"));
        assert!(!valid_redirect_uri("https://a.example/cb#frag"));
        assert!(!valid_redirect_uri("not a url"));
    }

    #[test]
    fn checks_pkce() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(pkce_ok(verifier, challenge));
        assert!(!pkce_ok("x".repeat(43).as_str(), challenge));
        assert!(!pkce_ok("short", challenge));
    }

    #[test]
    fn appends_params_to_redirects() {
        assert_eq!(
            redirect_with("https://a.example/cb?x=1", &[("code", "a b")], Some("s&t")),
            "https://a.example/cb?x=1&code=a+b&state=s%26t"
        );
    }
}
