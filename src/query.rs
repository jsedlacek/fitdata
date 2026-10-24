//! SQL from MCP clients (the `query` tool) runs here, in its own Cloud Run
//! service (`fitdata query-serve`) whose service account has no roles.
//!
//! The web sends the SQL together with the Parquet files of the one user who
//! asked, so whatever a query manages to do, it can only ever see that
//! user's data: the service holds no keys, no other data and no
//! permissions, and handles one request at a time. Locking DuckDB down
//! (data loaded into memory first, then no file or network access, no
//! extensions, configuration locked) is a second layer on top.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{DefaultBodyLimit, Multipart};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};

use crate::duck;
#[cfg(test)]
use crate::duck::literal;
use crate::warehouse::Snapshot;

pub const MAX_ROWS: usize = 1000;
/// Rows beyond this size are cut, so one answer stays readable for a model.
const MAX_RESULT_BYTES: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(30);
const MEMORY_LIMIT: &str = "768MB";

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub rows: Vec<Value>,
    /// More rows (or bytes) than returned.
    pub truncated: bool,
}

// ------------------------------------------------------------------ server

pub async fn serve(port: u16) -> Result<()> {
    // Cloud Run sends one request at a time (concurrency 1); this keeps it so
    // when run elsewhere.
    let one = Arc::new(Semaphore::new(1));
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/query",
            post(move |m: Multipart| {
                let one = one.clone();
                async move {
                    let _permit = one.acquire().await;
                    match handle(m).await {
                        Ok(r) => Json(r).into_response(),
                        Err(e) => (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("{e:#}") })),
                        )
                            .into_response(),
                    }
                }
            }),
        )
        .layer(DefaultBodyLimit::max(512 << 20));
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("query service listening on 0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle(mut m: Multipart) -> Result<QueryResult> {
    let dir = tempfile::tempdir()?;
    let mut sql = None;
    let mut files: Vec<(&'static str, PathBuf)> = Vec::new();
    while let Some(field) = m.next_field().await? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "sql" {
            sql = Some(field.text().await?);
            continue;
        }
        // Parts named by table; the client's file names are not used.
        let Some(table) = duck::TABLES.iter().find(|t| **t == name) else {
            bail!("unknown part {name:?}");
        };
        let path = dir.path().join(format!("{table}-{}.parquet", files.len()));
        std::fs::write(&path, field.bytes().await?)?;
        files.push((table, path));
    }
    let sql = sql.context("missing sql")?;
    let started = Instant::now();
    let result = tokio::task::spawn_blocking(move || run(&sql, &files)).await?;
    drop(dir);
    tracing::info!(
        ms = started.elapsed().as_millis() as u64,
        ok = result.is_ok(),
        "query"
    );
    result
}

/// Checks with DuckDB's own parser that `sql` is exactly one SELECT.
/// Wrapping it in a subquery is not enough: `prepare` runs every statement
/// before the last, so `1); CREATE ...; SELECT (1` would get through.
fn single_select(conn: &duckdb::Connection, sql: &str) -> Result<()> {
    let parsed: String =
        conn.query_row("SELECT json_serialize_sql(?::VARCHAR)", [sql], |r| r.get(0))?;
    let parsed: Value = serde_json::from_str(&parsed)?;
    if parsed["error"].as_bool() == Some(true) {
        bail!(
            "only a single SELECT is allowed: {}",
            parsed["error_message"].as_str().unwrap_or("not a SELECT")
        );
    }
    if parsed["statements"].as_array().map_or(0, Vec::len) != 1 {
        bail!("only a single SELECT statement is allowed");
    }
    Ok(())
}

