//! MCP server (Streamable HTTP, stateless, JSON responses only) with
//! read-only tools over the BigQuery tables, for Claude and ChatGPT
//! connectors. Clients authenticate with access tokens from `authz.rs`.

use anyhow::{Context, Result, bail};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use chrono::{Duration, NaiveDate};
use serde_json::{Map, Value, json};

use crate::authz::{bearer_email, unauthorized};
use crate::web::{Config, Shared, day_detail};

const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Longest range the list tools return at once.
const MAX_DAYS: i64 = 400;
const QUERY_MAX_BYTES: i64 = 1 << 30;
const QUERY_MAX_ROWS: usize = 1000;

const INSTRUCTIONS: &str = "Personal health data from the user's Fitbit, synced every hour into \
BigQuery. Dates and times are local (Europe/Prague). A night belongs to the date the user woke up. \
Start with get_daily_metrics for trends and get_day for one day in detail; for anything else read \
get_schema and use query.";

/// The streamable HTTP transport's optional GET stream is not offered.
pub async fn no_stream() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

pub async fn handle(State(cfg): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !cfg.auth_disabled && bearer_email(&cfg, &headers).is_none() {
        return unauthorized(&cfg);
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return Json(error(Value::Null, -32700, &e.to_string())).into_response(),
    };
    let reply = match msg {
        Value::Array(batch) => {
            let mut out = Vec::new();
            for m in batch {
                out.extend(dispatch(&cfg, m).await);
            }
            (!out.is_empty()).then_some(Value::Array(out))
        }
        m => dispatch(&cfg, m).await,
    };
    match reply {
        Some(r) => Json(r).into_response(),
        // Notifications and responses from the client need no answer.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn dispatch(cfg: &Config, msg: Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let Some(method) = msg["method"].as_str() else {
        return Some(error(id, -32600, "not a request"));
    };
    let params = &msg["params"];
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[1]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "fitdata", "title": "Fitbit data",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(cfg, name, &args).await {
                Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }] }),
                // Tool errors go to the model, which can fix its arguments.
                Err(e) => {
                    tracing::warn!("MCP tool {name} failed: {e:#}");
                    json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true })
                }
            }
        }
        _ => return Some(error(id, -32601, &format!("unknown method {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn tools() -> Value {
    let range = json!({
        "type": "object",
        "properties": {
            "from": { "type": "string", "format": "date", "description": "First day, YYYY-MM-DD. Default: 30 days before `to`." },
            "to": { "type": "string", "format": "date", "description": "Last day, YYYY-MM-DD. Default: today." },
        },
    });
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    let tool = |name: &str, title: &str, description: &str, schema: &Value| {
        json!({
            "name": name, "title": title, "description": description,
            "inputSchema": schema, "annotations": read_only,
        })
    };
    json!([
        tool(
            "get_daily_metrics",
            "Daily metrics",
            "One row per day: resting heart rate, HRV, SpO2, breathing rate, skin temperature, \
             steps, calories, active zone minutes, heart rate range, sleep duration and stages, \
             exercise minutes. Also when data was last synced. Missing values are left out.",
            &range,
        ),
        tool(
            "get_day",
            "Day detail",
            "One day in detail: daily metrics, the night that ended that morning (with stages), \
             exercises, heart rate in 15-minute buckets (min/avg/max), steps per 15 minutes, \
             and SpO2 and HRV readings during sleep.",
            &json!({
                "type": "object",
                "properties": { "date": { "type": "string", "format": "date", "description": "YYYY-MM-DD" } },
                "required": ["date"],
            }),
        ),
        tool(
            "get_sleep",
            "Sleep",
            "Nights in a date range: start, end, minutes asleep and awake, deep/light/REM minutes \
             and the stage timeline. A night belongs to the date of waking up.",
            &range,
        ),
        tool(
            "get_exercises",
            "Exercises",
            "Recorded exercises in a date range: type, start, end, active minutes, average heart \
             rate, calories, active zone minutes.",
            &range,
        ),
        tool(
            "get_schema",
            "Table schema",
            "Tables and columns of the BigQuery dataset, for writing `query` SQL.",
            &json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "query",
            "SQL query",
            "Runs one read-only GoogleSQL SELECT over the dataset; refer to tables by bare name \
             (e.g. `daily`, `heart_rate`). Filter on `date` (the partition column) to keep scans \
             small. Returns at most 1000 rows; aggregate rather than fetch raw minute data.",
            &json!({
                "type": "object",
                "properties": { "sql": { "type": "string", "description": "A single SELECT statement" } },
                "required": ["sql"],
            }),
        ),
    ])
}

fn date_arg(args: &Value, name: &str) -> Result<Option<NaiveDate>> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .with_context(|| format!("`{name}` must be a string"))?;
            let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .with_context(|| format!("`{name}` must be YYYY-MM-DD, got {s:?}"))?;
            Ok(Some(d))
        }
    }
}

