//! The normalized tables as Parquet files in the store, one set per user:
//!
//! ```text
//! u/{uid}/data/{table}/{YYYY-MM}.parquet   heart_rate, activity, spo2, hrv
//! u/{uid}/data/{table}.parquet             daily, nights, exercises
//! u/{uid}/data/manifest.json               every file with its generation
//! ```
//!
//! A load rewrites only the files its date range touches: the rows of the
//! range are replaced, the rest of each file is kept. The manifest is
//! written last, so a reader that follows it sees whole files of one load.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Datelike, Months, NaiveDate};
use datafusion::arrow::array::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::parquet::arrow::ArrowWriter;
use datafusion::parquet::basic::{Compression, ZstdLevel};
use datafusion::parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};

use crate::engine;
use crate::model::{Row, Tables};
use crate::store::Store;

/// Tables split into one file per month; the others are small.
pub const MONTHLY: &[&str] = &["heart_rate", "activity", "spo2", "hrv"];

pub fn user_prefix(uid: &str) -> String {
    format!("u/{uid}")
}

fn data_prefix(uid: &str) -> String {
    format!("{}/data", user_prefix(uid))
}

/// Path of a table file relative to the user's data prefix.
pub fn file_name(table: &str, month: Option<&str>) -> String {
    match month {
        Some(m) => format!("{table}/{m}.parquet"),
        None => format!("{table}.parquet"),
    }
}

/// `YYYY-MM` of every month overlapping `[from, to]`.
pub fn months(from: NaiveDate, to: NaiveDate) -> Vec<String> {
    let mut out = Vec::new();
    let mut d = from.with_day(1).expect("day 1 exists");
    while d <= to {
        out.push(d.format("%Y-%m").to_string());
        d = d
            .checked_add_months(chrono::Months::new(1))
            .expect("date in range");
    }
    out
}

/// The columns rows are sorted by within a file (for compression and for
/// row-group statistics on time).
fn sort_key(table: &str) -> String {
    let schema = engine::schema(table);
    let time = ["ts", "start"]
        .into_iter()
        .find(|t| schema.field_with_name(t).is_ok());
    match time {
        Some(t) => format!("date, \"{t}\""),
        None => "date".to_string(),
    }
}

/// `rows` as a record batch of the table's schema.
fn batch<R: Row>(rows: &[R]) -> Result<RecordBatch> {
    let schema = engine::schema(R::TABLE);
    Ok(serde_arrow::to_record_batch(schema.fields(), &rows)?)
}

/// One file's new content: the old file without the dates in `[from, to]`,
/// plus the new rows, limited to `month` if the table is split by month.
/// Returns the Parquet bytes and the number of rows.
async fn merge(
    table: &str,
    old: Option<PathBuf>,
    new: Vec<RecordBatch>,
    month: Option<&str>,
    (from, to): (NaiveDate, NaiveDate),
) -> Result<(Vec<u8>, usize)> {
    let schema = engine::schema(table);
    let ctx = engine::session(512 << 20)?;
    ctx.register_table(
        "incoming",
        Arc::new(MemTable::try_new(schema.clone(), vec![new])?),
    )?;
    let cols = schema
        .fields()
        .iter()
        .map(|f| format!("\"{}\"", f.name()))
        .collect::<Vec<_>>()
        .join(", ");
    let kept = match old {
        Some(old) => {
            engine::register_as(&ctx, "existing", table, &[old]).await?;
            format!(
                "SELECT {cols} FROM existing WHERE date NOT BETWEEN DATE '{from}' AND DATE '{to}'
                 UNION ALL "
            )
        }
        None => String::new(),
    };
    let filter = match month {
        Some(m) => {
            let first = NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")?;
            let next = first + Months::new(1);
            format!("WHERE date >= DATE '{first}' AND date < DATE '{next}'")
        }
        None => String::new(),
    };
    let sql = format!(
        "SELECT {cols} FROM ({kept}SELECT {cols} FROM incoming) {filter} ORDER BY {}",
        sort_key(table)
    );
    let result = engine::run(&ctx, &sql).await?;
    let n = result.iter().map(RecordBatch::num_rows).sum();
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let mut buf = Vec::new();
    let mut w = ArrowWriter::try_new(&mut buf, schema, Some(props))?;
    for b in &result {
        w.write(b)?;
    }
    w.close()?;
    Ok((buf, n))
}

