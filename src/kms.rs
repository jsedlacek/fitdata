//! Refresh tokens are encrypted with a Cloud KMS key before they are stored.
//! The web may only encrypt (it stores a token after "Connect Fitbit") and
//! the job may also decrypt, so a token stored by the web is readable by the
//! job alone. The user ID is bound to each ciphertext as additional
//! authenticated data: a token copied into another user's record does not
//! decrypt.

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder};
use serde::Deserialize;
use serde_json::json;

pub enum Kms {
    Cloud {
        http: reqwest::Client,
        creds: AccessTokenCredentials,
        /// `projects/{p}/locations/{l}/keyRings/{r}/cryptoKeys/{k}`
        key: String,
    },
    /// Local development only: "ciphertext" is the base64 plaintext.
    Plain,
}

#[derive(Deserialize)]
struct EncryptResponse {
    ciphertext: String,
}

#[derive(Deserialize)]
struct DecryptResponse {
    plaintext: String,
}

impl Kms {
    /// `plain` (local development) or a KMS key resource name.
    pub fn new(http: reqwest::Client, key: &str) -> Result<Self> {
        if key == "plain" {
            tracing::warn!("refresh tokens are stored unencrypted (KMS_KEY=plain)");
            return Ok(Self::Plain);
        }
        if !key.starts_with("projects/") {
            bail!("KMS_KEY must be `projects/.../cryptoKeys/...` or `plain`");
        }
        Ok(Self::Cloud {
            http,
            creds: Builder::default()
                .build_access_token_credentials()
                .context("Google credentials for Cloud KMS")?,
            key: key.to_string(),
        })
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        http: &reqwest::Client,
        creds: &AccessTokenCredentials,
        url: String,
        body: serde_json::Value,
    ) -> Result<T> {
        let token = creds.access_token().await?.token;
        let resp = http.post(url).bearer_auth(token).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("Cloud KMS: HTTP {status}: {text}");
        }
        resp.json().await.context("unexpected Cloud KMS response")
    }

    #[tracing::instrument(name = "kms encrypt", skip_all, fields(otel.kind = "client"))]
    pub async fn encrypt(&self, plaintext: &[u8], uid: &str) -> Result<String> {
        match self {
            Self::Plain => Ok(STANDARD.encode(plaintext)),
            Self::Cloud { http, creds, key } => {
                let r: EncryptResponse = Self::call(
                    http,
                    creds,
                    format!("https://cloudkms.googleapis.com/v1/{key}:encrypt"),
                    json!({
                        "plaintext": STANDARD.encode(plaintext),
                        "additionalAuthenticatedData": STANDARD.encode(uid),
                    }),
                )
                .await?;
                Ok(r.ciphertext)
            }
        }
    }

    #[tracing::instrument(name = "kms decrypt", skip_all, fields(otel.kind = "client"))]
    pub async fn decrypt(&self, ciphertext: &str, uid: &str) -> Result<Vec<u8>> {
        match self {
            Self::Plain => Ok(STANDARD.decode(ciphertext)?),
            Self::Cloud { http, creds, key } => {
                let r: DecryptResponse = Self::call(
                    http,
                    creds,
                    format!("https://cloudkms.googleapis.com/v1/{key}:decrypt"),
                    json!({
                        "ciphertext": ciphertext,
                        "additionalAuthenticatedData": STANDARD.encode(uid),
                    }),
                )
                .await?;
                Ok(STANDARD.decode(r.plaintext)?)
            }
        }
    }
}
