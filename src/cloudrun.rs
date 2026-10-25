//! Starting the sync job from the dashboard and telling whether it runs,
//! via the Cloud Run Admin API (the web service account needs
//! `run.jobs.runWithOverrides` and `run.jobs.get` on the job; deploy/deploy.sh
//! grants them).

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub struct SyncJob {
    http: reqwest::Client,
    creds: AccessTokenCredentials,
    url: String,
}

impl SyncJob {
    pub fn new(http: reqwest::Client, project: &str, region: &str, job: &str) -> Result<Self> {
        Ok(Self {
            http,
            creds: Builder::default()
                .build_access_token_credentials()
                .context("Google credentials for Cloud Run")?,
            url: format!(
                "https://run.googleapis.com/v2/projects/{project}/locations/{region}/jobs/{job}"
            ),
        })
    }

    #[tracing::instrument(name = "Cloud Run API", skip_all, fields(otel.kind = "client"))]
    async fn call<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        let token = self.creds.access_token().await?.token;
        let resp = req.bearer_auth(token).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body: ApiError = resp.json().await.unwrap_or_default();
            anyhow::bail!("Cloud Run API: HTTP {status}: {}", body.error.message);
        }
        resp.json()
            .await
            .context("unexpected Cloud Run API response")
    }

    /// When the latest execution (manual or scheduled) was created, if it has
    /// not finished yet. A new execution shows up here as soon as `run` returns.
    pub async fn running_since(&self) -> Result<Option<DateTime<Utc>>> {
        let job: Job = self.call(self.http.get(&self.url)).await?;
        Ok(job.running_since())
    }

    /// Starts an execution, of one user only if `user` is given (which
    /// needs `run.jobs.runWithOverrides`).
    pub async fn run(&self, user: Option<&str>) -> Result<()> {
        let body = match user {
            Some(uid) => serde_json::json!({ "overrides": { "containerOverrides": [{
                "args": ["job", "--days=7", format!("--user={uid}")],
            }]}}),
            None => serde_json::json!({}),
        };
        let _operation: Value = self
            .call(self.http.post(format!("{}:run", self.url)).json(&body))
            .await?;
        Ok(())
    }
}

/// The parts of a Cloud Run v2 `Job` used here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    latest_created_execution: Option<ExecutionReference>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionReference {
    create_time: Option<DateTime<Utc>>,
    completion_time: Option<String>,
}

impl Job {
    fn running_since(&self) -> Option<DateTime<Utc>> {
        let e = self.latest_created_execution.as_ref()?;
        // Without a create time, as old as can be: it still counts as running.
        e.completion_time
            .is_none()
            .then(|| e.create_time.unwrap_or(DateTime::<Utc>::MIN_UTC))
    }
}

#[derive(Debug, Default, Deserialize)]
struct ApiError {
    error: ApiErrorBody,
}

#[derive(Debug, Default, Deserialize)]
struct ApiErrorBody {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn running_since(v: Value) -> Option<DateTime<Utc>> {
        serde_json::from_value::<Job>(v).unwrap().running_since()
    }

    #[test]
    fn running_until_the_latest_execution_completes() {
        assert_eq!(running_since(json!({})), None);
        assert_eq!(
            running_since(json!({"latestCreatedExecution": {
                "name": "x", "createTime": "2026-10-24T09:00:02Z",
                "completionStatus": "EXECUTION_PENDING"}})),
            Some("2026-10-24T09:00:02Z".parse().unwrap())
        );
        assert_eq!(
            running_since(json!({"latestCreatedExecution": {
                "name": "x", "createTime": "2026-10-24T09:00:02Z",
                "completionTime": "2026-10-24T09:01:46Z"}})),
            None
        );
    }
}
