//! The sync job (`fitdata job`): for every user with a connected Fitbit,
//! re-fetch the last days, rebuild their tables and record the outcome in
//! their status. A new user gets their whole history first.
//!
//! Users are independent: one failing does not stop the others. A token
//! that no longer works is the user's state (the dashboard asks them to
//! reconnect), not a failure of the job; any other error fails the run, so
//! it alerts.
//!
//! Requests count against a quota per user (300 a minute), so:
//! - a user is synced by one process at a time (`Lock`), which lets the
//!   client's rate limiter see all of the user's requests;
//! - users are synced in parallel, each with their own quota;
//! - an hourly run fetches only yesterday and today, and skips the data
//!   types the user has no data for; once a day a full run re-fetches the
//!   whole window (devices sync late) and checks every type again;
//! - nothing before the account was created is fetched.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Days, Duration, Months, NaiveDate, Utc};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use crate::api::HealthClient;
use crate::datatypes::DataType;
use crate::kms::Kms;
use crate::oauth::{self, ClientCredentials, StoredToken, TokenSource};
use crate::status::{self, Run, Status};
use crate::store::Store;
use crate::tables::user_prefix;
use crate::users::{self, User};
use crate::{datatypes, model, sync, tables};

pub struct Ctx<'a> {
    pub http: &'a reqwest::Client,
    pub creds: &'a ClientCredentials,
    pub store: &'a Store,
    pub kms: &'a Kms,
}

/// Months of history a backfill stops at when the API reports nothing older.
const MAX_EMPTY_MONTHS: u32 = 3;
/// Users synced at once; each has a quota of their own.
const USERS_AT_ONCE: usize = 8;
/// How often a run re-fetches the whole window and probes every data type.
const FULL_EVERY: Duration = Duration::hours(20);
/// Days an hourly run fetches: yesterday and today.
const SHORT_DAYS: u64 = 1;
/// A lock older than this belongs to a run that died.
const LOCK_TTL: Duration = Duration::minutes(45);

/// Today in the user's time zone.
pub fn today(user: &User) -> NaiveDate {
    let tz: chrono_tz::Tz = user.time_zone.parse().unwrap_or(chrono_tz::Europe::Prague);
    Utc::now().with_timezone(&tz).date_naive()
}

/// Which share of the users this task syncs (Cloud Run job tasks).
fn my_share(uid: &str) -> bool {
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok());
    let (Some(index), Some(count)) = (env("CLOUD_RUN_TASK_INDEX"), env("CLOUD_RUN_TASK_COUNT"))
    else {
        return true;
    };
    // FNV-1a: stable across runs and builds, unlike the std hasher.
    let hash = uid.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    hash % count.max(1) == index
}

