//! DataFusion over the users' Parquet tables: the table schemas (schema/,
//! in BigQuery's JSON schema format), sessions that know only the tables
//! registered on them, and query results as JSON.
//!
//! A session has no way to files or the network of its own: there is no
//! SQL function that reads a path, and DDL, DML and `SET` are refused
//! (`sql_options`). What a query can read is what was registered.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::{DataType, Field, Fields, Schema, SchemaRef, TimeUnit};
use datafusion::arrow::json::ArrayWriter;
use datafusion::datasource::MemTable;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::execution::SessionStateBuilder;
use datafusion::execution::disk_manager::{DiskManagerBuilder, DiskManagerMode};
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::prelude::{SQLOptions, SessionConfig, SessionContext};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// A column of a table schema.
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

static COLUMNS: LazyLock<BTreeMap<&'static str, Vec<Column>>> = LazyLock::new(|| {
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

pub fn columns(table: &str) -> &'static [Column] {
    COLUMNS.get(table).map_or(&[], Vec::as_slice)
}

/// The Arrow type of a schema column.
fn arrow_type(c: &Column) -> DataType {
    let base = match c.kind.as_str() {
        "DATE" => DataType::Date32,
        "DATETIME" => DataType::Timestamp(TimeUnit::Microsecond, None),
        "TIMESTAMP" => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        "FLOAT" | "FLOAT64" => DataType::Float64,
        "INTEGER" | "INT64" => DataType::Int64,
        "BOOLEAN" | "BOOL" => DataType::Boolean,
        "RECORD" | "STRUCT" => {
            DataType::Struct(Fields::from(c.fields.iter().map(field).collect::<Vec<_>>()))
        }
        _ => DataType::Utf8,
    };
    if c.mode == "REPEATED" {
        DataType::List(Arc::new(Field::new("item", base, true)))
    } else {
        base
    }
}

/// Every column nullable: files from before a column existed lack it.
fn field(c: &Column) -> Field {
    Field::new(&c.name, arrow_type(c), true)
}

static SCHEMAS: LazyLock<BTreeMap<&'static str, SchemaRef>> = LazyLock::new(|| {
    TABLES
        .iter()
        .map(|t| {
            let fields: Vec<Field> = columns(t).iter().map(field).collect();
            (*t, Arc::new(Schema::new(fields)))
        })
        .collect()
});

pub fn schema(table: &str) -> SchemaRef {
    SCHEMAS
        .get(table)
        .cloned()
        .unwrap_or_else(|| Arc::new(Schema::empty()))
}