/// Replaces the rows of `R`'s table for local dates `[from, to]` in the
/// user's files. A file left without rows is deleted.
#[tracing::instrument(skip_all, fields(otel.name = format!("write {}", R::TABLE), table = R::TABLE, %from, %to))]
pub async fn replace_range<R: Row>(
    store: &Store,
    uid: &str,
    rows: &[R],
    from: NaiveDate,
    to: NaiveDate,
) -> Result<()> {
    let table = R::TABLE;
    let dir = tempfile::tempdir()?;
    let new = vec![batch(rows)?];
    let targets: Vec<Option<String>> = if MONTHLY.contains(&table) {
        months(from, to).into_iter().map(Some).collect()
    } else {
        vec![None]
    };
    let mut written = 0;
    for month in targets {
        let path = format!(
            "{}/{}",
            data_prefix(uid),
            file_name(table, month.as_deref())
        );
        let old = match store.get(&path).await? {
            Some(bytes) => {
                let p = dir.path().join("old.parquet");
                std::fs::write(&p, bytes)?;
                Some(p)
            }
            None => None,
        };
        let (bytes, n) = merge(table, old, new.clone(), month.as_deref(), (from, to))
            .await
            .with_context(|| format!("writing {path}"))?;
        if n == 0 {
            store.delete(&path).await?;
        } else {
            store.put(&path, bytes).await?;
        }
        written += n;
    }
    tracing::info!(
        "wrote {from}..{to}: {} new rows, {written} in files",
        rows.len()
    );
    Ok(())
}

/// Writes every table for `[from, to]`; the manifest is the caller's (once
/// after all ranges of a load).
pub async fn write_tables(
    store: &Store,
    uid: &str,
    t: &Tables,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<()> {
    replace_range(store, uid, &t.heart_rate, from, to).await?;
    replace_range(store, uid, &t.activity, from, to).await?;
    replace_range(store, uid, &t.spo2, from, to).await?;
    replace_range(store, uid, &t.hrv, from, to).await?;
    replace_range(store, uid, &t.nights, from, to).await?;
    replace_range(store, uid, &t.exercises, from, to).await?;
    replace_range(store, uid, &t.daily, from, to).await
}

/// Every data file of a user with its generation, so readers can tell what
/// changed with one read.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// File name (see `file_name`) -> generation.
    pub files: BTreeMap<String, i64>,
    /// The time zone the local dates in the files follow, and the one
    /// queries show times in (IANA name).
    #[serde(default = "default_tz")]
    pub time_zone: String,
}

fn default_tz() -> String {
    crate::users::DEFAULT_TIME_ZONE.to_string()
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            time_zone: default_tz(),
        }
    }
}

impl Manifest {
    pub fn tz(&self) -> chrono_tz::Tz {
        self.time_zone.parse().unwrap_or(chrono_tz::Europe::Prague)
    }
}

pub fn manifest_path(uid: &str) -> String {
    format!("{}/manifest.json", data_prefix(uid))
}

