//! Object storage for raw and staged data: a local directory for development,
//! a GCS bucket in production.

use std::path::PathBuf;

use anyhow::{Context, Result};
use google_cloud_gax::error::rpc::Code;
use google_cloud_storage::client::Storage;
use serde_json::Value;

pub enum Store {
    Local(PathBuf),
    Gcs {
        client: Box<Storage>,
        bucket: String,
    },
}

fn is_not_found(e: &google_cloud_storage::Error) -> bool {
    e.http_status_code() == Some(404) || e.status().is_some_and(|s| s.code == Code::NotFound)
}

impl Store {
    /// `gs://bucket` or a local directory path.
    pub async fn open(location: &str) -> Result<Self> {
        if let Some(bucket) = location.strip_prefix("gs://") {
            let client = Storage::builder().build().await.context("GCS client")?;
            return Ok(Self::Gcs {
                client: Box::new(client),
                bucket: bucket.trim_end_matches('/').to_string(),
            });
        }
        std::fs::create_dir_all(location)
            .with_context(|| format!("cannot create data dir {location}"))?;
        Ok(Self::Local(PathBuf::from(location)))
    }

    pub fn location(&self) -> String {
        match self {
            Self::Local(p) => p.display().to_string(),
            Self::Gcs { bucket, .. } => format!("gs://{bucket}"),
        }
    }

    /// `gs://` URI of an object, for BigQuery load jobs.
    pub fn gs_uri(&self, path: &str) -> Option<String> {
        match self {
            Self::Gcs { bucket, .. } => Some(format!("gs://{bucket}/{path}")),
            Self::Local(_) => None,
        }
    }

    pub async fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Local(dir) => match std::fs::read(dir.join(path)) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Self::Gcs { client, bucket } => {
                let mut resp = match client
                    .read_object(format!("projects/_/buckets/{bucket}"), path)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) if is_not_found(&e) => return Ok(None),
                    Err(e) => {
                        return Err(e).with_context(|| format!("reading gs://{bucket}/{path}"));
                    }
                };
                let mut out = Vec::new();
                while let Some(chunk) = resp.next().await.transpose()? {
                    out.extend_from_slice(&chunk);
                }
                Ok(Some(out))
            }
        }
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        match self {
            Self::Local(dir) => Ok(dir.join(path).exists()),
            // Objects are small; reading is simpler than a metadata call.
            Self::Gcs { .. } => Ok(self.get(path).await?.is_some()),
        }
    }

    pub async fn put(&self, path: &str, data: Vec<u8>) -> Result<()> {
        match self {
            Self::Local(dir) => {
                let p = dir.join(path);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(p, data)?;
            }
            Self::Gcs { client, bucket } => {
                client
                    .write_object(
                        format!("projects/_/buckets/{bucket}"),
                        path,
                        bytes::Bytes::from(data),
                    )
                    .send_buffered()
                    .await
                    .with_context(|| format!("writing gs://{bucket}/{path}"))?;
            }
        }
        Ok(())
    }

    /// Newline-delimited JSON, one record per line (loads directly into BigQuery).
    pub async fn put_jsonl(&self, path: &str, rows: &[Value]) -> Result<()> {
        self.put(path, to_jsonl(rows)?).await
    }

    pub async fn put_json(&self, path: &str, value: &Value) -> Result<()> {
        self.put(path, serde_json::to_vec_pretty(value)?).await
    }

    /// Lines of a JSONL object; empty if the object does not exist.
    pub async fn get_jsonl(&self, path: &str) -> Result<Vec<Value>> {
        let Some(data) = self.get(path).await? else {
            return Ok(Vec::new());
        };
        std::str::from_utf8(&data)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).with_context(|| format!("bad JSON line in {path}")))
            .collect()
    }
}

pub fn to_jsonl(rows: &[Value]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut buf, row)?;
        buf.push(b'\n');
    }
    Ok(buf)
}
