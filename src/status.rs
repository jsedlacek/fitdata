//! The sync state of a user, `u/{uid}/status.json`: the last run, the last
//! successful one, and whether a run is in progress. The job writes it; the
//! dashboard reads it to show "synced 5 min ago" or offer reconnecting.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::store::Store;
use crate::tables::user_prefix;

/// A run that has not finished after this long died without saying so.
const RUN_TIMEOUT: Duration = Duration::minutes(40);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub last_run: Option<Run>,
    pub last_ok: Option<DateTime<Utc>>,
    pub running_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub at: DateTime<Utc>,
    pub ok: bool,
    /// The refresh token no longer works: the user has to reconnect.
    pub auth_error: bool,
    pub error: Option<String>,
    /// Data types skipped as not readable.
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

async fn put(store: &Store, uid: &str, s: &Status) -> Result<()> {
    store.put(&path(uid), serde_json::to_vec_pretty(s)?).await
}

pub async fn started(store: &Store, uid: &str, at: DateTime<Utc>) -> Result<()> {
    let mut s = get(store, uid).await?;
    s.running_since = Some(at);
    put(store, uid, &s).await
}

pub async fn finished(store: &Store, uid: &str, run: Run) -> Result<()> {
    let mut s = get(store, uid).await?;
    s.running_since = None;
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
        finished(&store, "1", run(true)).await.unwrap();
        finished(&store, "1", run(false)).await.unwrap();
        let s = get(&store, "1").await.unwrap();
        assert!(!s.running(t0));
        assert_eq!(s.last_ok, Some(t0));
        assert!(s.last_run.unwrap().auth_error);
    }
}
