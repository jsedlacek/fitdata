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
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::duck::{self, ident, literal};
use crate::model::{Row, Tables};
use crate::store::{self, Store};

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
    let cols = duck::schema(table);
    let time = ["ts", "start"]
        .into_iter()
        .find(|t| cols.iter().any(|c| c.name == *t));
    match time {
        Some(t) => format!("date, {}", ident(t)),
        None => "date".to_string(),
    }
}

/// The SQL that writes one file: the old file without the dates in
/// `[from, to]`, plus the new rows, limited to `month` if the table is split.
fn merge_sql(
    table: &str,
    old: Option<&Path>,
    new: &Path,
    out: &Path,
    month: Option<&str>,
    (from, to): (NaiveDate, NaiveDate),
) -> String {
    let columns: Vec<String> = duck::schema(table).iter().map(|c| ident(&c.name)).collect();
    let new = format!(
        "SELECT * FROM read_json({}, format = 'newline_delimited', columns = {})",
        literal(&new.display().to_string()),
        duck::json_columns(table)
    );
    let source = match old {
        Some(old) => format!(
            "SELECT * FROM read_parquet({}) WHERE date NOT BETWEEN DATE '{from}' AND DATE '{to}'
             UNION ALL BY NAME {new}",
            literal(&old.display().to_string())
        ),
        None => new,
    };
    let filter = month.map_or(String::new(), |m| {
        format!("WHERE strftime(date, '%Y-%m') = {}", literal(m))
    });
    format!(
        "COPY (SELECT {cols} FROM ({source}) {filter} ORDER BY {key})
         TO {out} (FORMAT parquet, COMPRESSION zstd)",
        cols = columns.join(", "),
        key = sort_key(table),
        out = literal(&out.display().to_string()),
    )
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
    let new = dir.path().join("new.jsonl");
    std::fs::write(&new, store::to_jsonl(rows)?)?;
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
        let out = dir.path().join("out.parquet");
        let sql = merge_sql(
            table,
            old.as_deref(),
            &new,
            &out,
            month.as_deref(),
            (from, to),
        );
        let n = tokio::task::spawn_blocking(move || -> Result<usize> {
            Ok(duck::open()?.execute(&sql, [])?)
        })
        .await?
        .with_context(|| format!("writing {path}"))?;
        if n == 0 {
            store.delete(&path).await?;
        } else {
            store.put(&path, std::fs::read(&out)?).await?;
        }
        written += n;
    }
    tracing::info!(
        "wrote {from}..{to}: {} new rows, {written} in files",
        rows.len()
    );
    Ok(())
}

/// Writes every table for `[from, to]`, then the manifest.
pub async fn write_all(
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
    replace_range(store, uid, &t.daily, from, to).await?;
    write_manifest(store, uid).await
}

/// Every data file of a user with its generation, so readers can tell what
/// changed with one read.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// File name (see `file_name`) -> generation.
    pub files: BTreeMap<String, i64>,
}

pub fn manifest_path(uid: &str) -> String {
    format!("{}/manifest.json", data_prefix(uid))
}

pub async fn write_manifest(store: &Store, uid: &str) -> Result<()> {
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
    let manifest = Manifest { files };
    store
        .put(&manifest_path(uid), serde_json::to_vec_pretty(&manifest)?)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DailyMetrics, HeartRateRow};
    use chrono::NaiveDateTime;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn hr(ts: &str, bpm: f64) -> HeartRateRow {
        let ts = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M").unwrap();
        HeartRateRow {
            date: ts.date(),
            ts,
            bpm,
        }
    }

    fn read(store: &Store, path: &str) -> Vec<(String, f64)> {
        let Store::Local(dir) = store else {
            unreachable!()
        };
        let conn = duck::open().unwrap();
        let file = dir.join("u/x/data").join(path);
        if !file.exists() {
            return vec![];
        }
        duck::rows(
            &conn,
            &format!(
                "SELECT ts, bpm FROM read_parquet({}) ORDER BY ts",
                literal(&file.display().to_string())
            ),
        )
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
            read(&store, "heart_rate/2026-09.parquet"),
            [("2026-09-30T10:00:00".into(), 60.0)]
        );
        assert_eq!(read(&store, "heart_rate/2026-10.parquet").len(), 3);

        // Re-load the middle: Oct 2 changes, Oct 3 is kept, Oct 1 lost its data.
        let second = [hr("2026-10-02 11:00", 72.0)];
        replace_range(&store, "x", &second, d("2026-10-01"), d("2026-10-02"))
            .await
            .unwrap();
        assert_eq!(
            read(&store, "heart_rate/2026-10.parquet"),
            [
                ("2026-10-02T11:00:00".into(), 72.0),
                ("2026-10-03T10:00:00".into(), 63.0)
            ]
        );
        assert_eq!(read(&store, "heart_rate/2026-09.parquet").len(), 1);

        // A file left without rows is removed.
        replace_range::<HeartRateRow>(&store, "x", &[], d("2026-09-01"), d("2026-09-30"))
            .await
            .unwrap();
        assert!(read(&store, "heart_rate/2026-09.parquet").is_empty());
        assert!(
            !tmp.path()
                .join("u/x/data/heart_rate/2026-09.parquet")
                .exists()
        );

        write_manifest(&store, "x").await.unwrap();
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
        let conn = duck::open().unwrap();
        let file = tmp.path().join("u/x/data/daily.parquet");
        let rows: Vec<DailyMetrics> = duck::query_as(
            &conn,
            &format!(
                "SELECT * FROM read_parquet({}) ORDER BY date",
                literal(&file.display().to_string())
            ),
        )
        .unwrap();
        let got: Vec<_> = rows.iter().map(|r| (r.date, r.steps)).collect();
        assert_eq!(
            got,
            [(d("2026-09-30"), Some(1.0)), (d("2026-10-01"), Some(3.0))]
        );
    }
}
