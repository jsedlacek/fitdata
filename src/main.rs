// Layout of the deeply nested MCP handler futures (mcp::handle).
#![recursion_limit = "256"]

mod api;
mod authz;
mod bq;
mod claims;
mod cloudrun;
mod datatypes;
mod duck;
mod health;
mod mcp;
mod model;
mod oauth;
mod query;
mod secrets;
mod store;
mod sync;
mod tables;
mod telemetry;
mod warehouse;
mod web;

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use chrono::{Days, NaiveDate};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tracing::Instrument;

use api::HealthClient;
use bq::Bq;
use datatypes::DataType;
use oauth::{ClientCredentials, StoredToken, TokenSource};
use secrets::SecretRef;
use store::Store;

/// Downloads Fitbit / Google Health data via the Google Health API v4.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Project holding the `sm://` secrets.
    #[arg(long, env = "GCP_PROJECT")]
    gcp_project: Option<String>,
    /// OAuth client JSON from Cloud Console: `sm://<secret>` or a file path.
    #[arg(long, env = "OAUTH_CLIENT", default_value = "sm://oauth-client")]
    oauth_client: String,
    /// Where `login` stores the refresh token and other commands read it.
    #[arg(long, env = "REFRESH_TOKEN", default_value = "sm://refresh-token")]
    refresh_token: String,
    /// Raw data location: a local directory or `gs://bucket`.
    #[arg(long, env = "STORAGE", default_value = "data")]
    storage: String,
    /// Location of the BigQuery dataset (and of the Cloud Run resources).
    #[arg(long, env = "GCP_REGION", default_value = "europe-west1")]
    region: String,
    #[arg(long, env = "BQ_DATASET", default_value = "health")]
    bq_dataset: String,
    /// Google account ID (`sub`) whose Parquet tables `load` and `job` write.
    #[arg(long, env = "OWNER_ID")]
    owner_id: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dashboard over the BigQuery tables.
    Serve {
        #[arg(long, env = "PORT", default_value_t = 8080)]
        port: u16,
        /// Base URL the browser uses; Google redirects to {url}/auth/callback.
        #[arg(long, env = "PUBLIC_URL", default_value = "http://localhost:8080")]
        public_url: String,
        /// Google accounts allowed to sign in (comma separated).
        #[arg(long, env = "ALLOWED_EMAILS", value_delimiter = ',')]
        allowed_emails: Vec<String>,
        /// Key for signing session cookies: `sm://<secret>` or a file path.
        #[arg(long, env = "SESSION_KEY", default_value = "sm://session-key")]
        session_key: String,
        /// The built frontend (`npm run build` in frontend/).
        #[arg(long, env = "STATIC_DIR", default_value = "frontend/dist")]
        static_dir: std::path::PathBuf,
        /// Skip sign-in (local development only).
        #[arg(long, env = "AUTH_DISABLED", value_parser = clap::builder::BoolishValueParser::new())]
        auth_disabled: bool,
        /// Cloud Run job the "sync now" button starts.
        #[arg(long, env = "SYNC_JOB", default_value = "fitdata-sync")]
        sync_job: String,
        /// The service that runs MCP `query` SQL (`fitdata query-serve`).
        #[arg(long, env = "QUERY_URL", default_value = "http://localhost:8081")]
        query_url: String,
        /// Where the users' tables are cached (on Cloud Run: memory).
        #[arg(long, env = "CACHE_DIR", default_value = "/tmp/fitdata-cache")]
        cache_dir: std::path::PathBuf,
    },
    /// Runs MCP `query` SQL against the files sent with each request.
    QueryServe {
        #[arg(long, env = "PORT", default_value_t = 8081)]
        port: u16,
    },
    /// Interactive OAuth consent; stores a refresh token.
    Login {
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Show identity, profile and paired devices.
    Whoami,
    /// Check every data type: accessible? has data? newest point?
    Probe,
    /// Fetch all history, walking back month by month (resumable).
    Backfill {
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Do not go further back than this date (default: account creation).
        #[arg(long)]
        since: Option<NaiveDate>,
        /// Stop a type after this many consecutive months without data.
        #[arg(long, default_value_t = 3)]
        max_empty_months: u32,
    },
    /// Normalize stored raw data for local dates `[from, to]` into BigQuery.
    Load {
        #[arg(long)]
        from: NaiveDate,
        /// Inclusive end date (default: today).
        #[arg(long)]
        to: Option<NaiveDate>,
    },
    /// The daily job: `sync` the last N days, then `load` them into BigQuery.
    Job {
        #[arg(long, default_value_t = 7)]
        days: u64,
    },
    /// Re-fetch a date range (default: the last 7 days), overwriting stored data.
    Sync {
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        #[arg(long, default_value_t = 7)]
        days: u64,
        #[arg(long)]
        from: Option<NaiveDate>,
        /// Exclusive end date (default: tomorrow).
        #[arg(long)]
        to: Option<NaiveDate>,
    },
}

fn select_types(ids: &[String]) -> Result<Vec<&'static DataType>> {
    if ids.is_empty() {
        return Ok(datatypes::ALL.iter().collect());
    }
    ids.iter()
        .map(|id| datatypes::find(id).with_context(|| format!("unknown data type `{id}`")))
        .collect()
}

