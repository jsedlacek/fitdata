//! DuckDB: in-memory connections over the Parquet tables, the table schemas
//! (schema/) and
//! query results as JSON.
//!
//! DuckDB is synchronous; callers run it in `spawn_blocking`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate};
use duckdb::Connection;
use duckdb::types::{TimeUnit, Value as Duck};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};

/// A column of a table schema (BigQuery's JSON schema format).
#[derive(Debug, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub mode: String,
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<Column>,
}

macro_rules! schemas {
    ($($t:literal),*) => {
        [$(($t, include_str!(concat!("../schema/", $t, ".json")))),*]
    };
}

/// The normalized tables, in the order `get_schema` lists them.
pub const TABLES: &[&str] = &[
    "daily",
    "nights",
    "exercises",
    "heart_rate",
    "activity",
    "spo2",
    "hrv",
];

static SCHEMAS: LazyLock<BTreeMap<&'static str, Vec<Column>>> = LazyLock::new(|| {
    schemas!(
        "daily",
        "nights",
        "exercises",
        "heart_rate",
        "activity",
        "spo2",
        "hrv"
    )
    .into_iter()
    .map(|(t, json)| (t, serde_json::from_str(json).expect("valid schema JSON")))
    .collect()
});

pub fn schema(table: &str) -> &'static [Column] {
    SCHEMAS.get(table).map_or(&[], Vec::as_slice)
}

