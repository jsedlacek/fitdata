//! Object storage for raw data and tables: a local directory for
//! development, a GCS bucket in production. A store can be scoped to a
//! prefix (`scoped`), so a user's raw data lands under `u/{uid}/`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use google_cloud_gax::error::rpc::Code;
use google_cloud_storage::client::{Storage, StorageControl};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone)]
pub struct Store {
    backend: Arc<Backend>,
    /// Prepended to every path (`a/b`, no slashes around), or empty.
    prefix: String,
}

// One per process, behind an Arc: the size difference does not matter.
#[allow(clippy::large_enum_variant)]
enum Backend {
    Local(PathBuf),
    Gcs {
        client: Storage,
        /// Metadata, listings and deletes.
        control: StorageControl,
        bucket: String,
    },
}

fn is_not_found(e: &google_cloud_storage::Error) -> bool {
    e.http_status_code() == Some(404) || e.status().is_some_and(|s| s.code == Code::NotFound)
}

impl Store {
    /// `gs://bucket` or a local directory path.
    pub async fn open(location: &str) -> Result<Self> {
        let backend = if let Some(bucket) = location.strip_prefix("gs://") {
            let client = Storage::builder().build().await.context("GCS client")?;
            let control = StorageControl::builder()
                .build()
                .await
                .context("GCS control client")?;
            Backend::Gcs {
                client,
                control,
                bucket: bucket.trim_end_matches('/').to_string(),
            }
        } else {
            std::fs::create_dir_all(location)
                .with_context(|| format!("cannot create data dir {location}"))?;
            Backend::Local(PathBuf::from(location))
        };
        Ok(Self {
            backend: Arc::new(backend),
            prefix: String::new(),
        })
    }

    /// The same storage with paths under `prefix`.
    pub fn scoped(&self, prefix: &str) -> Self {
        Self {
            backend: self.backend.clone(),
            prefix: self.key(prefix.trim_matches('/')),
        }
    }

