//! The user registry: one object per user, `users/{uid}.json`, keyed by the
//! Google account ID (`sub`). It holds the profile and the encrypted Health
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
    pub email: String,
    pub created: DateTime<Utc>,
    /// IANA name; local dates ("today") of the sync follow it.
    #[serde(default = "default_tz")]
    pub time_zone: String,
    /// The Health API refresh token, once Fitbit is connected.
    #[serde(default)]
    pub token: Option<EncryptedToken>,
    /// Set by "delete account"; the job then removes the user's data,
    /// revokes the token and drops this record.
    #[serde(default)]
    pub deleted: bool,
}

impl User {
    pub fn new(uid: &str, email: &str) -> Self {
        Self {
            uid: uid.to_string(),
            email: email.to_string(),
            created: Utc::now(),
            time_zone: default_tz(),
            token: None,
            deleted: false,
        }
    }
}

fn default_tz() -> String {
    DEFAULT_TIME_ZONE.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedToken {
    /// `StoredToken` JSON encrypted with KMS (base64).
    pub ciphertext: String,
    pub obtained_at: DateTime<Utc>,
}

/// Google account IDs are decimal digits; anything else could escape the
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
pub async fn save_token(
    store: &Store,
    kms: &Kms,
    uid: &str,
    email: &str,
    token: &StoredToken,
) -> Result<()> {
    let mut user = get(store, uid)
        .await?
        .unwrap_or_else(|| User::new(uid, email));
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
        assert!(check_uid("102707141805967147144").is_ok());
        for bad in ["", "../x", "1/2", "a", "1 2"] {
            assert!(check_uid(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn stores_and_lists_users() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let u = User {
            uid: "1".into(),
            email: "a@b.c".into(),
            created: Utc::now(),
            time_zone: default_tz(),
            token: None,
            deleted: false,
        };
        put(&store, &u).await.unwrap();
        let all = list(&store).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].email, "a@b.c");
        assert!(get(&store, "2").await.unwrap().is_none());
        // Records from before a field existed still read.
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
    }
}