/// The DuckDB type of a schema column.
pub fn duck_type(c: &Column) -> String {
    let base = match c.kind.as_str() {
        "DATE" => "DATE".to_string(),
        "DATETIME" => "TIMESTAMP".to_string(),
        "TIMESTAMP" => "TIMESTAMPTZ".to_string(),
        "FLOAT" | "FLOAT64" => "DOUBLE".to_string(),
        "INTEGER" | "INT64" => "BIGINT".to_string(),
        "BOOLEAN" | "BOOL" => "BOOLEAN".to_string(),
        "RECORD" | "STRUCT" => format!(
            "STRUCT({})",
            c.fields
                .iter()
                .map(|f| format!("{} {}", ident(&f.name), duck_type(f)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "VARCHAR".to_string(),
    };
    if c.mode == "REPEATED" {
        format!("{base}[]")
    } else {
        base
    }
}

/// The `columns` argument of `read_json` for a table: `{'date': 'DATE', ...}`.
pub fn json_columns(table: &str) -> String {
    let cols: Vec<String> = schema(table)
        .iter()
        .map(|c| format!("{}: {}", literal(&c.name), literal(&duck_type(c))))
        .collect();
    format!("{{{}}}", cols.join(", "))
}

/// A quoted identifier (`end` is a keyword).
pub fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A quoted string literal.
pub fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A SELECT of `table`'s columns, in schema order, over Parquet `files`.
/// Columns that none of the files have (added to the schema later) come
/// back NULL; without files the result is empty but typed.
pub fn select_table(conn: &Connection, table: &str, files: &[String]) -> Result<String> {
    let schema = schema(table);
    if files.is_empty() {
        let typed: Vec<String> = schema
            .iter()
            .map(|c| format!("NULL::{} AS {}", duck_type(c), ident(&c.name)))
            .collect();
        return Ok(format!("SELECT {} WHERE false", typed.join(", ")));
    }
    let source = format!(
        "read_parquet([{}], union_by_name = true)",
        files
            .iter()
            .map(|f| literal(f))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let present: Vec<String> = rows(conn, &format!("DESCRIBE SELECT * FROM {source}"))?
        .into_iter()
        .filter_map(|r| r["column_name"].as_str().map(String::from))
        .collect();
    let cols: Vec<String> = schema
        .iter()
        .map(|c| {
            if present.contains(&c.name) {
                ident(&c.name)
            } else {
                format!("NULL::{} AS {}", duck_type(c), ident(&c.name))
            }
        })
        .collect();
    Ok(format!("SELECT {} FROM {source}", cols.join(", ")))
}

/// An in-memory database that only reads files it is pointed at: extensions
/// are linked in (Parquet, JSON) and never installed or loaded at runtime.
pub fn open() -> Result<Connection> {
    let conn = Connection::open_in_memory().context("DuckDB")?;
    conn.execute_batch(
        "SET autoinstall_known_extensions = false;
         SET autoload_known_extensions = false;
         SET threads = 2;",
    )?;
    Ok(conn)
}

/// Runs a query and returns its rows as JSON objects keyed by column name.
pub fn rows(conn: &Connection, sql: &str) -> Result<Vec<Value>> {
    let mut stmt = conn
        .prepare(sql)
        .with_context(|| format!("query failed: {}", first_line(sql)))?;
    let mut rows = stmt
        .query([])
        .with_context(|| format!("query failed: {}", first_line(sql)))?;
    let names = rows.as_ref().map(|s| s.column_names()).unwrap_or_default();
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = Map::new();
        for (i, name) in names.iter().enumerate() {
            obj.insert(name.clone(), to_json(row.get::<_, Duck>(i)?));
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// Runs a query and deserializes its rows (see `rows`).
pub fn query_as<T: DeserializeOwned>(conn: &Connection, sql: &str) -> Result<Vec<T>> {
    rows(conn, sql)?
        .into_iter()
        .map(|r| {
            serde_json::from_value(r)
                .with_context(|| format!("unexpected row from query: {}", first_line(sql)))
        })
        .collect()
}

fn first_line(sql: &str) -> &str {
    sql.trim().lines().next().unwrap_or_default()
}

fn micros(unit: TimeUnit, v: i64) -> i64 {
    unit.to_micros(v)
}

fn number(v: f64) -> Value {
    Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// A DuckDB value as JSON, with dates and times as the strings the model
/// types deserialize (`2026-10-22`, `2026-10-22T07:00:00`).
pub fn to_json(v: Duck) -> Value {
    match v {
        Duck::Null => Value::Null,
        Duck::Boolean(b) => Value::Bool(b),
        Duck::TinyInt(i) => i.into(),
        Duck::SmallInt(i) => i.into(),
        Duck::Int(i) => i.into(),
        Duck::BigInt(i) => i.into(),
        Duck::UTinyInt(i) => i.into(),
        Duck::USmallInt(i) => i.into(),
        Duck::UInt(i) => i.into(),
        Duck::UBigInt(i) => i.into(),
        Duck::HugeInt(i) => i64::try_from(i).map_or_else(|_| i.to_string().into(), Value::from),
        Duck::UHugeInt(i) => u64::try_from(i).map_or_else(|_| i.to_string().into(), Value::from),
        Duck::Float(f) => number(f.into()),
        Duck::Double(f) => number(f),
        Duck::Decimal(d) => d.to_string().parse::<f64>().map_or(Value::Null, number),
        Duck::Timestamp(unit, t) => DateTime::from_timestamp_micros(micros(unit, t))
            .map_or(Value::Null, |t| {
                t.naive_utc()
                    .format("%Y-%m-%dT%H:%M:%S%.f")
                    .to_string()
                    .into()
            }),
        Duck::Date32(d) => NaiveDate::from_epoch_days(d)
            .map_or(Value::Null, |d| d.format("%Y-%m-%d").to_string().into()),
        Duck::Time64(unit, t) => {
            let us = micros(unit, t);
            let (s, frac) = (us.div_euclid(1_000_000), us.rem_euclid(1_000_000));
            let hms = format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
            if frac == 0 {
                hms.into()
            } else {
                format!("{hms}.{frac:06}").into()
            }
        }
        Duck::Interval {
            months,
            days,
            nanos,
        } => format!("{months} months {days} days {}s", nanos as f64 / 1e9).into(),
        Duck::Text(s) | Duck::Enum(s) => Value::String(s),
        Duck::Blob(b) | Duck::Geometry(b) => format!("<{} bytes>", b.len()).into(),
        Duck::List(items) | Duck::Array(items) => {
            Value::Array(items.into_iter().map(to_json).collect())
        }
        Duck::Struct(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), to_json(v.clone())))
                .collect(),
        ),
        Duck::Map(entries) => Value::Array(
            entries
                .iter()
                .map(|(k, v)| {
                    serde_json::json!({ "key": to_json(k.clone()), "value": to_json(v.clone()) })
                })
                .collect(),
        ),
        Duck::Union(v) => to_json(*v),
        other => format!("{other:?}").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_schema_types() {
        let nights: Vec<String> = schema("nights")
            .iter()
            .map(|c| format!("{} {}", c.name, duck_type(c)))
            .collect();
        assert!(nights.contains(&"date DATE".to_string()));
        assert!(nights.contains(&"start TIMESTAMP".to_string()));
        assert!(nights.contains(&"minutes_asleep BIGINT".to_string()));
        assert!(
            nights.contains(
                &"stages STRUCT(\"start\" TIMESTAMP, \"end\" TIMESTAMP, \"stage\" VARCHAR)[]"
                    .to_string()
            )
        );
        for t in TABLES {
            assert!(!schema(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn rows_come_back_as_model_json() {
        let conn = open().unwrap();
        let rows = rows(
            &conn,
            "SELECT DATE '2026-10-22' AS d, TIMESTAMP '2026-10-22 07:00:00' AS t,
                    TIMESTAMP '2026-10-22 07:00:00.5' AS f, 1.5::DOUBLE AS x, 2 AS n,
                    NULL AS z, 'a''b' AS s, 3.25::DECIMAL(5,2) AS dec,
                    [{'start': TIMESTAMP '2026-10-22 01:00:00', 'stage': 'DEEP'}] AS stages",
        )
        .unwrap();
        assert_eq!(
            rows[0],
            json!({
                "d": "2026-10-22", "t": "2026-10-22T07:00:00", "f": "2026-10-22T07:00:00.500",
                "x": 1.5, "n": 2, "z": null, "s": "a'b", "dec": 3.25,
                "stages": [{"start": "2026-10-22T01:00:00", "stage": "DEEP"}],
            })
        );
    }
}