pub async fn write_manifest(store: &Store, uid: &str, tz: chrono_tz::Tz) -> Result<()> {
    let prefix = format!("{}/", data_prefix(uid));
    let files = store
        .list(&prefix)
        .await?
        .into_iter()
        .filter_map(|(name, generation)| {
            let name = name.strip_prefix(&prefix)?.to_string();
            name.ends_with(".parquet").then_some((name, generation))
        })
        .collect();
    let manifest = Manifest {
        files,
        time_zone: tz.name().to_string(),
    };
    store
        .put(&manifest_path(uid), serde_json::to_vec_pretty(&manifest)?)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::StageType;
    use crate::model::{DailyMetrics, HeartRateRow, NightRow, SleepStage, prague};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn hr(ts: &str, bpm: f64) -> HeartRateRow {
        let ts = prague(ts);
        HeartRateRow {
            date: ts.date_naive(),
            ts,
            bpm,
        }
    }

    async fn read(store: &Store, path: &str) -> Vec<(String, f64)> {
        let file = store.local_dir().unwrap().join("u/x/data").join(path);
        if !file.exists() {
            return vec![];
        }
        let ctx = engine::session(64 << 20).unwrap();
        engine::register(&ctx, "heart_rate", &[file]).await.unwrap();
        engine::rows(&ctx, "SELECT ts, bpm FROM heart_rate ORDER BY ts")
            .await
            .unwrap()
            .into_iter()
            .map(|r| {
                (
                    r["ts"].as_str().unwrap().to_string(),
                    r["bpm"].as_f64().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn lists_months_of_a_range() {
        assert_eq!(
            months(d("2026-09-28"), d("2026-11-02")),
            ["2026-09", "2026-10", "2026-11"]
        );
        assert_eq!(months(d("2026-10-05"), d("2026-10-05")), ["2026-10"]);
    }

    #[tokio::test]
    async fn replaces_only_the_range_and_splits_by_month() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let first = [
            hr("2026-09-30 10:00", 60.0),
            hr("2026-10-01 10:00", 61.0),
            hr("2026-10-02 10:00", 62.0),
            hr("2026-10-03 10:00", 63.0),
        ];
        replace_range(&store, "x", &first, d("2026-09-30"), d("2026-10-03"))
            .await
            .unwrap();
        assert_eq!(
            read(&store, "heart_rate/2026-09.parquet").await,
            [("2026-09-30T08:00:00Z".into(), 60.0)]
        );
        assert_eq!(read(&store, "heart_rate/2026-10.parquet").await.len(), 3);

        // Re-load the middle: Oct 2 changes, Oct 3 is kept, Oct 1 lost its data.
        let second = [hr("2026-10-02 11:00", 72.0)];
        replace_range(&store, "x", &second, d("2026-10-01"), d("2026-10-02"))
            .await
            .unwrap();
        assert_eq!(
            read(&store, "heart_rate/2026-10.parquet").await,
            [
                ("2026-10-02T09:00:00Z".into(), 72.0),
                ("2026-10-03T08:00:00Z".into(), 63.0)
            ]
        );
        assert_eq!(read(&store, "heart_rate/2026-09.parquet").await.len(), 1);

        // A file left without rows is removed.
        replace_range::<HeartRateRow>(&store, "x", &[], d("2026-09-01"), d("2026-09-30"))
            .await
            .unwrap();
        assert!(read(&store, "heart_rate/2026-09.parquet").await.is_empty());
        assert!(
            !tmp.path()
                .join("u/x/data/heart_rate/2026-09.parquet")
                .exists()
        );

        write_manifest(&store, "x", chrono_tz::Europe::Prague)
            .await
            .unwrap();
        let m: Manifest =
            serde_json::from_slice(&store.get(&manifest_path("x")).await.unwrap().unwrap())
                .unwrap();
        assert_eq!(
            m.files.keys().collect::<Vec<_>>(),
            ["heart_rate/2026-10.parquet"]
        );
    }

    #[tokio::test]
    async fn small_tables_are_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let day = |s: &str, steps| DailyMetrics {
            date: d(s),
            steps: Some(steps),
            ..Default::default()
        };
        replace_range(
            &store,
            "x",
            &[day("2026-09-30", 1.0), day("2026-10-01", 2.0)],
            d("2026-09-30"),
            d("2026-10-01"),
        )
        .await
        .unwrap();
        replace_range(
            &store,
            "x",
            &[day("2026-10-01", 3.0)],
            d("2026-10-01"),
            d("2026-10-01"),
        )
        .await
        .unwrap();
        let file = tmp.path().join("u/x/data/daily.parquet");
        let ctx = engine::session(64 << 20).unwrap();
        engine::register(&ctx, "daily", &[file]).await.unwrap();
        let rows: Vec<DailyMetrics> = engine::query_as(&ctx, "SELECT * FROM daily ORDER BY date")
            .await
            .unwrap();
        let got: Vec<_> = rows.iter().map(|r| (r.date, r.steps)).collect();
        assert_eq!(
            got,
            [(d("2026-09-30"), Some(1.0)), (d("2026-10-01"), Some(3.0))]
        );
    }

    #[tokio::test]
    async fn nested_columns_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().to_str().unwrap()).await.unwrap();
        let t = prague;
        let night = NightRow {
            date: d("2026-10-02"),
            start: t("2026-10-01 23:00"),
            end: t("2026-10-02 07:00"),
            minutes_asleep: Some(450),
            minutes_awake: None,
            deep_minutes: None,
            light_minutes: None,
            rem_minutes: None,
            awake_minutes: None,
            restless_minutes: Some(0),
            stages: vec![SleepStage {
                start: t("2026-10-01 23:10"),
                end: t("2026-10-02 00:40"),
                stage: StageType::Deep,
            }],
            restless: vec![],
        };
        replace_range(&store, "x", &[night], d("2026-10-02"), d("2026-10-02"))
            .await
            .unwrap();
        let ctx = engine::session(64 << 20).unwrap();
        let file = tmp.path().join("u/x/data/nights.parquet");
        engine::register(&ctx, "nights", &[file]).await.unwrap();
        let rows: Vec<NightRow> = engine::query_as(&ctx, "SELECT * FROM nights")
            .await
            .unwrap();
        assert_eq!(rows[0].stages[0].stage, StageType::Deep);
        assert_eq!(rows[0].stages[0].end, t("2026-10-02 00:40"));
        assert_eq!(rows[0].minutes_asleep, Some(450));
        assert!(rows[0].restless.is_empty());
    }
}
