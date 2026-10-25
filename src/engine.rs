//! DataFusion over the users' Parquet tables: the table schemas, sessions
//! that know only the tables registered on them, and query results as JSON.
//!
//! A table's columns are the fields of its row struct (`model.rs`,
//! `#[derive(Columns)]`).
//!
//! A session has no way to files or the network of its own: there is no
//! SQL function that reads a path, and DDL, DML and `SET` are refused
//! (`sql_options`). What a query can read is what was registered.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
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
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::model::{
    ActivityRow, DailyMetrics, Exercise, HeartRateRow, HrvRow, NightRow, Row, Spo2Row,
};

pub use fitdata_derive::Columns;

/// The normalized tables with their schemas, in the order `get_schema`
/// lists them.
static TABLES: LazyLock<Vec<(&'static str, SchemaRef)>> = LazyLock::new(|| {
    fn table<R: Row>() -> (&'static str, SchemaRef) {
        (R::TABLE, Arc::new(Schema::new(R::columns())))
    }
    vec![
        table::<DailyMetrics>(),
        table::<NightRow>(),
        table::<Exercise>(),
        table::<HeartRateRow>(),
        table::<ActivityRow>(),
        table::<Spo2Row>(),
        table::<HrvRow>(),
    ]
});

/// The names of the normalized tables.
pub fn tables() -> impl Iterator<Item = &'static str> {
    TABLES.iter().map(|(t, _)| *t)
}

pub fn schema(table: &str) -> SchemaRef {
    TABLES
        .iter()
        .find(|(t, _)| *t == table)
        .map(|(_, s)| s.clone())
        .unwrap_or_else(|| Arc::new(Schema::empty()))
}

/// A struct whose fields are columns: a table's row, or a struct nested in
/// one. Derived (`#[derive(Columns)]`) as one column per field, named as
/// serde names it, typed by its `ColumnType`, described by its doc comment.
pub trait Columns {
    fn columns() -> Vec<Field>;
}

/// A Rust type stored in a column, and its Arrow type.
pub trait ColumnType {
    fn data_type() -> DataType;
}

macro_rules! column_types {
    ($($t:ty => $dt:expr),* $(,)?) => {
        $(impl ColumnType for $t {
            fn data_type() -> DataType {
                $dt
            }
        })*
    };
}

column_types! {
    f64 => DataType::Float64,
    i64 => DataType::Int64,
    bool => DataType::Boolean,
    String => DataType::Utf8,
    NaiveDate => DataType::Date32,
    NaiveDateTime => DataType::Timestamp(TimeUnit::Microsecond, None),
}

impl<T: ColumnType> ColumnType for Option<T> {
    fn data_type() -> DataType {
        T::data_type()
    }
}

impl<T: ColumnType> ColumnType for Vec<T> {
    fn data_type() -> DataType {
        DataType::List(Arc::new(Field::new("item", T::data_type(), true)))
    }
}

const DESCRIPTION: &str = "description";

/// A column of type `T`, described by the lines of `doc`. Every column is
/// nullable: files from before a column existed lack it.
pub fn column<T: ColumnType>(name: &str, doc: &[&str]) -> Field {
    let field = Field::new(name, T::data_type(), true);
    let text = doc.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return field;
    }
    field.with_metadata([(DESCRIPTION.to_string(), text)].into())
}

/// What a column holds, as `get_schema` shows it.
pub fn description(field: &Field) -> Option<&str> {
    field.metadata().get(DESCRIPTION).map(String::as_str)
}

/// The SQL type of a column, as `get_schema` shows it.
pub fn sql_type(t: &DataType) -> String {
    match t {
        DataType::Date32 => "DATE".to_string(),
        DataType::Timestamp(_, None) => "TIMESTAMP".to_string(),
        DataType::Timestamp(_, Some(_)) => "TIMESTAMP WITH TIME ZONE".to_string(),
        DataType::Float64 => "DOUBLE".to_string(),
        DataType::Int64 => "BIGINT".to_string(),
        DataType::Boolean => "BOOLEAN".to_string(),
        DataType::List(item) => format!("ARRAY<{}>", sql_type(item.data_type())),
        DataType::Struct(fields) => format!(
            "STRUCT<{}>",
            fields
                .iter()
                .map(|f| format!("{} {}", f.name(), sql_type(f.data_type())))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "VARCHAR".to_string(),
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
        assert_eq!(
            sql_type(nights.field_with_name("stages").unwrap().data_type()),
            "ARRAY<STRUCT<start TIMESTAMP, end TIMESTAMP, stage VARCHAR>>"
        );
        for t in tables() {
            assert!(!schema(t).fields().is_empty(), "{t}");
        }
    }

    #[test]
    fn row_structs_declare_their_columns() {
        let hr = schema("heart_rate");
        let ts = hr.field_with_name("ts").unwrap();
        assert_eq!(
            ts.data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, None)
        );
        assert!(ts.is_nullable());
        assert_eq!(description(ts), Some("Local wall-clock time"));
        assert_eq!(description(hr.field_with_name("bpm").unwrap()), None);
        let names: Vec<_> = hr.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, ["date", "ts", "bpm"]);
    }

    #[test]
    fn columns_follow_serde_names() {
        #[derive(serde::Serialize, Columns)]
        struct R {
            /// Two
            /// lines.
            #[serde(rename = "b")]
            a: Option<f64>,
            #[serde(skip)]
            _x: i64,
        }
        let cols = R::columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].name(), "b");
        assert_eq!(cols[0].data_type(), &DataType::Float64);
        assert_eq!(description(&cols[0]), Some("Two lines."));
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
