//! The sync job (`fitdata job`): for every user with a connected Fitbit,
//! re-fetch the last days, rebuild their tables and record the outcome in
//! their status. A new user gets their whole history first.
//!
//! Users are independent: one failing does not stop the others. A token
//! that no longer works is the user's state (the dashboard asks them to
//! reconnect), not a failure of the job; any other error fails the run, so
//! it alerts.

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Days, Months, NaiveDate, Utc};

use crate::api::HealthClient;
use crate::kms::Kms;
use crate::oauth::{self, ClientCredentials, StoredToken, TokenSource};
use crate::status::{self, Run};
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
    let mut failed = Vec::new();
    for user in users
        .iter()
        .filter(|u| only.is_none_or(|o| o == u.uid) && my_share(&u.uid))
    {
        let span = tracing::info_span!("user", uid = %user.uid);
        let _e = span.enter();
        if user.deleted {
            if let Err(e) = delete_user(ctx, user).await {
                tracing::error!("deleting user failed: {e:#}");
                failed.push(user.uid.clone());
            }
            continue;
        }
        if user.token.is_none() {
            continue;
        }
        let started = Utc::now();
        status::started(ctx.store, &user.uid, started).await?;
        let result = sync_user(ctx, user, days).await;
        let auth_error = result
            .as_ref()
            .err()
            .is_some_and(|e| e.chain().any(|c| c.is::<oauth::AuthError>()));
        match &result {
            Ok(_) => tracing::info!("synced"),
            Err(e) if auth_error => tracing::warn!("needs to reconnect Fitbit: {e:#}"),
            Err(e) => {
                tracing::error!("sync failed: {e:#}");
                failed.push(user.uid.clone());
            }
        }
        let run = Run {
            at: started,
            ok: result.is_ok(),
            auth_error,
            error: result.as_ref().err().map(|e| format!("{e:#}")),
            skipped: result
                .map(|s| s.into_iter().map(String::from).collect())
                .unwrap_or_default(),
        };
        status::finished(ctx.store, &user.uid, run).await?;
    }
    if !failed.is_empty() {
        bail!(
            "sync failed for {} user(s): {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok(())
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

/// One user: the trailing `days` (or the whole history the first time),
/// then their tables. Returns the data types skipped as not readable.
async fn sync_user(ctx: &Ctx<'_>, user: &User, days: u64) -> Result<Vec<&'static str>> {
    let client = client(ctx, user).await?;
    let raw = ctx.store.scoped(&user_prefix(&user.uid));
    let today = today(user);
    let types: Vec<_> = datatypes::ALL.iter().collect();
    let from = today - Days::new(days);
    let fetched = sync::fetch_range(&client, &raw, &types, from, today + Days::new(1)).await?;
    if !fetched.skipped.is_empty() {
        tracing::warn!("data types not readable: {}", fetched.skipped.join(", "));
    }
    let first = status::get(ctx.store, &user.uid).await?.last_ok.is_none();
    if first {
        let since = client.membership_start().await?;
        let n = sync::backfill(&client, &raw, &types, since, MAX_EMPTY_MONTHS).await?;
        tracing::info!("backfilled {n} data points");
        let start = since.unwrap_or(from).min(from);
        load(ctx.store, &raw, &user.uid, start, today).await?;
    } else {
        load(ctx.store, &raw, &user.uid, from, today).await?;
    }
    Ok(fetched.skipped)
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
}