async fn health_client(
    http: &reqwest::Client,
    creds: ClientCredentials,
    token_ref: &SecretRef,
) -> Result<HealthClient> {
    let stored = token_ref.read().await?.ok_or_else(|| {
        oauth::AuthError(format!(
            "no refresh token in {token_ref}; run `fitdata login` first"
        ))
    })?;
    let refresh_token = StoredToken::from_json(&stored)?.refresh_token;
    Ok(HealthClient::new(
        http.clone(),
        TokenSource::new(http.clone(), creds, refresh_token),
    ))
}

/// The daily job: re-fetch the last `days` days, then rebuild them in BigQuery.
/// Returns the data types that were skipped as not readable.
async fn job(
    http: &reqwest::Client,
    creds: ClientCredentials,
    token_ref: &SecretRef,
    store: &Store,
    bq: &Bq,
    owner: Option<&str>,
    days: u64,
) -> Result<Vec<&'static str>> {
    let client = health_client(http, creds, token_ref).await?;
    let today = chrono::Local::now().date_naive();
    let from = today - Days::new(days);
    let fetched = sync::fetch_range(
        &client,
        store,
        &select_types(&[])?,
        from,
        today + Days::new(1),
    )
    .await?;
    tracing::info!("synced {from}..{today}: {} data points", fetched.points);
    if !fetched.skipped.is_empty() {
        // Usually a scope missing from the refresh token: reconnect Fitbit.
        tracing::warn!("data types not readable: {}", fetched.skipped.join(", "));
    }
    load(store, bq, owner, from, today).await?;
    Ok(fetched.skipped)
}

/// Appends the outcome of a job run to the `sync_runs` table.
async fn record_run(
    bq: &Bq,
    started: chrono::DateTime<chrono::Utc>,
    days: u64,
    result: &Result<Vec<&'static str>>,
) -> Result<()> {
    let auth_error = result
        .as_ref()
        .err()
        .is_some_and(|e| e.chain().any(|c| c.is::<oauth::AuthError>()));
    let error = match result {
        Ok(_) => "NULL".to_string(),
        Err(e) => bq::string_literal(&format!("{e:#}")),
    };
    let skipped: Vec<String> = result
        .as_ref()
        .map(|s| s.iter().map(|id| bq::string_literal(id)).collect())
        .unwrap_or_default();
    let local = started.with_timezone(&chrono::Local);
    bq.query(format!(
        "INSERT INTO {} (date, run_at, ok, auth_error, error, days, skipped) VALUES ('{}', '{}', {}, {auth_error}, {error}, {days}, ARRAY<STRING>[{}])",
        bq.table("sync_runs"),
        local.date_naive(),
        started.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        result.is_ok(),
        skipped.join(", "),
    ))
    .await?;
    Ok(())
}