/// `from`/`to` arguments with their defaults, checked.
fn range(args: &Value) -> Result<(NaiveDate, NaiveDate)> {
    let to = date_arg(args, "to")?.unwrap_or_else(|| chrono::Local::now().date_naive());
    let from = date_arg(args, "from")?.unwrap_or(to - Duration::days(29));
    if from > to {
        bail!("`from` must not be after `to`");
    }
    if (to - from).num_days() >= MAX_DAYS {
        bail!("at most {MAX_DAYS} days at once; use `query` to aggregate longer periods");
    }
    Ok((from, to))
}

/// Drops null fields, which are most of a sparse row, and rounds averages
/// like 70.33333333333333 to two decimals.
fn compact(rows: Vec<Value>) -> Vec<Value> {
    fn round(v: Value) -> Value {
        match v {
            Value::Number(n) if n.is_f64() => {
                let x = n.as_f64().unwrap_or_default();
                json!((x * 100.0).round() / 100.0)
            }
            Value::Array(a) => Value::Array(a.into_iter().map(round).collect()),
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| (k, round(v)))
                    .collect(),
            ),
            v => v,
        }
    }
    rows.into_iter().map(round).collect()
}

async fn call_tool(cfg: &Config, name: &str, args: &Value) -> Result<Value> {
    let bq = &cfg.bq;
    match name {
        "get_daily_metrics" => {
            let (from, to) = range(args)?;
            let (days, runs) = tokio::try_join!(
                bq.query(format!(
                    "SELECT * FROM {} WHERE date BETWEEN '{from}' AND '{to}' ORDER BY date",
                    bq.table("daily")
                )),
                bq.query(format!(
                    "SELECT FORMAT_TIMESTAMP('%Y-%m-%dT%H:%M:%S', run_at, 'Europe/Prague') AS run_at, ok
                     FROM {} WHERE ok ORDER BY run_at DESC LIMIT 1",
                    bq.table("sync_runs")
                )),
            )?;
            Ok(json!({
                "lastSuccessfulSync": runs.first().map(|r| r["run_at"].clone()),
                "days": compact(days),
            }))
        }
        "get_day" => {
            let date = date_arg(args, "date")?.context("`date` is required")?;
            let mut d = day_detail(bq, date).await?;
            Ok(json!({
                "date": d["date"].take(),
                "metrics": compact(vec![d["metrics"].take()]).pop(),
                "sleep": compact(vec![d["sleep"].take()]).pop(),
                "exercises": compact(vec![d["exercises"].take()]).pop(),
                "heartRate15min": { "columns": ["start", "min", "avg", "max"], "rows": hr_buckets(&d["heartRate"]) },
                "steps15min": { "columns": ["start", "steps"], "rows": d["steps"].take() },
                "spo2": { "columns": ["time", "percent"], "rows": d["spo2"].take() },
                "hrv": { "columns": ["time", "rmssd_ms"], "rows": d["hrv"].take() },
            }))
        }
        "get_sleep" | "get_exercises" => {
            let (from, to) = range(args)?;
            let (table, order) = if name == "get_sleep" {
                ("nights", "date")
            } else {
                ("exercises", "start")
            };
            let rows = bq
                .query(format!(
                    "SELECT * FROM {} WHERE date BETWEEN '{from}' AND '{to}' ORDER BY {order}",
                    bq.table(table)
                ))
                .await?;
            Ok(json!({ table: compact(rows) }))
        }
        "get_schema" => {
            let rows = bq
                .query(format!(
                    "SELECT table_name, field_path AS column, data_type, description
                     FROM `{}.{}`.INFORMATION_SCHEMA.COLUMN_FIELD_PATHS
                     WHERE NOT STARTS_WITH(table_name, '_') ORDER BY table_name",
                    bq.project, bq.dataset
                ))
                .await?;
            let mut tables = Map::new();
            for r in compact(rows) {
                let t = r["table_name"].as_str().unwrap_or_default().to_string();
                let mut col = r;
                col.as_object_mut().map(|o| o.remove("table_name"));
                tables
                    .entry(t)
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .expect("array")
                    .push(col);
            }
            Ok(json!({
                "notes": "All tables are partitioned by `date` (local date). DATETIME columns are \
                          local wall-clock time (Europe/Prague). `daily` has one row per day; \
                          heart_rate has a reading every few seconds, so aggregate it.",
                "tables": tables,
            }))
        }
        "query" => {
            let sql = args["sql"].as_str().context("`sql` is required")?;
            let mut rows = bq
                .query_untrusted(sql, QUERY_MAX_BYTES, QUERY_MAX_ROWS)
                .await?;
            let truncated = rows.len() > QUERY_MAX_ROWS;
            rows.truncate(QUERY_MAX_ROWS);
            let mut out = json!({ "rows": compact(rows) });
            if truncated {
                out["note"] = json!(format!(
                    "Only the first {QUERY_MAX_ROWS} rows; aggregate to see everything."
                ));
            }
            Ok(out)
        }
        _ => bail!("unknown tool {name}"),
    }
}