/// The SQL type of a column, as `get_schema` shows it.
pub fn sql_type(c: &Column) -> String {
    let base = match c.kind.as_str() {
        "DATE" => "DATE".to_string(),
        "DATETIME" => "TIMESTAMP".to_string(),
        "TIMESTAMP" => "TIMESTAMP WITH TIME ZONE".to_string(),
        "FLOAT" | "FLOAT64" => "DOUBLE".to_string(),
        "INTEGER" | "INT64" => "BIGINT".to_string(),
        "BOOLEAN" | "BOOL" => "BOOLEAN".to_string(),
        "RECORD" | "STRUCT" => format!(
            "STRUCT<{}>",
            c.fields
                .iter()
                .map(|f| format!("{} {}", f.name, sql_type(f)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "VARCHAR".to_string(),
    };
    if c.mode == "REPEATED" {
        format!("ARRAY<{base}>")
    } else {
        base
    }
}

/// Only queries: no DDL (`CREATE EXTERNAL TABLE` would read files), no DML
/// (`COPY TO` would write them), no `SET`.
pub fn sql_options() -> SQLOptions {
    SQLOptions::new()
        .with_allow_ddl(false)
        .with_allow_dml(false)
        .with_allow_statements(false)
}

/// A session with at most `memory` bytes for query state and no spilling
/// to disk.
pub fn session(memory: usize) -> Result<SessionContext> {
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_limit(memory, 1.0)
        .with_disk_manager_builder(
            DiskManagerBuilder::default().with_mode(DiskManagerMode::Disabled),
        )
        .build_arc()?;
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new().with_target_partitions(2))
        .with_runtime_env(runtime)
        .with_default_features()
        .build();
    Ok(SessionContext::new_with_state(state))
}

/// Registers `table` over Parquet `files`, read with the table's schema
/// (columns a file lacks come back NULL); without files it is empty.
pub async fn register(ctx: &SessionContext, table: &str, files: &[PathBuf]) -> Result<()> {
    register_as(ctx, table, table, files).await
}

/// `register` under another name.
pub async fn register_as(
    ctx: &SessionContext,
    name: &str,
    table: &str,
    files: &[PathBuf],
) -> Result<()> {
    let schema = schema(table);
    if files.is_empty() {
        ctx.register_table(name, Arc::new(MemTable::try_new(schema, vec![vec![]])?))?;
        return Ok(());
    }
    let urls = files
        .iter()
        .map(|p| ListingTableUrl::parse(p.display().to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let options =
        ListingOptions::new(Arc::new(ParquetFormat::default())).with_file_extension(".parquet");
    let config = ListingTableConfig::new_with_multi_paths(urls)
        .with_listing_options(options)
        .with_schema(schema);
    ctx.register_table(name, Arc::new(ListingTable::try_new(config)?))?;
    Ok(())
}

/// Runs one query (see `sql_options`).
pub async fn run(ctx: &SessionContext, sql: &str) -> Result<Vec<RecordBatch>> {
    let df = ctx
        .sql_with_options(sql, sql_options())
        .await
        .with_context(|| format!("query failed: {}", first_line(sql)))?;
    df.collect()
        .await
        .with_context(|| format!("query failed: {}", first_line(sql)))
}

/// Record batches as JSON objects keyed by column name; NULL columns are
/// left out. Dates and times come as the model types read them
/// (`2026-10-22`, `2026-10-22T07:00:00`).
pub fn to_json(batches: &[RecordBatch]) -> Result<Vec<Value>> {
    let mut w = ArrayWriter::new(Vec::new());
    for b in batches {
        w.write(b)?;
    }
    w.finish()?;
    let bytes = w.into_inner();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Runs a query and returns its rows as JSON objects (see `to_json`).
pub async fn rows(ctx: &SessionContext, sql: &str) -> Result<Vec<Value>> {
    to_json(&run(ctx, sql).await?)
}

/// Runs a query and deserializes its rows.
pub async fn query_as<T: DeserializeOwned>(ctx: &SessionContext, sql: &str) -> Result<Vec<T>> {
    rows(ctx, sql)
        .await?
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_schema_types() {
        let nights = schema("nights");
        assert_eq!(
            nights.field_with_name("date").unwrap().data_type(),
            &DataType::Date32
        );
        assert_eq!(
            nights.field_with_name("start").unwrap().data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, None)
        );
        assert!(matches!(
            nights.field_with_name("stages").unwrap().data_type(),
            DataType::List(_)
        ));
        let stages = columns("nights")
            .iter()
            .find(|c| c.name == "stages")
            .unwrap();
        assert_eq!(
            sql_type(stages),
            "ARRAY<STRUCT<start TIMESTAMP, end TIMESTAMP, stage VARCHAR>>"
        );
        for t in TABLES {
            assert!(!columns(t).is_empty(), "{t}");
        }
    }

    #[tokio::test]
    async fn rows_come_back_as_model_json() {
        let ctx = session(64 << 20).unwrap();
        let rows = rows(
            &ctx,
            "SELECT DATE '2026-10-22' AS d, TIMESTAMP '2026-10-22 07:00:00' AS t,
                    TIMESTAMP '2026-10-22 07:00:00.5' AS f, CAST(1.5 AS DOUBLE) AS x, 2 AS n,
                    NULL AS z, 'a''b' AS s,
                    [named_struct('start', TIMESTAMP '2026-10-22 01:00:00', 'stage', 'DEEP')] AS stages",
        )
        .await
        .unwrap();
        assert_eq!(
            rows[0],
            json!({
                "d": "2026-10-22", "t": "2026-10-22T07:00:00", "f": "2026-10-22T07:00:00.500",
                "x": 1.5, "n": 2, "s": "a'b",
                "stages": [{"start": "2026-10-22T01:00:00", "stage": "DEEP"}],
            })
        );
    }

    #[tokio::test]
    async fn empty_tables_have_the_schema() {
        let ctx = session(64 << 20).unwrap();
        register(&ctx, "spo2", &[]).await.unwrap();
        let r = rows(&ctx, "SELECT count(*) AS n, max(pct) AS m FROM spo2")
            .await
            .unwrap();
        assert_eq!(r[0], json!({"n": 0}));
    }
}
