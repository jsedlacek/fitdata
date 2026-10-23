//! BigQuery: idempotent loads of normalized rows and queries for the web.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use google_cloud_bigquery_v2::client::JobService;
use google_cloud_bigquery_v2::model::{
    Job, JobConfiguration, JobConfigurationLoad, QueryRequest, TableFieldSchema, TableReference,
};
use serde_json::{Map, Value};

use crate::store::Store;

pub struct Bq {
    jobs: JobService,
    pub project: String,
    pub dataset: String,
    location: String,
}

impl Bq {
    pub async fn new(project: &str, dataset: &str, location: &str) -> Result<Self> {
        Ok(Self {
            jobs: JobService::builder()
                .build()
                .await
                .context("BigQuery client")?,
            project: project.to_string(),
            dataset: dataset.to_string(),
            location: location.to_string(),
        })
    }

    /// Fully qualified, quoted table name for SQL.
    pub fn table(&self, name: &str) -> String {
        format!("`{}.{}.{name}`", self.project, self.dataset)
    }

    /// Replaces the rows of `table` whose `date` is in `[from, to]` with `rows`.
    ///
    /// Rows are staged as NDJSON in GCS, loaded into `_staging_{table}` (same
    /// schema, created by deploy/setup.sh) and swapped in with one
    /// DELETE + INSERT transaction, so re-running a day never duplicates it.
    pub async fn replace_range(
        &self,
        store: &Store,
        table: &str,
        rows: &[Value],
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<()> {
        let staging = format!("_staging_{table}");
        let path = format!("staging/{table}.jsonl");
        store.put_jsonl(&path, rows).await?;
        let uri = store
            .gs_uri(&path)
            .context("BigQuery loads need STORAGE=gs://...")?;

        let load = JobConfigurationLoad::new()
            .set_source_uris([uri])
            .set_source_format("NEWLINE_DELIMITED_JSON")
            .set_write_disposition("WRITE_TRUNCATE")
            .set_destination_table(
                TableReference::new()
                    .set_project_id(&self.project)
                    .set_dataset_id(&self.dataset)
                    .set_table_id(&staging),
            );
        let job = self
            .jobs
            .insert_job()
            .set_project_id(&self.project)
            .set_job(Job::new().set_configuration(JobConfiguration::new().set_load(load)))
            .into_job_poller()
            .until_done()
            .await
            .map_err(|e| anyhow::anyhow!("load job for {table} failed: {e:?}"))?;
        if let Some(err) = job.status.and_then(|s| s.error_result) {
            bail!("load job for {table} failed: {}", err.message);
        }

        self.query(format!(
            "BEGIN TRANSACTION;
             DELETE FROM {t} WHERE date BETWEEN '{from}' AND '{to}';
             INSERT INTO {t} SELECT * FROM {s};
             COMMIT TRANSACTION;",
            t = self.table(table),
            s = self.table(&staging),
        ))
        .await?;
        tracing::info!("{table:<12} {from}..{to} {:>7} rows", rows.len());
        Ok(())
    }

    /// Runs a query and returns its rows as JSON objects keyed by column name.
    /// Only interpolate trusted values (dates parsed by us) into `sql`.
    pub async fn query(&self, sql: String) -> Result<Vec<Value>> {
        let mut resp = self
            .jobs
            .query()
            .set_project_id(&self.project)
            .set_query_request(
                QueryRequest::new()
                    .set_query(&sql)
                    .set_use_legacy_sql(false)
                    .set_location(&self.location)
                    .set_timeout_ms(30_000u32),
            )
            .send()
            .await
            .with_context(|| format!("query failed: {}", first_line(&sql)))?;
        if let Some(e) = resp.errors.first() {
            bail!("query failed: {}", e.message);
        }
        let mut complete = resp.job_complete.unwrap_or(true);
        let job_id = resp
            .job_reference
            .as_ref()
            .map(|j| j.job_id.clone())
            .unwrap_or_default();
        let mut rows = std::mem::take(&mut resp.rows);
        let mut schema = resp.schema.take();
        let mut page_token = resp.page_token.clone();

        // Long queries and multi-page results continue via getQueryResults.
        while !complete || !page_token.is_empty() {
            let r = self
                .jobs
                .get_query_results()
                .set_project_id(&self.project)
                .set_job_id(&job_id)
                .set_location(&self.location)
                .set_page_token(&page_token)
                .set_timeout_ms(30_000u32)
                .send()
                .await?;
            if let Some(e) = r.errors.first() {
                bail!("query failed: {}", e.message);
            }
            complete = r.job_complete.unwrap_or(true);
            if complete {
                rows.extend(r.rows);
                schema = schema.or(r.schema);
                page_token = r.page_token;
            }
        }

        let fields = schema.map(|s| s.fields).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| decode_record(&fields, &Value::Object(r)))
            .collect())
    }
}

/// A quoted GoogleSQL string literal for untrusted text (at most 1000 chars).
pub fn string_literal(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars().take(1000) {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn first_line(sql: &str) -> &str {
    sql.trim().lines().next().unwrap_or_default()
}

/// Converts BigQuery's `{"f": [{"v": ...}]}` row encoding into a JSON object,
/// typing scalars by the schema (all values arrive as strings).
fn decode_record(fields: &[TableFieldSchema], row: &Value) -> Value {
    let cells = row["f"].as_array().cloned().unwrap_or_default();
    let mut out = Map::new();
    for (field, cell) in fields.iter().zip(cells) {
        let v = &cell["v"];
        let value = if field.mode == "REPEATED" {
            Value::Array(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| decode_value(field, &c["v"]))
                    .collect(),
            )
        } else {
            decode_value(field, v)
        };
        out.insert(field.name.clone(), value);
    }
    Value::Object(out)
}

fn decode_value(field: &TableFieldSchema, v: &Value) -> Value {
    if v.is_null() {
        return Value::Null;
    }
    match field.r#type.as_str() {
        "RECORD" | "STRUCT" => decode_record(&field.fields, v),
        "INTEGER" | "INT64" | "FLOAT" | "FLOAT64" | "NUMERIC" | "BIGNUMERIC" => v
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        "BOOLEAN" | "BOOL" => Value::Bool(v.as_str() == Some("true")),
        _ => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(name: &str, ty: &str, mode: &str, fields: Vec<TableFieldSchema>) -> TableFieldSchema {
        TableFieldSchema::new()
            .set_name(name)
            .set_type(ty)
            .set_mode(mode)
            .set_fields(fields)
    }

    #[test]
    fn escapes_string_literals() {
        assert_eq!(string_literal("it's a\\b\nc"), r"'it\'s a\\b\nc'");
    }

    #[test]
    fn decodes_rows_by_schema() {
        let fields = vec![
            field("date", "DATE", "", vec![]),
            field("bpm", "FLOAT", "", vec![]),
            field("missing", "INTEGER", "", vec![]),
            field(
                "stages",
                "RECORD",
                "REPEATED",
                vec![
                    field("stage", "STRING", "", vec![]),
                    field("n", "INTEGER", "", vec![]),
                ],
            ),
        ];
        let row = json!({"f": [
            {"v": "2026-10-23"}, {"v": "57.5"}, {"v": null},
            {"v": [{"v": {"f": [{"v": "DEEP"}, {"v": "3"}]}}]}
        ]});
        assert_eq!(
            decode_record(&fields, &row),
            json!({"date": "2026-10-23", "bpm": 57.5, "missing": null, "stages": [{"stage": "DEEP", "n": 3.0}]})
        );
    }
}
