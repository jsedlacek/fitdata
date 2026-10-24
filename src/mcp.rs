//! MCP server (Streamable HTTP, stateless, JSON responses only) with
//! read-only tools over the user's tables, for Claude and ChatGPT
//! connectors. Clients authenticate with access tokens from `authz.rs`.
//!
//! Tool arguments are serde types, so validation and its error messages (for
//! the model to fix its call) come from their definitions, and so do the
//! input schemas the tools are listed with.

use std::borrow::Cow;

use anyhow::{Result, bail};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use chrono::{Duration, NaiveDate, NaiveDateTime, Timelike};
use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::Instrument;

use crate::authz::{bearer_user, unauthorized};
use crate::duck;
use crate::model::{DailyMetrics, Exercise, NightRow};
use crate::warehouse::Need;
use crate::web::{Config, HrMinute, Shared, SignedIn, day_detail, last_run};

const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Longest range the list tools return at once.
const MAX_DAYS: i64 = 400;
/// Default and longest stretch of the minute tables a `query` can read.
const QUERY_DEFAULT_DAYS: i64 = 90;

const INSTRUCTIONS: &str = "Personal health data from the user's Fitbit, synced every hour. \
Dates and times are local (Europe/Prague). A night belongs to the date the user woke up. \
Start with get_daily_metrics for trends and get_day for one day in detail; for anything else read \
get_schema and use query.";

/// The streamable HTTP transport's optional GET stream is not offered.
pub async fn no_stream() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

pub async fn handle(State(cfg): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(user) = bearer_user(&cfg, &headers).await else {
        return unauthorized(&cfg);
    };
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return Json(Reply::error(Value::Null, -32700, e.to_string())).into_response(),
    };
    let reply = match msg {
        Value::Array(batch) => {
            let mut out = Vec::new();
            for m in batch {
                out.extend(dispatch(&cfg, &user, m).await);
            }
            (!out.is_empty()).then(|| json!(out))
        }
        m => dispatch(&cfg, &user, m).await.map(|r| json!(r)),
    };
    match reply {
        Some(r) => Json(r).into_response(),
        // Notifications and responses from the client need no answer.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

// --------------------------------------------------------------- JSON-RPC

/// A JSON-RPC message from the client. Without an `id` it is a
/// notification (or a response to us), which gets no reply.
#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Reply {
    jsonrpc: &'static str,
    id: Value,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Result(Value),
    Error { code: i64, message: String },
}

impl Reply {
    fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            outcome: Outcome::Result(result),
        }
    }

    fn error(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            outcome: Outcome::Error { code, message },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    #[serde(default)]
    protocol_version: String,
}

/// What `tools/call` returns: the tool's output as JSON text.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolResult {
    content: [TextContent; 1],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_error: bool,
}

#[derive(Serialize)]
struct TextContent {
    r#type: &'static str,
    text: String,
}

impl ToolResult {
    fn new(text: String, is_error: bool) -> Self {
        Self {
            content: [TextContent {
                r#type: "text",
                text,
            }],
            is_error,
        }
    }
}

async fn dispatch(cfg: &Config, user: &SignedIn, msg: Value) -> Option<Reply> {
    let Ok(msg) = serde_json::from_value::<Message>(msg) else {
        return Some(Reply::error(
            Value::Null,
            -32600,
            "not a JSON-RPC message".into(),
        ));
    };
    let id = msg.id?;
    let Some(method) = msg.method else {
        return Some(Reply::error(id, -32600, "not a request".into()));
    };
    let result = match method.as_str() {
        "initialize" => {
            let asked = serde_json::from_value::<InitializeParams>(msg.params)
                .map(|p| p.protocol_version)
                .unwrap_or_default();
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
        "tools/call" => json!(call(cfg, &user.uid, msg.params).await),
        _ => return Some(Reply::error(id, -32601, format!("unknown method {method}"))),
    };
    Some(Reply::result(id, result))
}

/// Runs a `tools/call`. Bad arguments and tool failures go to the model,
/// which can fix its call.
async fn call(cfg: &Config, uid: &str, mut params: Value) -> ToolResult {
    let name = params["name"].as_str().unwrap_or_default().to_string();
    if params.get("arguments").is_none_or(Value::is_null) {
        params["arguments"] = json!({});
    }
    let result = match serde_json::from_value::<ToolCall>(params) {
        Ok(tool) => {
            let span = tracing::info_span!("tool", otel.name = format!("tool {name}"));
            call_tool(cfg, uid, tool).instrument(span).await
        }
        Err(e) => Err(anyhow::anyhow!("invalid call of tool {name:?}: {e}")),
    };
    match result {
        Ok(v) => ToolResult::new(v.to_string(), false),
        Err(e) => {
            tracing::warn!("MCP tool {name} failed: {e:#}");
            ToolResult::new(format!("{e:#}"), true)
        }
    }
}

// ------------------------------------------------------------------ tools

#[derive(Deserialize)]
#[serde(tag = "name", content = "arguments", rename_all = "snake_case")]
enum ToolCall {
    GetDailyMetrics(RangeArgs),
    GetDay(DayArgs),
    GetSleep(RangeArgs),
    GetExercises(RangeArgs),
    GetSchema(NoArgs),
    Query(QueryArgs),
}

/// A date argument, `YYYY-MM-DD`.
#[derive(Clone, Copy)]
struct Date(NaiveDate);

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .map(Date)
            .map_err(|_| de::Error::custom(format!("expected a date YYYY-MM-DD, got {s:?}")))
    }
}

