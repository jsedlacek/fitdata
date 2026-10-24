//! Thin client for the Google Health API v4 (https://health.googleapis.com/).

use std::num::NonZeroU32;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use governor::{DefaultDirectRateLimiter, Jitter, Quota, RateLimiter};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::datatypes::DataType;
use crate::health::CivilDate;
use crate::oauth::TokenSource;

const BASE: &str = "https://health.googleapis.com/v4";
const MAX_ATTEMPTS: u32 = 8;
/// The API allows 300 requests per minute per user, counted over a rolling
/// minute. A burst of `BURST` goes out at once, then `PER_MINUTE` spread
/// evenly: at most BURST + PER_MINUTE = 280 in any minute.
const PER_MINUTE: u32 = 240;
const BURST: u32 = 40;

/// The per-user limit as a rate limiter (GCRA). It only sees this process,
/// so the job syncs a user in one process at a time (`job::Lock`).
fn limiter() -> DefaultDirectRateLimiter {
    let n = |v| NonZeroU32::new(v).expect("non-zero");
    RateLimiter::direct(Quota::per_minute(n(PER_MINUTE)).allow_burst(n(BURST)))
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub body: String,
}

impl std::error::Error for ApiError {}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Google errors are JSON; surface just the message when possible.
        let msg = serde_json::from_str::<Value>(&self.body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| self.body.chars().take(300).collect());
        write!(f, "HTTP {}: {msg}", self.status)
    }
}

pub struct HealthClient {
    http: reqwest::Client,
    tokens: TokenSource,
    limiter: DefaultDirectRateLimiter,
}

impl HealthClient {
    pub fn new(http: reqwest::Client, tokens: TokenSource) -> Self {
        Self {
            http,
            tokens,
            limiter: limiter(),
        }
    }

    /// GET with bearer auth, retrying throttling and transient server errors.
    #[tracing::instrument(skip_all, fields(
        otel.name = format!("GET {path}"),
        otel.kind = "client",
        http.response.status_code = tracing::field::Empty,
        attempts = tracing::field::Empty,
    ))]
    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let url = format!("{BASE}/{path}");
        let mut attempt = 0;
        loop {
            attempt += 1;
            let token = self.tokens.access_token().await?;
            // Requests wait here in turn until the quota has room.
            self.limiter
                .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(50)))
                .await;
            let resp = self
                .http
                .get(&url)
                .bearer_auth(token)
                .query(query)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) if attempt < MAX_ATTEMPTS => {
                    tracing::warn!("request error ({e}), retrying");
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let status = resp.status().as_u16();
            let span = tracing::Span::current();
            span.record("http.response.status_code", status);
            span.record("attempts", attempt);
            if (200..300).contains(&status) {
                return resp
                    .json()
                    .await
                    .with_context(|| format!("unexpected response from {path}"));
            }
            let body = resp.text().await.unwrap_or_default();
            match status {
                401 if attempt < MAX_ATTEMPTS => self.tokens.invalidate().await,
                // The minute is rolling, so room comes back within seconds;
                // a 429 means something else used the user's quota too.
                429 if attempt < MAX_ATTEMPTS => {
                    tracing::warn!("HTTP 429 on {path}, retrying");
                    let pause = 2_000 + rand::random::<u64>() % 3_000;
                    tokio::time::sleep(Duration::from_millis(pause)).await;
                }
                500 | 502 | 503 | 504 if attempt < MAX_ATTEMPTS => {
                    tracing::warn!("HTTP {status} on {path}, retrying");
                    tokio::time::sleep(backoff(attempt)).await;
                }
                _ => return Err(ApiError { status, body }.into()),
            }
        }
    }

    pub async fn identity(&self) -> Result<Value> {
        self.get("users/me/identity", &[]).await
    }

    pub async fn profile(&self) -> Result<Value> {
        self.get("users/me/profile", &[]).await
    }

    /// The day the Fitbit / Google Health account was created.
    pub async fn membership_start(&self) -> Result<Option<NaiveDate>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Profile {
            membership_start_date: Option<CivilDate>,
        }
        let p: Profile = self.get("users/me/profile", &[]).await?;
        Ok(p.membership_start_date.map(|d| d.0))
    }

    pub async fn paired_devices(&self) -> Result<Value> {
        self.get("users/me/pairedDevices", &[]).await
    }

    /// One page of data points, newest first.
    async fn list_page(
        &self,
        dt: &DataType,
        filter: Option<&str>,
        page_size: u32,
        page_token: Option<&str>,
    ) -> Result<(Vec<Value>, Option<String>)> {
        let mut q = vec![("pageSize", page_size.to_string())];
        if let Some(f) = filter {
            q.push(("filter", f.to_string()));
        }
        if let Some(t) = page_token {
            q.push(("pageToken", t.to_string()));
        }
        /// Points stay JSON: they are stored exactly as the API returned them.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            data_points: Vec<Value>,
            #[serde(default)]
            next_page_token: String,
        }
        let page: Page = self
            .get(
                &format!(
                    "users/me/dataTypes/{}/dataPoints{}",
                    dt.id,
                    if dt.reconcile { ":reconcile" } else { "" }
                ),
                &q,
            )
            .await?;
        let next = Some(page.next_page_token).filter(|t| !t.is_empty());
        Ok((page.data_points, next))
    }

    /// All data points matching `filter`, following pagination.
    pub async fn list_all(&self, dt: &DataType, filter: Option<&str>) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let (points, next) = self
                .list_page(dt, filter, dt.page_size(), token.as_deref())
                .await?;
            out.extend(points);
            match next {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }

    /// The most recent data point of a type, if any (results are newest first).
    pub async fn newest(&self, dt: &DataType, filter: Option<&str>) -> Result<Option<Value>> {
        let (points, _) = self.list_page(dt, filter, 1, None).await?;
        Ok(points.into_iter().next())
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.min(6)))
}
