//! Secrets (OAuth client, refresh token) live in Secret Manager and are only
//! held in memory. A local file path is accepted too, e.g. for tests.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

const SM_BASE: &str = "https://secretmanager.googleapis.com/v1";
const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

#[derive(Debug, Clone)]
pub enum SecretRef {
    /// `projects/{p}/secrets/{name}`
    Manager(String),
    File(PathBuf),
}

impl SecretRef {
    /// `sm://name` (in `project`), `sm://projects/p/secrets/name`, or a file path.
    pub fn parse(s: &str, project: Option<&str>) -> Result<Self> {
        let Some(rest) = s.strip_prefix("sm://") else {
            return Ok(Self::File(PathBuf::from(s)));
        };
        if rest.starts_with("projects/") {
            return Ok(Self::Manager(rest.to_string()));
        }
        let project = project.context("GCP_PROJECT must be set to use sm:// secrets")?;
        Ok(Self::Manager(format!("projects/{project}/secrets/{rest}")))
    }

    /// The latest secret version, or `None` if there is none yet.
    pub async fn read(&self, http: &reqwest::Client) -> Result<Option<Vec<u8>>> {
        match self {
            Self::File(p) => match std::fs::read(p) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
            },
            Self::Manager(name) => {
                let resp = http
                    .get(format!("{SM_BASE}/{name}/versions/latest:access"))
                    .bearer_auth(gcp_access_token(http).await?)
                    .send()
                    .await?;
                if resp.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let status = resp.status();
                let body: Value = resp.json().await.unwrap_or_default();
                // A secret without versions answers 400/404 with this status.
                if body["error"]["status"] == "NOT_FOUND"
                    || body["error"]["status"] == "FAILED_PRECONDITION"
                {
                    return Ok(None);
                }
                if !status.is_success() {
                    bail!(
                        "reading secret {name} failed ({status}): {}",
                        body["error"]["message"]
                    );
                }
                let data = body["payload"]["data"]
                    .as_str()
                    .context("secret has no payload")?;
                Ok(Some(STANDARD.decode(data)?))
            }
        }
    }

    /// Stores `data` as the new latest version.
    pub async fn write(&self, http: &reqwest::Client, data: &[u8]) -> Result<()> {
        match self {
            Self::File(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(p, data)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
                }
                Ok(())
            }
            Self::Manager(name) => {
                let resp = http
                    .post(format!("{SM_BASE}/{name}:addVersion"))
                    .bearer_auth(gcp_access_token(http).await?)
                    .json(&json!({ "payload": { "data": STANDARD.encode(data) } }))
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    bail!(
                        "writing secret {name} failed ({status}): {}",
                        resp.text().await.unwrap_or_default()
                    );
                }
                Ok(())
            }
        }
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manager(n) => write!(f, "Secret Manager {n}"),
            Self::File(p) => write!(f, "{}", p.display()),
        }
    }
}

/// Google Cloud access token: the service account's on Cloud Run, the gcloud
/// user's when running locally.
async fn gcp_access_token(http: &reqwest::Client) -> Result<String> {
    if std::env::var_os("K_SERVICE").is_some() || std::env::var_os("CLOUD_RUN_JOB").is_some() {
        let v: Value = http
            .get(METADATA_TOKEN_URL)
            .header("Metadata-Flavor", "Google")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        return v["access_token"]
            .as_str()
            .map(str::to_owned)
            .context("metadata server returned no token");
    }
    let out = tokio::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .await
        .context("running `gcloud auth print-access-token` (is gcloud installed and logged in?)")?;
    if !out.status.success() {
        bail!(
            "gcloud auth print-access-token failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs() {
        assert!(matches!(
            SecretRef::parse("sm://oauth-client", Some("p1")).unwrap(),
            SecretRef::Manager(n) if n == "projects/p1/secrets/oauth-client"
        ));
        assert!(matches!(
            SecretRef::parse("sm://projects/p2/secrets/x", None).unwrap(),
            SecretRef::Manager(n) if n == "projects/p2/secrets/x"
        ));
        assert!(SecretRef::parse("sm://x", None).is_err());
        assert!(matches!(
            SecretRef::parse(".secrets/t.json", None).unwrap(),
            SecretRef::File(_)
        ));
    }
}
