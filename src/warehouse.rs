//! Reading a user's Parquet tables (`tables.rs`) in the web service.
//!
//! Files are cached on local disk (on Cloud Run: memory) per user. Before a
//! query the user's manifest says which files changed since they were
//! cached; only those are downloaded, and only the ones the query needs
//! (the small tables always, monthly ones for the months of a date range).
//! Queries see each table under its name, over its files (`engine.rs`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use chrono_tz::Tz;
use datafusion::prelude::SessionContext;
use futures::{StreamExt, TryStreamExt, stream};
use tokio::sync::Mutex;

use crate::engine;
use crate::store::Store;
use crate::tables::{self, MONTHLY, Manifest, file_name, months, user_prefix};

/// How long a fetched manifest is trusted; the dashboard asks several
/// endpoints at once, and a sync takes minutes anyway.
const MANIFEST_TTL: Duration = Duration::from_secs(5);
/// Query state of the dashboard's own queries (minutes of a day at most).
pub const SESSION_MEMORY: usize = 128 << 20;
/// Above this, the users used longest ago are dropped from the cache.
const CACHE_BYTES: u64 = 256 << 20;

/// Which months of the monthly tables a query needs.
#[derive(Debug, Clone, Copy)]
pub enum Need {
    /// `[from, to]`, local dates.
    Range(NaiveDate, NaiveDate),
    /// None: the query reads only the small tables.
    None,
}

pub struct Warehouse {
    store: Arc<Store>,
    dir: PathBuf,
    users: Mutex<HashMap<String, Arc<Mutex<UserCache>>>>,
}

#[derive(Default)]
struct UserCache {
    manifest: Manifest,
    checked: Option<Instant>,
    /// Cached file name -> generation.
    local: BTreeMap<String, i64>,
    used: Option<Instant>,
}

/// The files of one user a query can read. Every table is registered,
/// empty if the user has no file for it (yet).
#[derive(Clone)]
pub struct Snapshot {
    files: BTreeMap<&'static str, Vec<PathBuf>>,
    /// The user's time zone (`Manifest::time_zone`).
    pub tz: Tz,
}

