//! Raw data storage: a local directory for the prototype, a GCS bucket in
//! production. Both go through `object_store`, so the code paths are identical.

use std::sync::Arc;

use anyhow::{Context, Result};
use object_store::{
    ObjectStore, ObjectStoreExt, gcp::GoogleCloudStorageBuilder, local::LocalFileSystem, path::Path,
};
use serde_json::Value;

pub struct Store {
    inner: Arc<dyn ObjectStore>,
    pub location: String,
}

impl Store {
    /// `gs://bucket` or a local directory path.
    pub fn open(location: &str) -> Result<Self> {
        let inner: Arc<dyn ObjectStore> = if let Some(bucket) = location.strip_prefix("gs://") {
            Arc::new(
                GoogleCloudStorageBuilder::from_env()
                    .with_bucket_name(bucket.trim_end_matches('/'))
                    .build()?,
            )
        } else {
            std::fs::create_dir_all(location)
                .with_context(|| format!("cannot create data dir {location}"))?;
            Arc::new(LocalFileSystem::new_with_prefix(location)?)
        };
        Ok(Self {
            inner,
            location: location.to_string(),
        })
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        match self.inner.head(&Path::from(path)).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Newline-delimited JSON, one data point per line (loads directly into
    /// BigQuery and DuckDB).
    pub async fn put_jsonl(&self, path: &str, rows: &[Value]) -> Result<()> {
        let mut buf = Vec::new();
        for row in rows {
            serde_json::to_writer(&mut buf, row)?;
            buf.push(b'\n');
        }
        self.inner.put(&Path::from(path), buf.into()).await?;
        Ok(())
    }

    pub async fn put_json(&self, path: &str, value: &Value) -> Result<()> {
        let buf = serde_json::to_vec_pretty(value)?;
        self.inner.put(&Path::from(path), buf.into()).await?;
        Ok(())
    }
}
