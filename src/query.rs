//! SQL from MCP clients (the `query` tool).
//!
//! A query gets a session of its own that knows only the signed-in user's
//! tables (`warehouse::Snapshot`): DataFusion has no SQL that reaches files
//! or the network, and DDL, DML and `SET` are refused (`engine::sql_options`),
//! so other users' data and the server are out of reach. What is left is
//! load: queries run on a runtime of their own with two threads, at most two
//! at a time, with a memory limit and a timeout, so a heavy one slows other
//! queries but not the dashboard.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::engine;
use crate::warehouse::Snapshot;

pub const MAX_ROWS: usize = 1000;
/// Rows beyond this size are cut, so one answer stays readable for a model.
const MAX_RESULT_BYTES: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Query state (hash tables, sorts) per query; nothing spills to disk.
const MEMORY: usize = 256 << 20;
const CONCURRENT: usize = 2;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("query")
        .enable_all()
        .build()
        .expect("query runtime")
});
static SLOTS: Semaphore = Semaphore::const_new(CONCURRENT);

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub rows: Vec<Value>,
    /// More rows (or bytes) than returned.
    pub truncated: bool,
}

/// Runs one SELECT over the tables of `snapshot`.
pub async fn run(snapshot: Snapshot, sql: &str) -> Result<QueryResult> {
    let _slot = SLOTS.acquire().await?;
    let sql = sql.trim().trim_end_matches(';').to_string();
    let started = Instant::now();
    // Dropping the timed-out future cancels the query's tasks.
    let result = RUNTIME
        .spawn(async move { tokio::time::timeout(TIMEOUT, execute(snapshot, &sql)).await })
        .await?;
    tracing::info!(
        ms = started.elapsed().as_millis() as u64,
        ok = matches!(result, Ok(Ok(_))),
        "MCP query"
    );
    match result {
        Ok(r) => r,
        Err(_) => bail!("the query took longer than {}s", TIMEOUT.as_secs()),
    }
}

