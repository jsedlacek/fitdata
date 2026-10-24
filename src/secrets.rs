//! Secrets (OAuth client, session key) live in Secret Manager
//! and are only held in memory. A local file path is accepted too, for tests.

use std::path::PathBuf;

use anyhow::{Context, Result};
use google_cloud_gax::error::rpc::Code;
use google_cloud_secretmanager_v1::client::SecretManagerService;
use tokio::sync::OnceCell;

static CLIENT: OnceCell<SecretManagerService> = OnceCell::const_new();

/// Shared client using Application Default Credentials (the service account
/// on Cloud Run, `gcloud auth application-default login` locally).
async fn client() -> Result<&'static SecretManagerService> {
    CLIENT
        .get_or_try_init(|| async {
            SecretManagerService::builder()
                .build()
                .await
                .context("Secret Manager client")
        })
        .await
}

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
    pub async fn read(&self) -> Result<Option<Vec<u8>>> {
        match self {
            Self::File(p) => match std::fs::read(p) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
            },
            Self::Manager(name) => {
                let resp = client()
                    .await?
                    .access_secret_version()
                    .set_name(format!("{name}/versions/latest"))
                    .send()
                    .await;
                match resp {
                    Ok(r) => Ok(r.payload.map(|p| p.data.to_vec())),
                    // No secret, or a secret without enabled versions yet.
                    Err(e)
                        if e.status().is_some_and(|s| {
                            matches!(s.code, Code::NotFound | Code::FailedPrecondition)
                        }) =>
                    {
                        Ok(None)
                    }
                    Err(e) => Err(e).with_context(|| format!("reading secret {name}")),
                }
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