impl Snapshot {
    #[cfg(test)]
    pub fn from_files<const N: usize>(files: [(&'static str, Vec<PathBuf>); N]) -> Self {
        Self {
            files: files.into_iter().collect(),
            tz: chrono_tz::Europe::Prague,
        }
    }

    /// A session that knows these tables and nothing else, with at most
    /// `memory` bytes for query state. Times are in the user's time zone.
    pub async fn session(&self, memory: usize) -> Result<SessionContext> {
        let ctx = engine::session(memory)?;
        for table in engine::tables() {
            let files = self.files.get(table).map(Vec::as_slice).unwrap_or_default();
            engine::register_local(&ctx, table, files, self.tz.name()).await?;
        }
        Ok(ctx)
    }
}

/// The files of `manifest` a query with `need` reads.
fn needed(manifest: &Manifest, need: Need) -> Vec<String> {
    let months = match need {
        Need::Range(from, to) => months(from, to),
        Need::None => Vec::new(),
    };
    manifest
        .files
        .keys()
        .filter(|name| {
            let table = name.split(['/', '.']).next().unwrap_or_default();
            if !MONTHLY.contains(&table) {
                return true;
            }
            months.iter().any(|m| **name == file_name(table, Some(m)))
        })
        .cloned()
        .collect()
}

impl Warehouse {
    pub fn new(store: Arc<Store>, dir: PathBuf) -> Self {
        Self {
            store,
            dir,
            users: Mutex::new(HashMap::new()),
        }
    }

    fn user_dir(&self, uid: &str) -> PathBuf {
        self.dir.join(uid)
    }

    /// Makes sure the files a query with `need` reads are cached and current.
    #[tracing::instrument(name = "warehouse snapshot", skip(self))]
    pub async fn snapshot(&self, uid: &str, need: Need) -> Result<Snapshot> {
        let user = self
            .users
            .lock()
            .await
            .entry(uid.to_string())
            .or_default()
            .clone();
        // One request per user at a time downloads; the others wait for it.
        let mut cache = user.lock().await;
        if cache.checked.is_none_or(|t| t.elapsed() > MANIFEST_TTL) {
            cache.manifest = tables::read_manifest(&self.store, uid)
                .await?
                .unwrap_or_default();
            cache.checked = Some(Instant::now());
        }
        let dir = self.user_dir(uid);
        let wanted = needed(&cache.manifest, need);
        let stale: Vec<(String, i64)> = wanted
            .iter()
            .filter_map(|name| {
                let generation = cache.manifest.files[name];
                (cache.local.get(name) != Some(&generation)).then(|| (name.clone(), generation))
            })
            .collect();
        let prefix = format!("{}/data", user_prefix(uid));
        let fetched: Vec<(String, i64)> = stream::iter(stale)
            .map(|(name, generation)| {
                let (store, dir, prefix) = (&self.store, &dir, &prefix);
                async move {
                    let bytes = store
                        .get(&format!("{prefix}/{name}"))
                        .await?
                        .with_context(|| format!("{name} is in the manifest but missing"))?;
                    write_atomically(&dir.join(&name), &bytes)?;
                    Ok::<_, anyhow::Error>((name, generation))
                }
            })
            .buffer_unordered(8)
            .try_collect()
            .await?;
        cache.local.extend(fetched);
        // Files gone from the manifest (a month without data any more).
        let gone: Vec<String> = cache
            .local
            .keys()
            .filter(|n| !cache.manifest.files.contains_key(*n))
            .cloned()
            .collect();
        for name in gone {
            let _ = std::fs::remove_file(dir.join(&name));
            cache.local.remove(&name);
        }
        cache.used = Some(Instant::now());

        let mut files: BTreeMap<&'static str, Vec<PathBuf>> = BTreeMap::new();
        for name in &wanted {
            let table = name.split(['/', '.']).next().unwrap_or_default();
            if let Some(t) = engine::tables().find(|t| *t == table) {
                files.entry(t).or_default().push(dir.join(name));
            }
        }
        let tz = cache.manifest.tz();
        drop(cache);
        self.evict(uid).await;
        Ok(Snapshot { files, tz })
    }

    /// Drops the least recently used users' files while the cache is too big.
    async fn evict(&self, keep: &str) {
        let Ok(mut total) = dir_size(&self.dir) else {
            return;
        };
        if total <= CACHE_BYTES {
            return;
        }
        let users = self.users.lock().await;
        let mut by_use: Vec<(Option<Instant>, String)> = Vec::new();
        for (uid, c) in users.iter() {
            if uid != keep
                && let Ok(c) = c.try_lock()
            {
                by_use.push((c.used, uid.clone()));
            }
        }
        by_use.sort();
        for (_, uid) in by_use {
            if total <= CACHE_BYTES {
                break;
            }
            let dir = self.user_dir(&uid);
            let size = dir_size(&dir).unwrap_or(0);
            if let Some(c) = users.get(&uid)
                && let Ok(mut c) = c.try_lock()
            {
                let _ = std::fs::remove_dir_all(&dir);
                c.local.clear();
                total = total.saturating_sub(size);
            }
        }
    }

    /// A session over the user's tables (see `snapshot`) for our own queries.
    pub async fn session(&self, uid: &str, need: Need) -> Result<SessionContext> {
        self.snapshot(uid, need)
            .await?
            .session(SESSION_MEMORY)
            .await
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{:08x}", rand::random::<u32>()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn dir_size(dir: &Path) -> Result<u64> {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    for e in entries {
        let e = e?;
        let meta = e.metadata()?;
        total += if meta.is_dir() {
            dir_size(&e.path())?
        } else {
            meta.len()
        };
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HeartRateRow;
    use crate::model::prague;
    use crate::tables;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn needs_small_tables_and_months_of_the_range() {
        let manifest = Manifest {
            files: [
                "daily.parquet",
                "heart_rate/2026-08.parquet",
                "heart_rate/2026-09.parquet",
                "heart_rate/2026-10.parquet",
                "spo2/2026-10.parquet",
            ]
            .into_iter()
            .map(|n| (n.to_string(), 1))
            .collect(),
            ..Default::default()
        };
        assert_eq!(
            needed(&manifest, Need::Range(d("2026-09-30"), d("2026-10-01"))),
            [
                "daily.parquet",
                "heart_rate/2026-09.parquet",
                "heart_rate/2026-10.parquet",
                "spo2/2026-10.parquet"
            ]
        );
        assert_eq!(needed(&manifest, Need::None), ["daily.parquet"]);
    }

    #[tokio::test]
    async fn queries_see_local_time_with_offsets() {
        use crate::health::StageType;
        use crate::model::{NightRow, SleepStage, Time};
        let data = tempfile::tempdir().unwrap();
        let store = Store::open(data.path().to_str().unwrap()).await.unwrap();
        let at = |s: &str| Time::parse_from_rfc3339(s).unwrap();
        // The hour repeated when clocks go back, twice.
        let hr: Vec<HeartRateRow> = ["2026-10-25T02:30:00+02:00", "2026-10-25T02:30:00+01:00"]
            .into_iter()
            .map(|t| HeartRateRow {
                date: at(t).date_naive(),
                ts: at(t),
                bpm: 60.0,
            })
            .collect();
        let day = hr[0].date;
        tables::replace_range(&store, "x", &hr, day, day)
            .await
            .unwrap();
        let night = NightRow {
            date: day,
            start: at("2026-10-25T00:00:00+02:00"),
            end: at("2026-10-25T07:00:00+01:00"),
            minutes_asleep: Some(480),
            minutes_awake: None,
            deep_minutes: None,
            light_minutes: None,
            rem_minutes: None,
            awake_minutes: None,
            restless_minutes: Some(0),
            stages: vec![SleepStage {
                start: at("2026-10-25T02:50:00+02:00"),
                end: at("2026-10-25T02:10:00+01:00"),
                stage: StageType::Deep,
            }],
            restless: vec![],
        };
        tables::replace_range(&store, "x", std::slice::from_ref(&night), day, day)
            .await
            .unwrap();
        let data = data.path().join("u/x/data");
        let ctx = Snapshot::from_files([
            ("heart_rate", vec![data.join("heart_rate/2026-10.parquet")]),
            ("nights", vec![data.join("nights.parquet")]),
        ])
        .session(64 << 20)
        .await
        .unwrap();

        let r = engine::rows(
            &ctx,
            "SELECT date_trunc('hour', ts) AS h, extract(hour FROM ts) AS n, count(*) AS c
             FROM heart_rate GROUP BY h, n ORDER BY h",
        )
        .await
        .unwrap();
        assert_eq!(
            r,
            [
                serde_json::json!({"h": "2026-10-25T02:00:00+02:00", "n": 2, "c": 1}),
                serde_json::json!({"h": "2026-10-25T02:00:00+01:00", "n": 2, "c": 1}),
            ]
        );
        let r = engine::rows(
            &ctx,
            "SELECT count(*) AS c FROM heart_rate WHERE ts = '2026-10-25 02:30:00+01:00'",
        )
        .await
        .unwrap();
        assert_eq!(r[0]["c"], 1);

        let nights: Vec<NightRow> = engine::query_as(&ctx, "SELECT * FROM nights")
            .await
            .unwrap();
        assert_eq!(nights[0].stages, night.stages);
        assert_eq!(nights[0].end.to_rfc3339(), "2026-10-25T07:00:00+01:00");
    }

    #[tokio::test]
    async fn follows_the_manifest() {
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(data.path().to_str().unwrap()).await.unwrap());
        let wh = Warehouse::new(store.clone(), cache.path().to_path_buf());
        let hr = |ts: &str, bpm| {
            let ts = prague(ts);
            HeartRateRow {
                date: ts.date_naive(),
                ts,
                bpm,
            }
        };
        let count = |need| {
            let wh = &wh;
            async move {
                let ctx = wh.session("x", need).await.unwrap();
                let r = engine::rows(&ctx, "SELECT count(*) AS n FROM heart_rate")
                    .await
                    .unwrap();
                r[0]["n"].as_i64().unwrap()
            }
        };
        let oct = Need::Range(d("2026-10-01"), d("2026-10-31"));

        // No data yet: empty tables.
        assert_eq!(count(oct).await, 0);

        tables::replace_range(
            &store,
            "x",
            &[hr("2026-10-01 10:00", 60.0)],
            d("2026-10-01"),
            d("2026-10-01"),
        )
        .await
        .unwrap();
        tables::write_manifest(&store, "x", chrono_tz::Europe::Prague)
            .await
            .unwrap();
        // The manifest is trusted for a few seconds.
        assert_eq!(count(oct).await, 0);
        wh.users.lock().await["x"].lock().await.checked = None;
        assert_eq!(count(oct).await, 1);
        assert_eq!(
            count(Need::Range(d("2026-09-01"), d("2026-09-30"))).await,
            0
        );

        tables::replace_range(
            &store,
            "x",
            &[hr("2026-10-01 10:00", 60.0), hr("2026-10-01 11:00", 61.0)],
            d("2026-10-01"),
            d("2026-10-01"),
        )
        .await
        .unwrap();
        tables::write_manifest(&store, "x", chrono_tz::Europe::Prague)
            .await
            .unwrap();
        wh.users.lock().await["x"].lock().await.checked = None;
        assert_eq!(count(oct).await, 2);
    }
}
