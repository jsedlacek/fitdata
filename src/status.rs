//! The sync state of a user, `u/{uid}/status.json`: the last run, the last
//! successful one, whether a run is in progress, and what the job learned
//! to plan the next runs (`job.rs`). The job writes it; the dashboard reads
//! it to show "synced 5 min ago" or offer reconnecting.

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::store::{self, Store};
use crate::tables::user_prefix;

/// A run that has not finished after this long died without saying so.
const RUN_TIMEOUT: Duration = Duration::minutes(40);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub last_run: Option<Run>,
    pub last_ok: Option<DateTime<Utc>>,
    pub running_since: Option<DateTime<Utc>>,
    /// The last run that refreshed every day of the window and probed every
    /// data type.
    #[serde(default)]
    pub last_full: Option<DateTime<Utc>>,
    /// Data types with no data at all at the last full run; runs in between
    /// skip them.
    #[serde(default)]
    pub absent: Vec<String>,
    /// The day the account was created; there is nothing to fetch before it.
    #[serde(default)]
    pub since: Option<NaiveDate>,
    /// How far the running first sync has got.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

/// The steps of a user's first sync, which downloads their whole history and
/// takes minutes; the dashboard shows them meanwhile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Progress {
    /// Finding the data types with data, and fetching the last days.
    Recent,
    /// The whole history, `done` of `total` data types.
    History { done: usize, total: usize },
    /// Building the tables from the downloaded data.
    Tables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub at: DateTime<Utc>,
    pub ok: bool,
    /// The refresh token no longer works: the user has to reconnect.
    pub auth_error: bool,
    pub error: Option<String>,
    /// Data types skipped as not readable or failing on Google's side.
    #[serde(default)]
    pub skipped: Vec<String>,
}

impl Status {
    pub fn running(&self, now: DateTime<Utc>) -> bool {
        self.running_since.is_some_and(|t| now - t < RUN_TIMEOUT)
    }
}

pub fn path(uid: &str) -> String {
    format!("{}/status.json", user_prefix(uid))
}

pub async fn get(store: &Store, uid: &str) -> Result<Status> {
    Ok(match store.get(&path(uid)).await? {
        Some(b) => serde_json::from_slice(&b).unwrap_or_default(),
        None => Status::default(),
    })
}

/// Writes the status, waiting and trying again while GCS refuses it for
/// changing too often: a lost "finished" would fail (and retry) a whole run.
async fn put(store: &Store, uid: &str, s: &Status) -> Result<()> {
    let body = serde_json::to_vec_pretty(s)?;
    let mut wait = std::time::Duration::from_secs(1);
    for _ in 0..4 {
        match store.put(&path(uid), body.clone()).await {
            Err(e) if store::is_rate_limited(&e) => {
                tracing::warn!("status.json written too often, trying again in {wait:?}");
                tokio::time::sleep(wait).await;
                wait *= 2;
            }
            r => return r,
        }
    }
    store.put(&path(uid), body).await
}

pub async fn started(store: &Store, uid: &str, at: DateTime<Utc>) -> Result<()> {
    let mut s = get(store, uid).await?;
    s.running_since = Some(at);
    s.progress = None;
    put(store, uid, &s).await
}

/// Records how far a running sync has got.
pub async fn progress(store: &Store, uid: &str, p: Progress) -> Result<()> {
    let mut s = get(store, uid).await?;
    s.progress = Some(p);
    put(store, uid, &s).await
}

/// Records a finished run; `update` adds what the run learned.
pub async fn finished(
    store: &Store,
    uid: &str,
    run: Run,
    update: impl FnOnce(&mut Status),
) -> Result<()> {
    let mut s = get(store, uid).await?;
    s.running_since = None;
    s.progress = None;
    update(&mut s);
    if run.ok {
        s.last_ok = Some(run.at);
    }
    s.last_run = Some(run);
    put(store, uid, &s).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let t0 = Utc::now();
        assert!(get(&store, "1").await.unwrap().last_run.is_none());
        started(&store, "1", t0).await.unwrap();
        progress(&store, "1", Progress::History { done: 1, total: 3 })
            .await
            .unwrap();
        assert_eq!(
            get(&store, "1").await.unwrap().progress,
            Some(Progress::History { done: 1, total: 3 })
        );
        assert!(
            get(&store, "1")
                .await
                .unwrap()
                .running(t0 + Duration::minutes(1))
        );
        assert!(
            !get(&store, "1")
                .await
                .unwrap()
                .running(t0 + Duration::hours(1))
        );
        let run = |ok| Run {
            at: t0,
            ok,
            auth_error: !ok,
            error: (!ok).then(|| "revoked".into()),
            skipped: vec![],
        };
        finished(&store, "1", run(true), |_| {}).await.unwrap();
        finished(&store, "1", run(false), |_| {}).await.unwrap();
        let s = get(&store, "1").await.unwrap();
        assert!(!s.running(t0));
        assert_eq!(s.last_ok, Some(t0));
        assert!(s.progress.is_none());
        assert!(s.last_run.unwrap().auth_error);
    }
}
