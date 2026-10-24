//! Starting the sync job from the dashboard and telling whether it runs,
//! via the Cloud Run Admin API (the web service account needs
//! `run.jobs.run` and `run.jobs.get` on the job; deploy/deploy.sh grants them).

use anyhow::{Context, Result};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder};
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
    async fn call(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let token = self.creds.access_token().await?.token;
        let resp = req.bearer_auth(token).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Cloud Run API: HTTP {status}: {}", body["error"]["message"]);
        }
        Ok(body)
    }

    /// Whether the latest execution (manual or scheduled) has not finished yet.
    /// A new execution shows up here as soon as `run` returns.
    pub async fn running(&self) -> Result<bool> {
        let job = self.call(self.http.get(&self.url)).await?;
        Ok(is_running(&job))
    }

    pub async fn run(&self) -> Result<()> {
        self.call(
            self.http
                .post(format!("{}:run", self.url))
                .json(&serde_json::json!({})),
        )
        .await?;
        Ok(())
    }
}

fn is_running(job: &Value) -> bool {
    let latest = &job["latestCreatedExecution"];
    latest.is_object() && latest["completionTime"].is_null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn running_until_the_latest_execution_completes() {
        assert!(!is_running(&json!({})));
        assert!(is_running(&json!({"latestCreatedExecution": {
            "name": "x", "completionStatus": "EXECUTION_PENDING"}})));
        assert!(!is_running(&json!({"latestCreatedExecution": {
            "name": "x", "completionTime": "2026-10-24T09:01:46Z"}})));
    }
}