/// Loads the files into in-memory tables, locks DuckDB down and runs `sql`.
fn run(sql: &str, files: &[(&'static str, PathBuf)]) -> Result<QueryResult> {
    let conn = duck::open()?;
    conn.execute_batch(&format!(
        "SET memory_limit = '{MEMORY_LIMIT}'; SET threads = 1; SET temp_directory = '';"
    ))?;
    for table in duck::TABLES {
        let paths: Vec<String> = files
            .iter()
            .filter(|(t, _)| t == table)
            .map(|(_, p)| p.display().to_string())
            .collect();
        let select = duck::select_table(&conn, table, &paths)?;
        conn.execute_batch(&format!("CREATE TABLE {table} AS {select};"))?;
    }
    // From here on nothing outside the loaded tables is reachable.
    conn.execute_batch(
        "SET enable_external_access = false;
         SET allow_community_extensions = false;
         SET lock_configuration = true;",
    )?;
    let sql = sql.trim().trim_end_matches(';');
    single_select(&conn, sql)?;
    let wrapped = format!("SELECT * FROM (\n{sql}\n) LIMIT {}", MAX_ROWS + 1);

    // Interrupt the query once it runs too long.
    let interrupt = conn.interrupt_handle();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(TIMEOUT).is_err() {
            interrupt.interrupt();
        }
    });
    let rows = duck::rows(&conn, &wrapped);
    let _ = done.send(());
    let _ = watchdog.join();
    let mut rows = rows.map_err(|e| {
        let msg = format!("{e:#}");
        if msg.contains("INTERRUPT") {
            anyhow::anyhow!("query took longer than {}s", TIMEOUT.as_secs())
        } else {
            e
        }
    })?;

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

// ------------------------------------------------------------------ client

/// The web's side: sends a query and the user's files to the service.
pub struct QueryClient {
    http: reqwest::Client,
    url: String,
    token: Mutex<Option<(String, Instant)>>,
}

/// ID tokens from the metadata server live an hour.
const TOKEN_TTL: Duration = Duration::from_secs(45 * 60);

impl QueryClient {
    pub fn new(http: reqwest::Client, url: &str) -> Self {
        Self {
            http,
            url: url.trim_end_matches('/').to_string(),
            token: Mutex::new(None),
        }
    }

    /// An ID token for the service from the metadata server (Cloud Run
    /// checks that the caller may invoke it). Locally the service runs
    /// without that check.
    async fn id_token(&self) -> Result<Option<String>> {
        if self.url.starts_with("http://") {
            return Ok(None);
        }
        let mut cached = self.token.lock().await;
        if let Some((t, at)) = cached.as_ref()
            && at.elapsed() < TOKEN_TTL
        {
            return Ok(Some(t.clone()));
        }
        let t = self
            .http
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity")
            .query(&[("audience", self.url.as_str())])
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .context("ID token for the query service")?
            .error_for_status()?
            .text()
            .await?;
        *cached = Some((t.clone(), Instant::now()));
        Ok(Some(t))
    }

    #[tracing::instrument(name = "query service", skip_all, fields(otel.kind = "client", db.query.text = %sql))]
    pub async fn run(&self, sql: &str, snapshot: &Snapshot) -> Result<QueryResult> {
        let mut form = reqwest::multipart::Form::new().text("sql", sql.to_string());
        for (table, path) in snapshot.files() {
            let bytes = tokio::fs::read(path).await?;
            form = form.part(
                table,
                reqwest::multipart::Part::bytes(bytes).file_name("data.parquet"),
            );
        }
        let mut req = self
            .http
            .post(format!("{}/query", self.url))
            .multipart(form);
        if let Some(t) = self.id_token().await? {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.context("query service unreachable")?;
        let status = resp.status();
        if status == StatusCode::BAD_REQUEST {
            let body: Value = resp.json().await.unwrap_or_default();
            bail!("{}", body["error"].as_str().unwrap_or("query failed"));
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
            bail!("the query service is busy; try again in a moment");
        }
        if !status.is_success() {
            bail!("query service: HTTP {status}");
        }
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few rows of every table, as files the service would receive.
    fn files(dir: &std::path::Path) -> Vec<(&'static str, PathBuf)> {
        let conn = duck::open().unwrap();
        let hr = dir.join("hr.parquet");
        let daily = dir.join("daily.parquet");
        conn.execute_batch(&format!(
            "COPY (SELECT DATE '2026-10-01' + (i % 3)::INT AS date,
                          TIMESTAMP '2026-10-01 00:00:00' + to_seconds(i * 60) AS ts,
                          60.0 + i % 20 AS bpm FROM range(5000) t(i))
               TO {} (FORMAT parquet);
             COPY (SELECT DATE '2026-10-01' AS date, 55.0 AS resting_hr) TO {} (FORMAT parquet);",
            literal(&hr.display().to_string()),
            literal(&daily.display().to_string())
        ))
        .unwrap();
        vec![("heart_rate", hr), ("daily", daily)]
    }

    fn query(sql: &str) -> Result<QueryResult> {
        let dir = tempfile::tempdir().unwrap();
        run(sql, &files(dir.path()))
    }

    #[test]
    fn answers_selects_over_the_loaded_tables() {
        let r = query("SELECT date, count(*) AS n FROM heart_rate GROUP BY date ORDER BY date")
            .unwrap();
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.rows[0]["date"], "2026-10-01");
        // Tables without files exist, empty.
        let r = query("SELECT count(*) AS n FROM spo2").unwrap();
        assert_eq!(r.rows[0]["n"], 0);
        // Columns missing from older files come back NULL.
        let r = query("SELECT resting_hr, steps FROM daily").unwrap();
        assert_eq!(r.rows[0]["resting_hr"], 55.0);
        assert!(r.rows[0]["steps"].is_null());
        // CTEs and a trailing semicolon are fine.
        let r = query("WITH x AS (SELECT max(bpm) AS m FROM heart_rate) SELECT m FROM x;").unwrap();
        assert_eq!(r.rows[0]["m"], 79.0);
    }

    #[test]
    fn cuts_long_results() {
        let r = query("SELECT * FROM heart_rate").unwrap();
        assert_eq!(r.rows.len(), MAX_ROWS);
        assert!(r.truncated);
    }

    /// Everything here must fail: the service is the boundary, but DuckDB
    /// itself must not reach outside the loaded tables either.
    #[test]
    fn refuses_to_reach_outside_the_loaded_tables() {
        let dir = tempfile::tempdir().unwrap();
        let fs = files(dir.path());
        let loaded = fs[0].1.display().to_string();
        let attempts = [
            "SELECT * FROM read_csv('/etc/passwd')".to_string(),
            "SELECT * FROM read_text('/etc/hostname')".to_string(),
            "SELECT * FROM read_blob('/proc/self/environ')".to_string(),
            "SELECT * FROM glob('/**')".to_string(),
            format!("SELECT * FROM read_parquet('{loaded}')"),
            "SELECT * FROM 'http://metadata.google.internal/computeMetadata/v1/'".to_string(),
            "SELECT getenv('HOME') AS h".to_string(),
            "SELECT current_setting('enable_external_access') AS x FROM (SELECT 1); SET enable_external_access = true; SELECT (1".to_string(),
            "SELECT 1); COPY heart_rate TO '/tmp/x.csv'; SELECT (1".to_string(),
            "SELECT 1); ATTACH '/tmp/x.db' AS x; SELECT (1".to_string(),
            "SELECT 1); INSTALL httpfs; SELECT (1".to_string(),
            "SELECT 1); LOAD httpfs; SELECT (1".to_string(),
            "SELECT 1); SET lock_configuration = false; SELECT (1".to_string(),
            "SELECT 1); PRAGMA enable_external_access; SELECT (1".to_string(),
            "SELECT 1); CREATE TABLE y AS SELECT 1; SELECT (1".to_string(),
            "SELECT 1; SELECT 2".to_string(),
            "DROP TABLE heart_rate".to_string(),
        ];
        for sql in &attempts {
            assert!(run(sql, &fs).is_err(), "should fail: {sql}");
        }
    }

    #[test]
    fn stops_queries_over_the_memory_limit() {
        let err = query(
            "SELECT count(DISTINCT (a.i, b.i)) AS n FROM range(100000) a(i), range(100000) b(i)",
        );
        assert!(err.is_err());
    }
}