    /// The full object name of `path`.
    fn key(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_string()
        } else {
            format!("{}/{path}", self.prefix)
        }
    }

    pub fn location(&self) -> String {
        let base = match &*self.backend {
            Backend::Local(p) => p.display().to_string(),
            Backend::Gcs { bucket, .. } => format!("gs://{bucket}"),
        };
        if self.prefix.is_empty() {
            base
        } else {
            format!("{base}/{}", self.prefix)
        }
    }

    /// The local directory, if the store is one (tests, development).
    #[cfg(test)]
    pub fn local_dir(&self) -> Option<PathBuf> {
        match &*self.backend {
            Backend::Local(p) => Some(p.join(&self.prefix)),
            Backend::Gcs { .. } => None,
        }
    }

    #[tracing::instrument(name = "storage read", skip(self), fields(otel.kind = "client"))]
    pub async fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let key = self.key(path);
        match &*self.backend {
            Backend::Local(dir) => match std::fs::read(dir.join(&key)) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Backend::Gcs { client, bucket, .. } => {
                let mut resp = match client
                    .read_object(format!("projects/_/buckets/{bucket}"), &key)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) if is_not_found(&e) => return Ok(None),
                    Err(e) => {
                        return Err(e).with_context(|| format!("reading gs://{bucket}/{key}"));
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
        let key = self.key(path);
        match &*self.backend {
            Backend::Local(dir) => Ok(dir.join(&key).exists()),
            Backend::Gcs {
                control, bucket, ..
            } => match control
                .get_object()
                .set_bucket(format!("projects/_/buckets/{bucket}"))
                .set_object(&key)
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(e) if is_not_found(&e) => Ok(false),
                Err(e) => Err(e).with_context(|| format!("checking gs://{bucket}/{key}")),
            },
        }
    }

    /// Removes an object; a missing one is not an error.
    #[tracing::instrument(name = "storage delete", skip(self), fields(otel.kind = "client"))]
    pub async fn delete(&self, path: &str) -> Result<()> {
        let key = self.key(path);
        match &*self.backend {
            Backend::Local(dir) => match std::fs::remove_file(dir.join(&key)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
            Backend::Gcs {
                control, bucket, ..
            } => match control
                .delete_object()
                .set_bucket(format!("projects/_/buckets/{bucket}"))
                .set_object(&key)
                .send()
                .await
            {
                Err(e) if !is_not_found(&e) => {
                    Err(e).with_context(|| format!("deleting gs://{bucket}/{key}"))
                }
                _ => Ok(()),
            },
        }
    }

    #[tracing::instrument(name = "storage write", skip(self, data), fields(otel.kind = "client", bytes = data.len()))]
    pub async fn put(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let key = self.key(path);
        match &*self.backend {
            Backend::Local(dir) => {
                let p = dir.join(&key);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(p, data)?;
            }
            Backend::Gcs { client, bucket, .. } => {
                client
                    .write_object(
                        format!("projects/_/buckets/{bucket}"),
                        &key,
                        bytes::Bytes::from(data),
                    )
                    .send_buffered()
                    .await
                    .with_context(|| format!("writing gs://{bucket}/{key}"))?;
            }
        }
        Ok(())
    }

    /// Objects under `prefix` (relative to the store) with their generation
    /// (in a local directory: the modification time in nanoseconds), sorted
    /// by name; names are relative to the store too.
    pub async fn list(&self, prefix: &str) -> Result<Vec<(String, i64)>> {
        let full = self.key(prefix);
        let mut out = Vec::new();
        match &*self.backend {
            Backend::Local(dir) => {
                fn walk(
                    root: &std::path::Path,
                    d: &std::path::Path,
                    out: &mut Vec<(String, i64)>,
                ) -> Result<()> {
                    let Ok(entries) = std::fs::read_dir(d) else {
                        return Ok(());
                    };
                    for e in entries {
                        let p = e?.path();
                        if p.is_dir() {
                            walk(root, &p, out)?;
                        } else {
                            let name = p.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                            let modified = std::fs::metadata(&p)?.modified()?;
                            let nanos = modified.duration_since(std::time::UNIX_EPOCH)?.as_nanos();
                            out.push((name, nanos as i64));
                        }
                    }
                    Ok(())
                }
                // The deepest directory that the prefix names.
                let start = match full.rfind('/') {
                    Some(i) => dir.join(&full[..i]),
                    None => dir.clone(),
                };
                walk(dir, &start, &mut out)?;
                out.retain(|(name, _)| name.starts_with(&full));
            }
            Backend::Gcs {
                control, bucket, ..
            } => {
                let mut page_token = String::new();
                loop {
                    let resp = control
                        .list_objects()
                        .set_parent(format!("projects/_/buckets/{bucket}"))
                        .set_prefix(&full)
                        .set_page_token(&page_token)
                        .send()
                        .await
                        .with_context(|| format!("listing gs://{bucket}/{full}"))?;
                    out.extend(resp.objects.into_iter().map(|o| (o.name, o.generation)));
                    if resp.next_page_token.is_empty() {
                        break;
                    }
                    page_token = resp.next_page_token;
                }
            }
        }
        let strip = if self.prefix.is_empty() {
            0
        } else {
            self.prefix.len() + 1
        };
        let mut out: Vec<(String, i64)> = out
            .into_iter()
            .map(|(name, generation)| (name[strip..].to_string(), generation))
            .collect();
        out.sort();
        Ok(out)
    }

    /// Newline-delimited JSON, one record per line.
    pub async fn put_jsonl<T: Serialize>(&self, path: &str, rows: &[T]) -> Result<()> {
        self.put(path, to_jsonl(rows)?).await
    }

    pub async fn put_json(&self, path: &str, value: &Value) -> Result<()> {
        self.put(path, serde_json::to_vec_pretty(value)?).await
    }
}

pub fn to_jsonl<T: Serialize>(rows: &[T]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut buf, row)?;
        buf.push(b'\n');
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scoped_stores_share_the_backend() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let user = root.scoped("u/1");
        user.put("raw/x.jsonl", b"a".to_vec()).await.unwrap();
        assert_eq!(root.get("u/1/raw/x.jsonl").await.unwrap().unwrap(), b"a");
        assert!(user.exists("raw/x.jsonl").await.unwrap());
        let names: Vec<_> = user
            .list("raw/")
            .await
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, ["raw/x.jsonl"]);
        let names: Vec<_> = root
            .list("u/")
            .await
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, ["u/1/raw/x.jsonl"]);
        user.delete("raw/x.jsonl").await.unwrap();
        assert!(root.list("").await.unwrap().is_empty());
    }
}
