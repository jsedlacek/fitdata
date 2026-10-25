//! "Download your data" (GDPR access and portability): one ZIP with
//! everything Fitdata stores about the user, streamed while it is written,
//! so neither the whole archive nor the whole history is held in memory.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::Instrument;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::store::Store;
use crate::tables::user_prefix;
use crate::users;
use crate::web::{Shared, SignedIn};

const README: &str = "\
Your Fitdata export
===================

Everything Fitdata stores about you, as of the date in the file name.

account.json         Your account: Google Health user ID, sign-up time and settings.
                     (The Fitbit access token is kept only encrypted and is left out.)
status.json          The state of the hourly sync.
raw/<type>/*.jsonl   Your data exactly as the Google Health API returned it,
                     one data point per line (JSON), by data type and day or month.
data/                The tables the dashboard and Claude/ChatGPT read, built from raw/.
                     Parquet opens in DuckDB, pandas, Polars, Excel (Power Query) and more.
                     Times are in your time zone (data/manifest.json).

Questions: fitdata@jakub.contact
";

/// The bytes the ZIP writer has produced and not sent yet.
#[derive(Clone, Default)]
struct Pending(Arc<Mutex<Vec<u8>>>);

impl Write for Pending {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("poisoned").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

type Chunks = mpsc::Sender<io::Result<Bytes>>;

impl Pending {
    async fn send(&self, tx: &Chunks) -> Result<()> {
        let chunk = std::mem::take(&mut *self.0.lock().expect("poisoned"));
        if chunk.is_empty() {
            return Ok(());
        }
        tx.send(Ok(chunk.into()))
            .await
            .map_err(|_| anyhow!("download cancelled"))
    }
}

pub async fn download(State(cfg): State<Shared>, Extension(user): Extension<SignedIn>) -> Response {
    let (tx, mut rx) = mpsc::channel(2);
    let store = cfg.store.clone();
    let uid = user.uid;
    tracing::info!(%uid, "data export started");
    tokio::spawn(
        async move {
            if let Err(e) = write(&store, &uid, &tx).await {
                tracing::warn!(%uid, "data export failed: {e:#}");
                // Ends the download with an error rather than a ZIP cut short.
                let _ = tx.send(Err(io::Error::other("export failed"))).await;
            }
        }
        .in_current_span(),
    );
    let body = futures::stream::poll_fn(move |cx| rx.poll_recv(cx));
    let date = chrono::Utc::now().format("%Y-%m-%d");
    (
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"fitdata-{date}.zip\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Body::from_stream(body),
    )
        .into_response()
}

/// Writes the user's export to `tx`, a file at a time.
async fn write(store: &Store, uid: &str, tx: &Chunks) -> Result<()> {
    let user = users::get(store, uid).await?.context("no such user")?;
    let out = Pending::default();
    let mut zip = zip::ZipWriter::new_stream(out.clone());
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip_time(chrono::Utc::now()));
    // Parquet is compressed already.
    let stored = deflated.compression_method(CompressionMethod::Stored);

    zip.start_file("README.txt", deflated)?;
    zip.write_all(README.as_bytes())?;
    let account = json!({
        "userId": user.uid,
        "created": user.created,
        "timeZone": user.time_zone,
        "settings": user.prefs.unwrap_or_default(),
        "fitbitConnected": user.token.is_some(),
        "fitbitConnectedAt": user.token.as_ref().map(|t| t.obtained_at),
    });
    zip.start_file("account.json", deflated)?;
    zip.write_all(&serde_json::to_vec_pretty(&account)?)?;
    out.send(tx).await?;

    let prefix = format!("{}/", user_prefix(uid));
    for (name, _) in store.list(&prefix).await? {
        let Some(path) = name.strip_prefix(&prefix) else {
            continue;
        };
        // The sync's lock says nothing about the user.
        if path == "lock" {
            continue;
        }
        // Removed since the listing (a sync replaced it): skip it.
        let Some(bytes) = store.get(&name).await? else {
            continue;
        };
        let opts = if path.ends_with(".parquet") {
            stored
        } else {
            deflated
        };
        zip.start_file(path, opts)?;
        zip.write_all(&bytes)?;
        out.send(tx).await?;
    }
    zip.finish()?;
    out.send(tx).await
}

/// A ZIP timestamp (no zone; UTC). Without one the files date from 1980.
fn zip_time(t: chrono::DateTime<chrono::Utc>) -> zip::DateTime {
    use chrono::{Datelike, Timelike};
    zip::DateTime::from_date_and_time(
        t.year() as u16,
        t.month() as u8,
        t.day() as u8,
        t.hour() as u8,
        t.minute() as u8,
        t.second() as u8,
    )
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::users::{EncryptedToken, User};

    #[tokio::test]
    async fn exports_everything_but_the_token_and_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let mut user = User::new("1");
        user.token = Some(EncryptedToken {
            ciphertext: "secret".into(),
            obtained_at: chrono::Utc::now(),
        });
        users::put(&store, &user).await.unwrap();
        for (path, body) in [
            ("u/1/raw/steps/2026-10-24.jsonl", "{}\n"),
            ("u/1/data/daily_2026.parquet", "PAR1"),
            ("u/1/status.json", "{}"),
            ("u/1/lock", "{}"),
            ("u/2/raw/steps/2026-10-24.jsonl", "{\"other\":1}\n"),
        ] {
            store.put(path, body.into()).await.unwrap();
        }

        let (tx, mut rx) = mpsc::channel(1000);
        write(&store, "1", &tx).await.unwrap();
        drop(tx);
        let mut bytes = Vec::new();
        while let Some(chunk) = rx.recv().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }

        let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).unwrap();
        let mut names: Vec<_> = archive
            .file_names()
            .map(|n| n.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "README.txt",
                "account.json",
                "data/daily_2026.parquet",
                "raw/steps/2026-10-24.jsonl",
                "status.json"
            ]
        );
        let mut account = String::new();
        io::Read::read_to_string(&mut archive.by_name("account.json").unwrap(), &mut account)
            .unwrap();
        assert!(account.contains("\"fitbitConnected\": true"));
        assert!(!account.contains("secret"));
    }
}
