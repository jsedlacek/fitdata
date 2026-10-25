//! Refresh tokens are encrypted with a Cloud KMS key before they are stored.
//! The web may only encrypt (it stores a token after "Connect Fitbit") and
//! the job may also decrypt, so a token stored by the web is readable by the
//! job alone. The user ID is bound to each ciphertext as additional
//! authenticated data: a token copied into another user's record does not
//! decrypt.

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use google_cloud_gax::options::RequestOptionsBuilder;
use google_cloud_kms_v1::client::KeyManagementService;
use tokio::sync::OnceCell;

static CLIENT: OnceCell<KeyManagementService> = OnceCell::const_new();

/// Shared client using Application Default Credentials (the service account
/// on Cloud Run, `gcloud auth application-default login` locally).
///
/// Encrypt and decrypt are POSTs, which the client retries only when marked
/// idempotent; both change nothing, so a transient error is retried.
async fn client() -> Result<&'static KeyManagementService> {
    CLIENT
        .get_or_try_init(|| async {
            KeyManagementService::builder()
                .build()
                .await
                .context("Cloud KMS client")
        })
        .await
}

pub enum Kms {
    /// `projects/{p}/locations/{l}/keyRings/{r}/cryptoKeys/{k}`
    Cloud(String),
    /// Local development only: "ciphertext" is the base64 plaintext.
    Plain,
}

impl Kms {
    /// `plain` (local development) or a KMS key resource name.
    pub fn new(key: &str) -> Result<Self> {
        if key == "plain" {
            tracing::warn!("refresh tokens are stored unencrypted (KMS_KEY=plain)");
            return Ok(Self::Plain);
        }
        if !key.starts_with("projects/") {
            bail!("KMS_KEY must be `projects/.../cryptoKeys/...` or `plain`");
        }
        Ok(Self::Cloud(key.to_string()))
    }

    /// The ciphertext as base64, as it is stored in the user's record.
    #[tracing::instrument(name = "kms encrypt", skip_all, fields(otel.kind = "client"))]
    pub async fn encrypt(&self, plaintext: &[u8], uid: &str) -> Result<String> {
        match self {
            Self::Plain => Ok(STANDARD.encode(plaintext)),
            Self::Cloud(key) => {
                let resp = client()
                    .await?
                    .encrypt()
                    .set_name(key)
                    .set_plaintext(plaintext.to_vec())
                    .set_additional_authenticated_data(uid.as_bytes().to_vec())
                    .with_idempotency(true)
                    .send()
                    .await
                    .context("Cloud KMS encrypt")?;
                Ok(STANDARD.encode(resp.ciphertext))
            }
        }
    }

    #[tracing::instrument(name = "kms decrypt", skip_all, fields(otel.kind = "client"))]
    pub async fn decrypt(&self, ciphertext: &str, uid: &str) -> Result<Vec<u8>> {
        let ciphertext = STANDARD.decode(ciphertext)?;
        match self {
            Self::Plain => Ok(ciphertext),
            Self::Cloud(key) => {
                let resp = client()
                    .await?
                    .decrypt()
                    .set_name(key)
                    .set_ciphertext(ciphertext)
                    .set_additional_authenticated_data(uid.as_bytes().to_vec())
                    .with_idempotency(true)
                    .send()
                    .await
                    .context("Cloud KMS decrypt")?;
                Ok(resp.plaintext.to_vec())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys() {
        assert!(matches!(Kms::new("plain").unwrap(), Kms::Plain));
        assert!(matches!(
            Kms::new("projects/p/locations/l/keyRings/r/cryptoKeys/k").unwrap(),
            Kms::Cloud(k) if k == "projects/p/locations/l/keyRings/r/cryptoKeys/k"
        ));
        assert!(Kms::new("fitdata/refresh-token").is_err());
    }

    #[tokio::test]
    async fn plain_round_trips() {
        let kms = Kms::Plain;
        let c = kms.encrypt(b"token", "123").await.unwrap();
        assert_eq!(kms.decrypt(&c, "123").await.unwrap(), b"token");
    }
}