/// Rebuilds the BigQuery tables (and the owner's Parquet tables) for local
/// dates `[from, to]` from raw data.
async fn load(
    store: &Store,
    bq: &Bq,
    owner: Option<&str>,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<()> {
    if from > to {
        bail!("--from must not be after --to");
    }
    // One extra day on each side: nights start the evening before, and
    // intervals near midnight may be stored in the neighbouring day's file.
    let points = sync::read_raw(store, from - Days::new(1), to + Days::new(2)).await?;
    let ds = model::Dataset::from_points(points);
    let t = model::table_rows(&ds, from, to);
    bq.replace_range(store, &t.heart_rate, from, to).await?;
    bq.replace_range(store, &t.activity, from, to).await?;
    bq.replace_range(store, &t.spo2, from, to).await?;
    bq.replace_range(store, &t.hrv, from, to).await?;
    bq.replace_range(store, &t.nights, from, to).await?;
    bq.replace_range(store, &t.exercises, from, to).await?;
    bq.replace_range(store, &t.daily, from, to).await?;
    tracing::info!("loaded {from}..{to} into BigQuery dataset {}", bq.dataset);
    if let Some(uid) = owner {
        tables::write_all(store, uid, &t, from, to).await?;
        tracing::info!("wrote {from}..{to} as Parquet for {uid}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let telemetry = telemetry::Telemetry::init();
    // One trace per job run; the server makes one per request instead.
    let span = match cli.cmd {
        Cmd::Job { days } => tracing::info_span!("job", otel.name = "sync job", days),
        _ => tracing::Span::none(),
    };
    let result = async {
        let result = run(cli).await;
        if let Err(e) = &result {
            tracing::error!("{e:#}");
        }
        result
    }
    .instrument(span)
    .await;
    telemetry.shutdown().await;
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn run(cli: Cli) -> Result<()> {
    // Needs nothing else: no secrets, storage or credentials (see query.rs).
    if let Cmd::QueryServe { port } = cli.cmd {
        return query::serve(port).await;
    }
    let http = reqwest::Client::builder()
        .user_agent("fitdata/0.1")
        .build()?;
    let project = cli.gcp_project.as_deref();
    let client_ref = SecretRef::parse(&cli.oauth_client, project)?;
    let token_ref = SecretRef::parse(&cli.refresh_token, project)?;
    let creds = ClientCredentials::from_json(
        &client_ref
            .read()
            .await?
            .with_context(|| format!("no OAuth client in {client_ref}"))?,
    )?;
    let store = Store::open(&cli.storage).await?;
    let bq = || async {
        Bq::new(
            project.context("GCP_PROJECT is required for BigQuery")?,
            &cli.bq_dataset,
            &cli.region,
        )
        .await
    };

    if let Cmd::Serve {
        port,
        public_url,
        allowed_emails,
        session_key,
        static_dir,
        auth_disabled,
        sync_job,
        query_url,
        cache_dir,
    } = &cli.cmd
    {
        let session_key = SecretRef::parse(session_key, project)?
            .read()
            .await?
            .context("session key secret is empty")?;
        return web::serve(web::Config {
            port: *port,
            static_dir: static_dir.clone(),
            public_url: public_url.trim_end_matches('/').to_string(),
            allowed_emails: allowed_emails
                .iter()
                .map(|e| e.trim().to_lowercase())
                .collect(),
            session_key,
            auth_disabled: *auth_disabled,
            creds,
            token_ref,
            bq: bq().await?,
            warehouse: warehouse::Warehouse::new(std::sync::Arc::new(store), cache_dir.clone()),
            query: query::QueryClient::new(http.clone(), query_url),
            owner: cli
                .owner_id
                .clone()
                .context("OWNER_ID is required to serve the dashboard")?,
            sync_job: cloudrun::SyncJob::new(
                http.clone(),
                project.context("GCP_PROJECT is required")?,
                &cli.region,
                sync_job,
            )?,
            http,
        })
        .await;
    }
    if let Cmd::Load { from, to } = &cli.cmd {
        let to = to.unwrap_or_else(|| chrono::Local::now().date_naive());
        return load(&store, &bq().await?, cli.owner_id.as_deref(), *from, to).await;
    }

    if let Cmd::Login { port } = cli.cmd {
        let token = oauth::login(&http, &creds, port).await?;
        token_ref.write(&token.to_json()?).await?;
        println!("Saved refresh token to {token_ref}");
        return Ok(());
    }

    if let Cmd::Job { days } = cli.cmd {
        let bq = bq().await?;
        let started = chrono::Utc::now();
        let result = job(
            &http,
            creds,
            &token_ref,
            &store,
            &bq,
            cli.owner_id.as_deref(),
            days,
        )
        .await;
        // The dashboard reads the last run to tell whether the token still works.
        if let Err(e) = record_run(&bq, started, days, &result).await {
            tracing::error!("recording the run failed: {e:#}");
        }
        return result.map(drop);
    }

    let client = health_client(&http, creds, &token_ref).await?;

    match cli.cmd {
        Cmd::Login { .. }
        | Cmd::Serve { .. }
        | Cmd::QueryServe { .. }
        | Cmd::Load { .. }
        | Cmd::Job { .. } => {
            unreachable!()
        }
        Cmd::Whoami => {
            let info = json!({
                "identity": client.identity().await?,
                "profile": client.profile().await.unwrap_or_else(|e| json!({"error": e.to_string()})),
                "pairedDevices": client.paired_devices().await.unwrap_or_else(|e| json!({"error": e.to_string()})),
            });
            println!("{}", serde_json::to_string_pretty(&info)?);
            store.put_json("meta/whoami.json", &info).await?;
        }
        Cmd::Probe => {
            let report = sync::probe(&client, &select_types(&[])?).await;
            for r in &report {
                println!(
                    "{:<36} {:<12} {}",
                    r["dataType"].as_str().unwrap_or_default(),
                    r["status"].as_str().unwrap_or_default(),
                    r["newest"].as_str().unwrap_or("")
                );
            }
            store
                .put_json("meta/probe.json", &Value::Array(report))
                .await?;
            println!(
                "\nFull report incl. one sample point per type: {}/meta/probe.json",
                store.location()
            );
        }
        Cmd::Backfill {
            types,
            since,
            max_empty_months,
        } => {
            // Some daily types are computed for days before the account existed.
            let since = match since {
                Some(d) => Some(d),
                None => client.membership_start().await?,
            };
            if let Some(d) = since {
                println!("Backfilling from {d}");
            }
            let n = sync::backfill(
                &client,
                &store,
                &select_types(&types)?,
                since,
                max_empty_months,
            )
            .await?;
            println!("Backfill done: {n} new data points in {}", store.location());
        }
        Cmd::Sync {
            types,
            days,
            from,
            to,
        } => {
            let today = chrono::Local::now().date_naive();
            let to = to.unwrap_or(today + Days::new(1));
            let from = from.unwrap_or(today - Days::new(days));
            if from >= to {
                bail!("--from must be before --to");
            }
            let fetched =
                sync::fetch_range(&client, &store, &select_types(&types)?, from, to).await?;
            println!("Sync {from}..{to} done: {} data points", fetched.points);
            if !fetched.skipped.is_empty() {
                println!("Not readable: {}", fetched.skipped.join(", "));
            }
        }
    }
    Ok(())
}