async fn execute(snapshot: Snapshot, sql: &str) -> Result<QueryResult> {
    let ctx = snapshot.session(MEMORY).await?;
    // The subquery also keeps anything but a query out (on top of the
    // session's own checks) and bounds what is collected.
    let wrapped = format!("SELECT * FROM (\n{sql}\n) LIMIT {}", MAX_ROWS + 1);
    let mut rows = engine::rows(&ctx, &wrapped).await?;
    let mut truncated = rows.len() > MAX_ROWS;
    rows.truncate(MAX_ROWS);
    let mut size = 0;
    if let Some(cut) = rows.iter().position(|r| {
        size += r.to_string().len();
        size > MAX_RESULT_BYTES
    }) {
        rows.truncate(cut);
        truncated = true;
    }
    Ok(QueryResult { rows, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DailyMetrics, HeartRateRow};
    use crate::store::Store;
    use crate::tables;
    use chrono::{Duration as Days, NaiveDate, NaiveDateTime};

    /// A user's files: heart rate every minute over three days, one day of
    /// daily metrics.
    async fn snapshot(dir: &std::path::Path) -> Snapshot {
        let store = Store::open(dir.to_str().unwrap()).await.unwrap();
        let start = NaiveDateTime::parse_from_str("2026-10-01 00:00", "%Y-%m-%d %H:%M").unwrap();
        let hr: Vec<HeartRateRow> = (0..3 * 1440)
            .map(|i| {
                let ts = start + Days::minutes(i);
                HeartRateRow {
                    date: ts.date(),
                    ts,
                    bpm: 60.0 + (i % 20) as f64,
                }
            })
            .collect();
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        tables::replace_range(&store, "x", &hr, d("2026-10-01"), d("2026-10-03"))
            .await
            .unwrap();
        let daily = DailyMetrics {
            date: d("2026-10-01"),
            resting_hr: Some(55.0),
            ..Default::default()
        };
        tables::replace_range(&store, "x", &[daily], d("2026-10-01"), d("2026-10-01"))
            .await
            .unwrap();
        let data = dir.join("u/x/data");
        Snapshot::from_files([
            ("heart_rate", vec![data.join("heart_rate/2026-10.parquet")]),
            ("daily", vec![data.join("daily.parquet")]),
        ])
    }

    async fn query(sql: &str) -> Result<QueryResult> {
        let dir = tempfile::tempdir().unwrap();
        run(snapshot(dir.path()).await, sql).await
    }

    #[tokio::test]
    async fn answers_selects_over_the_tables() {
        let r = query("SELECT date, count(*) AS n FROM heart_rate GROUP BY date ORDER BY date")
            .await
            .unwrap();
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.rows[0]["date"], "2026-10-01");
        assert_eq!(r.rows[0]["n"], 1440);
        // Tables without files exist, empty.
        let r = query("SELECT count(*) AS n FROM spo2").await.unwrap();
        assert_eq!(r.rows[0]["n"], 0);
        let r = query("SELECT resting_hr, steps FROM daily").await.unwrap();
        assert_eq!(r.rows[0]["resting_hr"], 55.0);
        assert!(r.rows[0].get("steps").is_none());
        // CTEs and a trailing semicolon are fine.
        let r = query("WITH x AS (SELECT max(bpm) AS m FROM heart_rate) SELECT m FROM x;")
            .await
            .unwrap();
        assert_eq!(r.rows[0]["m"], 79.0);
    }

    #[tokio::test]
    async fn cuts_long_results() {
        let r = query("SELECT * FROM heart_rate").await.unwrap();
        assert_eq!(r.rows.len(), MAX_ROWS);
        assert!(r.truncated);
    }

    /// Everything here must fail: a query sees the registered tables and
    /// nothing else.
    #[tokio::test]
    async fn refuses_to_reach_outside_the_tables() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("u/x/data/heart_rate/2026-10.parquet");
        let attempts = [
            "SELECT * FROM '/etc/passwd'".to_string(),
            "SELECT * FROM 'file:///etc/hostname'".to_string(),
            format!("SELECT * FROM '{}'", file.display()),
            "SELECT * FROM 'http://metadata.google.internal/computeMetadata/v1/'".to_string(),
            "SELECT * FROM read_csv('/etc/passwd')".to_string(),
            "SELECT * FROM read_parquet('/etc/passwd')".to_string(),
            "CREATE EXTERNAL TABLE x STORED AS CSV LOCATION '/etc/passwd'".to_string(),
            "COPY (SELECT 1) TO '/tmp/fitdata.csv'".to_string(),
            "COPY heart_rate TO '/tmp/fitdata.parquet'".to_string(),
            "INSERT INTO daily SELECT * FROM daily".to_string(),
            "DROP TABLE daily".to_string(),
            "CREATE TABLE y AS SELECT 1".to_string(),
            "CREATE VIEW v AS SELECT 1".to_string(),
            "SET datafusion.execution.batch_size = 1".to_string(),
            "SELECT 1; SELECT 2".to_string(),
            "SELECT 1); DROP TABLE daily; SELECT (1".to_string(),
            "SELECT * FROM information_schema.df_settings".to_string(),
            "EXPLAIN ANALYZE SELECT 1".to_string(),
            "PREPARE p AS SELECT 1".to_string(),
        ];
        let snap = snapshot(dir.path()).await;
        for sql in &attempts {
            let r = run(snap.clone(), sql).await;
            assert!(r.is_err(), "should fail: {sql}");
        }
        assert!(!std::path::Path::new("/tmp/fitdata.csv").exists());
    }

    #[tokio::test]
    async fn stops_queries_over_the_memory_limit() {
        let r = query(
            "SELECT count(DISTINCT a.value * 100000 + b.value) AS n
             FROM generate_series(1, 3000) a, generate_series(1, 100000) b",
        )
        .await;
        let e = format!("{:#}", r.unwrap_err());
        assert!(e.contains("esources exhausted"), "{e}");
    }
}