impl JsonSchema for Date {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Date".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "format": "date" })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RangeArgs {
    /// First day, YYYY-MM-DD. Default: 30 days before `to`.
    // Optional means "may be left out", so not nullable in the schema.
    #[serde(default)]
    #[schemars(with = "Date")]
    from: Option<Date>,
    /// Last day, YYYY-MM-DD. Default: today.
    #[serde(default)]
    #[schemars(with = "Date")]
    to: Option<Date>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DayArgs {
    /// YYYY-MM-DD
    date: Date,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    /// A single SELECT statement (DuckDB SQL)
    sql: String,
    /// First day of heart_rate, activity, spo2 and hrv to load, YYYY-MM-DD. Default: 90 days
    /// before `to`. The other tables are always complete.
    #[serde(default)]
    #[schemars(with = "Date")]
    from: Option<Date>,
    /// Last day of the minute tables to load, YYYY-MM-DD. Default: today.
    #[serde(default)]
    #[schemars(with = "Date")]
    to: Option<Date>,
}

impl QueryArgs {
    /// The days of the minute tables to load; checked like `RangeArgs`.
    fn range(&self) -> Result<(NaiveDate, NaiveDate)> {
        let to = self
            .to
            .map_or_else(|| chrono::Local::now().date_naive(), |d| d.0);
        let from = self
            .from
            .map_or(to - Duration::days(QUERY_DEFAULT_DAYS - 1), |d| d.0);
        RangeArgs {
            from: Some(Date(from)),
            to: Some(Date(to)),
        }
        .range()
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
    annotations: Value,
}

/// The input schema of a tool's arguments, without the `$schema` and `title`
/// that clients don't need.
fn input_schema<T: JsonSchema>() -> Value {
    let mut s = SchemaSettings::draft2020_12()
        .with(|s| s.meta_schema = None)
        .for_deserialize()
        .into_generator()
        .into_root_schema_for::<T>();
    s.remove("title");
    let mut v = s.to_value();
    if v.get("properties").is_none() {
        v["properties"] = json!({});
    }
    v
}

fn tools() -> Vec<Tool> {
    let tool = |name, title, description, input_schema| Tool {
        name,
        title,
        description,
        input_schema,
        annotations: json!({ "readOnlyHint": true, "openWorldHint": false }),
    };
    vec![
        tool(
            "get_daily_metrics",
            "Daily metrics",
            "One row per day: resting heart rate, HRV, SpO2, breathing rate, skin temperature, \
             steps, calories, active zone minutes, heart rate range, sleep duration and stages, \
             exercise minutes. Also when data was last synced. Missing values are left out.",
            input_schema::<RangeArgs>(),
        ),
        tool(
            "get_day",
            "Day detail",
            "One day in detail: daily metrics, the night that ended that morning (with stages), \
             exercises, heart rate in 15-minute buckets (min/avg/max), steps per 15 minutes, \
             and SpO2 and HRV readings during sleep.",
            input_schema::<DayArgs>(),
        ),
        tool(
            "get_sleep",
            "Sleep",
            "Nights in a date range: start, end, minutes asleep and awake, deep/light/REM minutes \
             and the stage timeline. A night belongs to the date of waking up.",
            input_schema::<RangeArgs>(),
        ),
        tool(
            "get_exercises",
            "Exercises",
            "Recorded exercises in a date range: type, start, end, active minutes, average heart \
             rate, calories, active zone minutes.",
            input_schema::<RangeArgs>(),
        ),
        tool(
            "get_schema",
            "Table schema",
            "Tables and columns, for writing `query` SQL.",
            input_schema::<NoArgs>(),
        ),
        tool(
            "query",
            "SQL query",
            "Runs one read-only SELECT in DuckDB SQL over the user's tables, by bare name (e.g. \
             `daily`, `heart_rate`). daily, nights and exercises are complete; heart_rate, \
             activity, spo2 and hrv hold only the days `from`..`to` (default: the last 90 days, \
             at most 400). Returns at most 1000 rows; aggregate rather than fetch raw readings.",
            input_schema::<QueryArgs>(),
        ),
    ]
}

impl RangeArgs {
    /// `from`/`to` with their defaults, checked.
    fn range(&self) -> Result<(NaiveDate, NaiveDate)> {
        let to = self
            .to
            .map_or_else(|| chrono::Local::now().date_naive(), |d| d.0);
        let from = self.from.map_or(to - Duration::days(29), |d| d.0);
        if from > to {
            bail!("`from` must not be after `to`");
        }
        if (to - from).num_days() >= MAX_DAYS {
            bail!("at most {MAX_DAYS} days at once; use `query` to aggregate longer periods");
        }
        Ok((from, to))
    }
}

/// Drops null fields, which are most of a sparse row, and rounds averages
/// like 70.33333333333333 to two decimals.
fn compact<T: Serialize>(value: T) -> Value {
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
    round(serde_json::to_value(value).unwrap_or_default())
}

async fn call_tool(cfg: &Config, uid: &str, tool: ToolCall) -> Result<Value> {
    let wh = &cfg.warehouse;
    let in_range = |table: &str, (from, to): (NaiveDate, NaiveDate), order: &str| {
        format!(
            "SELECT * FROM {table} WHERE date BETWEEN DATE '{from}' AND DATE '{to}' ORDER BY {order}"
        )
    };
    match tool {
        ToolCall::GetDailyMetrics(args) => {
            let sql = in_range("daily", args.range()?, "date");
            let days = wh.with(uid, Need::None, move |c| {
                duck::query_as::<DailyMetrics>(c, &sql)
            });
            let (days, run) = tokio::try_join!(days, last_run(&cfg.store, uid, true))?;
            Ok(json!({
                "lastSuccessfulSync": run.map(|r| r.run_at),
                "days": compact(days),
            }))
        }
        ToolCall::GetDay(DayArgs { date }) => {
            let d = day_detail(wh, uid, date.0).await?;
            Ok(json!({
                "date": d.date,
                "metrics": compact(d.metrics),
                "sleep": compact(d.sleep),
                "exercises": compact(d.exercises),
                "heartRate15min": { "columns": ["start", "min", "avg", "max"], "rows": hr_buckets(&d.heart_rate) },
                "steps15min": { "columns": ["start", "steps"], "rows": d.steps },
                "spo2": { "columns": ["time", "percent"], "rows": d.spo2 },
                "hrv": { "columns": ["time", "rmssd_ms"], "rows": d.hrv },
            }))
        }
        ToolCall::GetSleep(args) => {
            let sql = in_range("nights", args.range()?, "date");
            let rows = wh
                .with(uid, Need::None, move |c| {
                    duck::query_as::<NightRow>(c, &sql)
                })
                .await?;
            Ok(json!({ "nights": compact(rows) }))
        }
        ToolCall::GetExercises(args) => {
            let sql = in_range("exercises", args.range()?, "start");
            let rows = wh
                .with(uid, Need::None, move |c| {
                    duck::query_as::<Exercise>(c, &sql)
                })
                .await?;
            Ok(json!({ "exercises": compact(rows) }))
        }
        ToolCall::GetSchema(NoArgs {}) => Ok(schema()),
        ToolCall::Query(args) => {
            let (from, to) = args.range()?;
            let snapshot = wh.snapshot(uid, Need::Range(from, to)).await?;
            let result = cfg.query.run(&args.sql, &snapshot).await?;
            let mut out = json!({ "rows": compact(result.rows) });
            if result.truncated {
                out["note"] = json!("Only the first rows; aggregate to see everything.");
            }
            Ok(out)
        }
    }
}

/// `get_schema`: every table with its columns, types and descriptions.
fn schema() -> Value {
    let tables: Map<String, Value> = duck::TABLES
        .iter()
        .map(|t| {
            let cols: Vec<Value> = duck::schema(t)
                .iter()
                .map(|c| {
                    compact(json!({
                        "column": c.name,
                        "type": duck::duck_type(c),
                        "description": c.description,
                    }))
                })
                .collect();
            (t.to_string(), Value::Array(cols))
        })
        .collect();
    json!({
        "notes": "DuckDB SQL. Every table has `date` (local date); TIMESTAMP columns are local \
                  wall-clock time (Europe/Prague). `daily` has one row per day; heart_rate has a \
                  reading every few seconds, so aggregate it.",
        "tables": tables,
    })
}

/// Minute rows merged into 15-minute buckets: `(start, min, mean, max)`.
fn hr_buckets(minutes: &[HrMinute]) -> Vec<(NaiveDateTime, f64, f64, f64)> {
    let mut out: Vec<(NaiveDateTime, f64, f64, f64, f64)> = Vec::new();
    for m in minutes {
        let start =
            m.t.date()
                .and_hms_opt(m.t.hour(), m.t.minute() / 15 * 15, 0)
                .expect("a valid time");
        match out.last_mut() {
            Some(b) if b.0 == start => {
                b.1 = b.1.min(m.lo);
                b.2 += m.avg;
                b.3 = b.3.max(m.hi);
                b.4 += 1.0;
            }
            _ => out.push((start, m.lo, m.avg, m.hi, 1.0)),
        }
    }
    out.into_iter()
        .map(|(t, lo, sum, hi, n)| (t, lo, (sum / n).round(), hi))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    #[test]
    fn buckets_heart_rate_by_quarter_hour() {
        let m = |s, lo, avg, hi| HrMinute {
            t: t(s),
            lo,
            avg,
            hi,
        };
        let minutes = [
            m("2026-10-23T07:00:00", 60.0, 62.0, 65.0),
            m("2026-10-23T07:14:00", 58.0, 64.0, 70.0),
            m("2026-10-23T07:15:00", 80.0, 90.0, 100.0),
        ];
        assert_eq!(
            json!(hr_buckets(&minutes)),
            json!([
                ["2026-10-23T07:00:00", 58.0, 63.0, 70.0],
                ["2026-10-23T07:15:00", 80.0, 90.0, 100.0],
            ])
        );
    }

    fn parse(v: Value) -> Result<ToolCall, String> {
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    #[test]
    fn checks_tool_arguments() {
        let range = |args| match parse(json!({ "name": "get_sleep", "arguments": args })) {
            Ok(ToolCall::GetSleep(r)) => r.range().map_err(|e| e.to_string()),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        };
        let (from, to) = range(json!({ "from": "2026-01-01", "to": "2026-01-31" })).unwrap();
        assert_eq!(
            (from.to_string(), to.to_string()),
            ("2026-01-01".into(), "2026-01-31".into())
        );
        let (from, to) = range(json!({ "to": "2026-01-31" })).unwrap();
        assert_eq!((to - from).num_days(), 29);
        assert!(range(json!({ "from": "2026-02-01", "to": "2026-01-31" })).is_err());
        assert!(range(json!({ "from": "2024-01-01", "to": "2026-01-31" })).is_err());
        let bad = range(json!({ "from": "1. 1. 2026" })).unwrap_err();
        assert!(bad.contains("expected a date YYYY-MM-DD"), "{bad}");
        let unknown = range(json!({ "since": "2026-01-01" })).unwrap_err();
        assert!(unknown.contains("unknown field `since`"), "{unknown}");
        let missing = parse(json!({ "name": "get_day", "arguments": {} }))
            .err()
            .unwrap();
        assert!(missing.contains("missing field `date`"), "{missing}");
        let tool = parse(json!({ "name": "drop_tables", "arguments": {} }))
            .err()
            .unwrap();
        assert!(tool.contains("unknown variant `drop_tables`"), "{tool}");
    }

    #[test]
    fn every_listed_tool_can_be_called() {
        for t in tools() {
            let e = parse(json!({ "name": t.name, "arguments": {} })).err();
            assert!(
                e.as_deref().is_none_or(|e| !e.contains("unknown variant")),
                "{}: {e:?}",
                t.name
            );
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            assert!(t.input_schema["properties"].is_object(), "{}", t.name);
        }
    }

    #[test]
    fn generates_input_schemas() {
        assert_eq!(
            input_schema::<RangeArgs>()["properties"]["from"],
            json!({ "type": "string", "format": "date", "description": "First day, YYYY-MM-DD. Default: 30 days before `to`." })
        );
        assert!(input_schema::<RangeArgs>().get("required").is_none());
        assert_eq!(
            input_schema::<DayArgs>(),
            json!({
                "type": "object",
                "properties": { "date": { "type": "string", "format": "date", "description": "YYYY-MM-DD" } },
                "required": ["date"],
                "additionalProperties": false,
            })
        );
    }

    #[test]
    fn compacts_rows() {
        assert_eq!(
            compact(vec![json!({ "a": 1, "b": null, "c": [70.33333333333333] })]),
            json!([{ "a": 1, "c": [70.33] }])
        );
    }

    #[test]
    fn replies_are_json_rpc() {
        assert_eq!(
            json!(Reply::result(json!(1), json!({}))),
            json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
        );
        assert_eq!(
            json!(Reply::error(json!("a"), -32601, "x".into())),
            json!({ "jsonrpc": "2.0", "id": "a", "error": { "code": -32601, "message": "x" } })
        );
        assert_eq!(
            json!(ToolResult::new("t".into(), false)),
            json!({ "content": [{ "type": "text", "text": "t" }] })
        );
    }
}
