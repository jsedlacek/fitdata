//! The user registry: one object per user, `users/{uid}.json`, keyed by the
//! Google Health user ID (`healthUserId`, the API's `users/me/identity`). It holds the profile and the encrypted Health
//! API refresh token (`kms.rs`); the user's data lives under `u/{uid}/`.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::kms::Kms;
use crate::oauth::StoredToken;
use crate::store::Store;

pub const DEFAULT_TIME_ZONE: &str = "Europe/Prague";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub uid: String,
    pub created: DateTime<Utc>,
    /// IANA name; local dates ("today") of the sync follow it.
    #[serde(default = "default_tz")]
    pub time_zone: String,
    /// Display preferences (Settings page); `None` until the user (or their
    /// browser, on the first visit) has chosen them.
    #[serde(default)]
    pub prefs: Option<Prefs>,
    /// The Health API refresh token, once Fitbit is connected.
    #[serde(default)]
    pub token: Option<EncryptedToken>,
    /// Set by "delete account"; the job then removes the user's data,
    /// revokes the token and drops this record. Until then the user cannot
    /// sign in again.
    #[serde(default)]
    pub deleted: bool,
}

impl User {
    pub fn new(uid: &str) -> Self {
        Self {
            uid: uid.to_string(),
            created: Utc::now(),
            time_zone: default_tz(),
            prefs: None,
            token: None,
            deleted: false,
        }
    }
}

impl User {
    /// `time_zone`, or the default if it isn't a known zone.
    pub fn tz(&self) -> chrono_tz::Tz {
        self.time_zone.parse().unwrap_or(chrono_tz::Europe::Prague)
    }
}

fn default_tz() -> String {
    DEFAULT_TIME_ZONE.to_string()
}

/// How the dashboard (and Claude or ChatGPT, through the MCP instructions)
/// shows the data. The data itself is always stored in metric units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub units: Units,
    pub week_start: WeekStart,
    pub clock: Clock,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
    Saturday,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Clock {
    #[default]
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

/// A time zone the user may choose: a known IANA name.
pub fn check_time_zone(name: &str) -> Result<chrono_tz::Tz> {
    name.parse()
        .map_err(|_| anyhow::anyhow!("unknown time zone {name:?}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedToken {
    /// `StoredToken` JSON encrypted with KMS (base64).
    pub ciphertext: String,
    pub obtained_at: DateTime<Utc>,
}

/// Health user IDs are decimal digits; anything else could escape the
/// user's prefix in paths.
pub fn check_uid(uid: &str) -> Result<()> {
    if uid.is_empty() || uid.len() > 64 || !uid.bytes().all(|b| b.is_ascii_digit()) {
        bail!("invalid user ID {uid:?}");
    }
    Ok(())
}

pub fn path(uid: &str) -> String {
    format!("users/{uid}.json")
}

pub async fn get(store: &Store, uid: &str) -> Result<Option<User>> {
    check_uid(uid)?;
    match store.get(&path(uid)).await? {
        Some(b) => Ok(Some(
            serde_json::from_slice(&b).with_context(|| format!("bad record of user {uid}"))?,
        )),
        None => Ok(None),
    }
}

pub async fn put(store: &Store, user: &User) -> Result<()> {
    check_uid(&user.uid)?;
    store
        .put(&path(&user.uid), serde_json::to_vec_pretty(user)?)
        .await
}

/// Encrypts `token` into the user's record, creating the record if needed.
pub async fn save_token(store: &Store, kms: &Kms, uid: &str, token: &StoredToken) -> Result<()> {
    let mut user = get(store, uid).await?.unwrap_or_else(|| User::new(uid));
    user.token = Some(EncryptedToken {
        ciphertext: kms.encrypt(&token.to_json()?, uid).await?,
        obtained_at: token.obtained_at,
    });
    put(store, &user).await
}

/// Every registered user, by ID.
pub async fn list(store: &Store) -> Result<Vec<User>> {
    let mut out = Vec::new();
    for (name, _) in store.list("users/").await? {
        let Some(uid) = name
            .strip_prefix("users/")
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        match get(store, uid).await {
            Ok(Some(u)) => out.push(u),
            Ok(None) => {}
            Err(e) => tracing::error!("{e:#}"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_digits() {
        assert!(check_uid("1234567890123456789").is_ok());
        for bad in ["", "../x", "1/2", "a", "1 2"] {
            assert!(check_uid(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn prefs_serialize_as_the_page_sends_them() {
        let p = Prefs {
            units: Units::Imperial,
            week_start: WeekStart::Sunday,
            clock: Clock::H12,
        };
        let v = serde_json::to_value(p).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"units": "imperial", "weekStart": "sunday", "clock": "12h"})
        );
        assert!(check_time_zone("America/New_York").is_ok());
        assert!(check_time_zone("Mars/Olympus").is_err());
    }

    #[tokio::test]
    async fn stores_and_lists_users() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        put(&store, &User::new("1")).await.unwrap();
        let all = list(&store).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].uid, "1");
        assert!(get(&store, "2").await.unwrap().is_none());
        // Records from before a field existed (or with one dropped since) still read.
        store
            .put(
                "users/3.json",
                br#"{"uid":"3","email":"x@y.z","created":"2026-10-24T10:00:00Z"}"#.to_vec(),
            )
            .await
            .unwrap();
        let three = get(&store, "3").await.unwrap().unwrap();
        assert_eq!(three.time_zone, DEFAULT_TIME_ZONE);
        assert!(three.token.is_none());
        assert!(three.prefs.is_none());
    }
}
