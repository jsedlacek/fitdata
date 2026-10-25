//! Google OAuth 2.0: one-off interactive login (authorization code + PKCE over a
//! loopback redirect) and refresh-token based access tokens for the job.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

use crate::datatypes::READ_SCOPES;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Clone)]
pub struct ClientCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl ClientCredentials {
    /// Parses the client JSON downloaded from Cloud Console
    /// (`{"web": {...}}` for web clients, `{"installed": {...}}` for desktop ones).
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        let v: serde_json::Value =
            serde_json::from_slice(raw).context("OAuth client is not JSON")?;
        let c = v.get("web").or_else(|| v.get("installed")).unwrap_or(&v);
        let field = |k: &str| {
            c[k].as_str()
                .map(str::to_owned)
                .with_context(|| format!("`{k}` missing in OAuth client JSON"))
        };
        Ok(Self {
            client_id: field("client_id")?,
            client_secret: field("client_secret")?,
        })
    }
}

/// The stored refresh token no longer works and a new consent is needed.
#[derive(Debug)]
pub struct AuthError(pub String);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AuthError {}

#[derive(Serialize, Deserialize)]
pub struct StoredToken {
    pub refresh_token: String,
    pub scope: String,
    pub obtained_at: chrono::DateTime<chrono::Utc>,
}

impl StoredToken {
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        serde_json::from_slice(raw).context("stored refresh token is not valid JSON")
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

pub fn random_b64(bytes: usize) -> String {
    let buf: Vec<u8> = (0..bytes).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(buf)
}

/// A pending authorization: the URL to send the browser to, plus the secrets
/// needed to verify and redeem the callback.
pub struct AuthRequest {
    pub url: Url,
    pub state: String,
    pub verifier: String,
}

/// Authorization-code + PKCE request for offline access. Google issues a
/// refresh token only on the user's first consent, unless `renew` forces the
/// consent screen, which always yields a new one. Without `renew` a returning
/// user who granted every scope goes straight through.
pub fn auth_request(
    creds: &ClientCredentials,
    redirect_uri: &str,
    scopes: &[&str],
    renew: bool,
) -> Result<AuthRequest> {
    let verifier = random_b64(48);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_b64(16);
    let mut url = Url::parse(AUTH_URL)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &creds.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &scopes.join(" "))
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline");
        if renew {
            q.append_pair("prompt", "consent");
        }
    }
    Ok(AuthRequest {
        url,
        state,
        verifier,
    })
}

pub async fn exchange_code(
    http: &reqwest::Client,
    creds: &ClientCredentials,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    let resp = http
        .post(TOKEN_URL)
        .form(&[
            ("code", code),
            ("client_id", &creds.client_id),
            ("client_secret", &creds.client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        bail!(
            "token exchange failed ({status}): {}",
            resp.text().await.unwrap_or_default()
        );
    }
    Ok(resp.json().await?)
}

impl TokenResponse {
    /// The long-lived token to store after a Health API consent.
    pub fn into_stored(self) -> Result<StoredToken> {
        Ok(StoredToken {
            refresh_token: self
                .refresh_token
                .context("Google returned no refresh token; revoke the app's access and retry")?,
            scope: self.scope.unwrap_or_default(),
            obtained_at: chrono::Utc::now(),
        })
    }
}

/// Runs the interactive consent flow and returns the new refresh token.
///
/// Google redirects the browser to `http://localhost:{port}/callback`. If that
/// reaches this process (port forwarded to the browser's machine) the code is
/// picked up automatically; otherwise the user pastes the redirected URL from
/// the browser's address bar into the terminal.
pub async fn login(
    http: &reqwest::Client,
    creds: &ClientCredentials,
    port: u16,
) -> Result<StoredToken> {
    let redirect_uri = format!("http://localhost:{port}/callback");
    let req = auth_request(creds, &redirect_uri, READ_SCOPES, true)?;

    println!(
        "Open this URL in your browser and grant access:\n\n{}\n",
        req.url
    );
    println!(
        "Waiting for the redirect on port {port}. If the browser shows an error page \
         after consent, copy its full address (http://localhost:{port}/callback?...) and paste it here:"
    );

    let callback = wait_for_callback(port).await?;
    let params: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    if let Some(err) = params.get("error") {
        bail!("authorization failed: {err}");
    }
    if params.get("state") != Some(&req.state) {
        bail!("state mismatch in callback; start the login again");
    }
    let code = params
        .get("code")
        .context("callback has no `code` parameter")?;
    let token = exchange_code(http, creds, code, &redirect_uri, &req.verifier)
        .await?
        .into_stored()?;
    println!("\nGranted scopes: {}", token.scope);
    Ok(token)
}

/// Resolves with the first callback URL received over HTTP or pasted on stdin.
async fn wait_for_callback(port: u16) -> Result<Url> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("cannot listen on port {port}"))?;
    let base = format!("http://localhost:{port}");

    let http = async {
        loop {
            let (mut sock, _) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await?;
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            if !path.starts_with("/callback") {
                sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                    .await?;
                continue;
            }
            let body = "Login complete. You can close this tab and return to the terminal.";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await?;
            return Ok::<Url, anyhow::Error>(Url::parse(&format!("{base}{path}"))?);
        }
    };

    let stdin = async {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match Url::parse(line) {
                Ok(u) if u.query_pairs().any(|(k, _)| k == "code" || k == "error") => return Ok(u),
                _ => eprintln!("That does not look like the callback URL; paste the full address."),
            }
        }
        bail!("stdin closed before a callback URL was pasted")
    };

    tokio::select! {
        r = http => r,
        r = stdin => r,
    }
}