/// Minute rows `[time, lo, avg, hi]` merged into 15-minute buckets.
fn hr_buckets(minutes: &Value) -> Vec<Value> {
    let mut out: Vec<(String, f64, f64, f64, f64)> = Vec::new();
    for row in minutes.as_array().into_iter().flatten() {
        let (Some(t), Some(lo), Some(avg), Some(hi)) = (
            row[0].as_str(),
            row[1].as_f64(),
            row[2].as_f64(),
            row[3].as_f64(),
        ) else {
            continue;
        };
        // "YYYY-MM-DDTHH:MM:SS" -> start of its quarter hour.
        let Some(minute) = t.get(14..16).and_then(|m| m.parse::<u32>().ok()) else {
            continue;
        };
        let key = format!("{}{:02}:00", &t[..14], minute / 15 * 15);
        match out.last_mut() {
            Some(b) if b.0 == key => {
                b.1 = b.1.min(lo);
                b.2 += avg;
                b.3 = b.3.max(hi);
                b.4 += 1.0;
            }
            _ => out.push((key, lo, avg, hi, 1.0)),
        }
    }
    out.into_iter()
        .map(|(t, lo, sum, hi, n)| json!([t, lo, (sum / n).round(), hi]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_heart_rate_by_quarter_hour() {
        let minutes = json!([
            ["2026-10-23T07:00:00", 60.0, 62.0, 65.0],
            ["2026-10-23T07:14:00", 58.0, 64.0, 70.0],
            ["2026-10-23T07:15:00", 80.0, 90.0, 100.0],
        ]);
        assert_eq!(
            hr_buckets(&minutes),
            vec![
                json!(["2026-10-23T07:00:00", 58.0, 63.0, 70.0]),
                json!(["2026-10-23T07:15:00", 80.0, 90.0, 100.0]),
            ]
        );
    }

    #[test]
    fn checks_date_ranges() {
        let r = range(&json!({ "from": "2026-01-01", "to": "2026-01-31" })).unwrap();
        assert_eq!(r.0.to_string(), "2026-01-01");
        let (from, to) = range(&json!({ "to": "2026-01-31" })).unwrap();
        assert_eq!((to - from).num_days(), 29);
        assert!(range(&json!({ "from": "2026-02-01", "to": "2026-01-31" })).is_err());
        assert!(range(&json!({ "from": "2024-01-01", "to": "2026-01-31" })).is_err());
        assert!(range(&json!({ "from": "1. 1. 2026" })).is_err());
    }

    #[test]
    fn compacts_rows() {
        assert_eq!(
            compact(vec![json!({ "a": 1, "b": null, "c": [70.33333333333333] })]),
            vec![json!({ "a": 1, "c": [70.33] })]
        );
    }
}