pub async fn run(ctx: &Ctx<'_>, days: u64, only: Option<&str>) -> Result<()> {
    let users = users::list(ctx.store).await?;
    let mine: Vec<&User> = users
        .iter()
        .filter(|u| only.is_none_or(|o| o == u.uid) && my_share(&u.uid))
        .collect();
    let failed: Vec<String> = stream::iter(mine)
        .map(|user| {
            let span = tracing::info_span!("user", otel.name = "sync user", uid = %user.uid);
            async move { (!run_user(ctx, user, days).await).then(|| user.uid.clone()) }
                .instrument(span)
        })
        .buffer_unordered(USERS_AT_ONCE)
        .filter_map(|f| async { f })
        .collect()
        .await;
    if !failed.is_empty() {
        bail!(
            "sync failed for {} user(s): {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

/// One user, under their lock; false if it failed (other than a token that
/// no longer works).
async fn run_user(ctx: &Ctx<'_>, user: &User, days: u64) -> bool {
    if user.token.is_none() && !user.deleted {
        return true;
    }
    let lock = match Lock::acquire(ctx.store, &user.uid).await {
        Ok(Some(l)) => l,
        Ok(None) => {
            tracing::info!("already being synced by another run; skipped");
            return true;
        }
        Err(e) => {
            tracing::error!("locking failed: {e:#}");
            return false;
        }
    };
    let ok = if user.deleted {
        match delete_user(ctx, user).await {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("deleting user failed: {e:#}");
                false
            }
        }
    } else {
        match sync_and_record(ctx, user, days).await {
            Ok(ok) => ok,
            Err(e) => {
                tracing::error!("recording the run failed: {e:#}");
                false
            }
        }
    };
    if let Err(e) = lock.release(ctx.store).await {
        tracing::warn!("releasing the lock failed: {e:#}");
    }
    ok
}

/// Syncs a user and records the outcome in their status.
async fn sync_and_record(ctx: &Ctx<'_>, user: &User, days: u64) -> Result<bool> {
    let started = Utc::now();
    status::started(ctx.store, &user.uid, started).await?;
    let before = status::get(ctx.store, &user.uid).await?;
    let result = sync_user(ctx, user, days, &before).await;
    let auth_error = result
        .as_ref()
        .err()
        .is_some_and(|e| e.chain().any(|c| c.is::<oauth::AuthError>()));
    let ok = match &result {
        Ok(_) => {
            tracing::info!("synced");
            true
        }
        Err(e) if auth_error => {
            tracing::warn!("needs to reconnect Fitbit: {e:#}");
            true
        }
        Err(e) => {
            tracing::error!("sync failed: {e:#}");
            false
        }
    };
    let run = Run {
        at: started,
        ok: result.is_ok(),
        auth_error,
        error: result.as_ref().err().map(|e| format!("{e:#}")),
        skipped: result
            .as_ref()
            .map(|s| s.skipped.iter().map(|t| t.to_string()).collect())
            .unwrap_or_default(),
    };
    status::finished(ctx.store, &user.uid, run, |s| {
        if let Ok(synced) = result {
            if synced.full {
                s.last_full = Some(started);
                s.absent = synced.absent;
            }
            s.since = synced.since.or(s.since);
        }
    })
    .await?;
    Ok(ok)
}

/// A user being synced: `u/{uid}/lock`, created only if absent (atomic in
/// GCS), so a scheduled run and "sync now" never sync one user at once.
pub struct Lock {
    generation: i64,
    uid: String,
}

#[derive(Serialize, Deserialize)]
struct LockBody {
    since: DateTime<Utc>,
    /// The Cloud Run execution holding it, for debugging.
    holder: String,
}

fn lock_path(uid: &str) -> String {
    format!("{}/lock", user_prefix(uid))
}

impl Lock {
    /// The user's lock, or `None` if another run holds it. A lock older
    /// than `LOCK_TTL` is taken over.
    pub async fn acquire(store: &Store, uid: &str) -> Result<Option<Lock>> {
        let path = lock_path(uid);
        for _ in 0..2 {
            let body = serde_json::to_vec(&LockBody {
                since: Utc::now(),
                holder: std::env::var("CLOUD_RUN_EXECUTION").unwrap_or_else(|_| "local".into()),
            })?;
            if let Some(generation) = store.create_new(&path, body).await? {
                return Ok(Some(Lock {
                    generation,
                    uid: uid.to_string(),
                }));
            }
            let Some(generation) = store.generation(&path).await? else {
                continue; // released in between
            };
            let stale = match store.get(&path).await? {
                Some(b) => serde_json::from_slice::<LockBody>(&b)
                    .map_or(true, |l| Utc::now() - l.since > LOCK_TTL),
                None => true,
            };
            if !stale {
                return Ok(None);
            }
            tracing::warn!("taking over a stale lock");
            store.delete_generation(&path, generation).await?;
        }
        Ok(None)
    }

    pub async fn release(self, store: &Store) -> Result<()> {
        store
            .delete_generation(&lock_path(&self.uid), self.generation)
            .await?;
        Ok(())
    }
}

pub async fn client(ctx: &Ctx<'_>, user: &User) -> Result<HealthClient> {
    let token = user.token.as_ref().context("Fitbit not connected")?;
    let plain = ctx.kms.decrypt(&token.ciphertext, &user.uid).await?;
    let refresh_token = StoredToken::from_json(&plain)?.refresh_token;
    Ok(HealthClient::new(
        ctx.http.clone(),
        TokenSource::new(ctx.http.clone(), ctx.creds.clone(), refresh_token),
    ))
}

/// What a sync did and learned.
struct Synced {
    /// Data types skipped as not readable.
    skipped: Vec<&'static str>,
    /// A full run: every day of the window, every data type probed.
    full: bool,
    /// After a full run: the types without any data.
    absent: Vec<String>,
    since: Option<NaiveDate>,
}

/// Whether this run re-fetches the whole window and probes every type.
fn full_run(before: &Status, now: DateTime<Utc>) -> bool {
    before.last_ok.is_none() || before.last_full.is_none_or(|t| now - t > FULL_EVERY)
}

/// One user: the days to refresh (see the module docs), the whole history
/// the first time, then their tables.
async fn sync_user(ctx: &Ctx<'_>, user: &User, days: u64, before: &Status) -> Result<Synced> {
    let client = client(ctx, user).await?;
    let raw = ctx.store.scoped(&user_prefix(&user.uid));
    let today = today(user);
    let first = before.last_ok.is_none();
    let full = full_run(before, Utc::now());

    let since = match before.since {
        Some(d) if !full => Some(d),
        _ => client.membership_start().await?,
    };
    let (types, absent): (Vec<&'static DataType>, Vec<String>) = if full {
        // One cheap request per type: is there any data at all?
        let probe = sync::probe(&client, &datatypes::ALL.iter().collect::<Vec<_>>()).await;
        let empty: Vec<String> = probe
            .iter()
            .filter(|p| p["status"] == "empty")
            .filter_map(|p| p["dataType"].as_str().map(String::from))
            .collect();
        let types = datatypes::ALL
            .iter()
            .filter(|t| !empty.iter().any(|e| e == t.id))
            .collect();
        (types, empty)
    } else {
        let types = datatypes::ALL
            .iter()
            .filter(|t| !before.absent.iter().any(|a| a == t.id))
            .collect();
        (types, before.absent.clone())
    };

    let window = if full { days } else { SHORT_DAYS.min(days) };
    let from = (today - Days::new(window)).max(since.unwrap_or(NaiveDate::MIN));
    let fetched = sync::fetch_range(&client, &raw, &types, from, today + Days::new(1)).await?;
    tracing::info!(
        full,
        types = types.len(),
        "fetched {from}..{today}: {} data points",
        fetched.points
    );
    if !fetched.skipped.is_empty() {
        tracing::warn!("data types not readable: {}", fetched.skipped.join(", "));
    }
    if first {
        let n = sync::backfill(&client, &raw, &types, since, MAX_EMPTY_MONTHS).await?;
        tracing::info!("backfilled {n} data points");
        let start = since.unwrap_or(from).min(from);
        load(ctx.store, &raw, &user.uid, start, today).await?;
    } else {
        load(ctx.store, &raw, &user.uid, from, today).await?;
    }
    Ok(Synced {
        skipped: fetched.skipped,
        full,
        absent,
        since,
    })
}

/// Rebuilds the user's tables for local dates `[from, to]` from their raw
/// data, a month at a time (a month of heart rate is ~800k points).
pub async fn load(
    store: &Store,
    raw: &Store,
    uid: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<()> {
    if from > to {
        bail!("--from must not be after --to");
    }
    let mut start = from;
    while start <= to {
        let next_month = start
            .with_day(1)
            .and_then(|d| d.checked_add_months(Months::new(1)))
            .context("date out of range")?;
        let end = (next_month - Days::new(1)).min(to);
        // One extra day on each side: nights start the evening before, and
        // intervals near midnight may be stored in the neighbouring day's file.
        let points = sync::read_raw(raw, start - Days::new(1), end + Days::new(2)).await?;
        let ds = model::Dataset::from_points(points);
        let t = model::table_rows(&ds, start, end);
        tables::write_tables(store, uid, &t, start, end).await?;
        start = end + Days::new(1);
    }
    tables::write_manifest(store, uid).await?;
    tracing::info!("loaded {from}..{to}");
    Ok(())
}

/// "Delete account": revoke the token, remove the data, then the record.
async fn delete_user(ctx: &Ctx<'_>, user: &User) -> Result<()> {
    if user.token.is_some() {
        match client_token(ctx, user).await {
            Ok(t) => oauth::revoke(ctx.http, &t).await.unwrap_or_else(|e| {
                tracing::warn!("revoking the token failed: {e:#}");
            }),
            Err(e) => tracing::warn!("reading the token failed: {e:#}"),
        }
    }
    let prefix = format!("{}/", user_prefix(&user.uid));
    for (name, _) in ctx.store.list(&prefix).await? {
        ctx.store.delete(&name).await?;
    }
    ctx.store.delete(&users::path(&user.uid)).await?;
    tracing::info!("deleted the user and their data");
    Ok(())
}

async fn client_token(ctx: &Ctx<'_>, user: &User) -> Result<String> {
    let token = user.token.as_ref().context("no token")?;
    let plain = ctx.kms.decrypt(&token.ciphertext, &user.uid).await?;
    Ok(StoredToken::from_json(&plain)?.refresh_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_users_between_tasks() {
        // Without task variables every user is ours.
        assert!(my_share("1"));
    }

    #[test]
    fn runs_in_full_once_a_day() {
        let now = Utc::now();
        let mut s = Status::default();
        assert!(full_run(&s, now), "first run");
        s.last_ok = Some(now);
        assert!(full_run(&s, now), "never full");
        s.last_full = Some(now - Duration::hours(2));
        assert!(!full_run(&s, now));
        s.last_full = Some(now - Duration::hours(21));
        assert!(full_run(&s, now));
    }

    #[tokio::test]
    async fn one_run_per_user() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let a = Lock::acquire(&store, "1").await.unwrap().unwrap();
        assert!(Lock::acquire(&store, "1").await.unwrap().is_none());
        assert!(Lock::acquire(&store, "2").await.unwrap().is_some());
        a.release(&store).await.unwrap();
        let b = Lock::acquire(&store, "1").await.unwrap().unwrap();

        // A lock left by a run that died is taken over.
        let old = LockBody {
            since: Utc::now() - Duration::hours(2),
            holder: "dead".into(),
        };
        store
            .put(&lock_path("3"), serde_json::to_vec(&old).unwrap())
            .await
            .unwrap();
        assert!(Lock::acquire(&store, "3").await.unwrap().is_some());
        b.release(&store).await.unwrap();
    }
}