/// Revokes a refresh token (and the access it grants) at Google.
pub async fn revoke(http: &reqwest::Client, refresh_token: &str) -> Result<()> {
    let resp = http
        .post("https://oauth2.googleapis.com/revoke")
        .form(&[("token", refresh_token)])
        .send()
        .await?;
    // 400 invalid_token: already revoked or expired, which is what we want.
    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::BAD_REQUEST {
        bail!("revoking the token failed: HTTP {}", resp.status());
    }
    Ok(())
}

/// Hands out access tokens, refreshing them from the long-lived refresh token.
pub struct TokenSource {
    http: reqwest::Client,
    creds: ClientCredentials,
    refresh_token: String,
    cached: Mutex<Option<(String, Instant)>>,
}

impl TokenSource {
    pub fn new(http: reqwest::Client, creds: ClientCredentials, refresh_token: String) -> Self {
        Self {
            http,
            creds,
            refresh_token,
            cached: Mutex::new(None),
        }
    }

    pub async fn access_token(&self) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((tok, exp)) = cached.as_ref()
            && Instant::now() < *exp
        {
            return Ok(tok.clone());
        }
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.creds.client_id.as_str()),
                ("client_secret", &self.creds.client_secret),
                ("refresh_token", &self.refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // 400/401 (e.g. invalid_grant): the refresh token was revoked or
            // expired; only a new consent ("Connect Fitbit") fixes that.
            if matches!(status.as_u16(), 400 | 401) {
                return Err(AuthError(format!(
                    "refreshing access token failed ({status}): {body}"
                ))
                .into());
            }
            bail!("refreshing access token failed ({status}): {body}");
        }
        let resp: TokenResponse = resp.json().await?;
        let exp = Instant::now() + Duration::from_secs(resp.expires_in.saturating_sub(60));
        *cached = Some((resp.access_token.clone(), exp));
        Ok(resp.access_token)
    }

    pub async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(renew: bool) -> std::collections::HashMap<String, String> {
        let creds = ClientCredentials {
            client_id: "id".into(),
            client_secret: "secret".into(),
        };
        let req = auth_request(&creds, "https://x/cb", READ_SCOPES, renew).unwrap();
        req.url.query_pairs().into_owned().collect()
    }

    #[test]
    fn sign_in_asks_for_consent_only_when_renewing() {
        let p = params(false);
        assert_eq!(p["access_type"], "offline");
        assert!(!p.contains_key("prompt"));
        let p = params(true);
        assert_eq!(p["access_type"], "offline");
        assert_eq!(p["prompt"], "consent");
    }
}
