// Layout of the deeply nested MCP handler futures (mcp::handle).
#![recursion_limit = "256"]

mod api;
mod authz;
mod claims;
mod cloudrun;
mod datatypes;
mod engine;
mod health;
mod job;
mod kms;
mod mcp;
mod model;
mod oauth;
mod query;
mod secrets;
mod status;
mod store;
mod sync;
mod tables;
mod telemetry;
mod users;
mod warehouse;
mod web;

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{Days, NaiveDate};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tracing::Instrument;

use datatypes::DataType;
use kms::Kms;
use oauth::{ClientCredentials, StoredToken};
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
    /// Users and their data: a local directory or `gs://bucket`.
    #[arg(long, env = "STORAGE", default_value = "data")]
    storage: String,
    /// Region of the Cloud Run resources.
    #[arg(long, env = "GCP_REGION", default_value = "europe-west1")]
    region: String,
    /// Cloud KMS key that encrypts refresh tokens
    /// (`projects/.../cryptoKeys/...`), or `plain` for local development.
    #[arg(long, env = "KMS_KEY", default_value = "plain")]
    kms_key: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dashboard and MCP server over the users' tables.
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
        /// Skip sign-in and act as this user ID (local development only).
        #[arg(long, env = "DEV_USER")]
        dev_user: Option<String>,
        /// Cloud Run job the "sync now" button starts.
        #[arg(long, env = "SYNC_JOB", default_value = "fitdata-sync")]
        sync_job: String,
        /// Where the users' tables are cached (on Cloud Run: memory).
        #[arg(long, env = "CACHE_DIR", default_value = "/tmp/fitdata-cache")]
        cache_dir: std::path::PathBuf,
    },
    /// The hourly job: sync the last N days of every user (or one).
    Job {
        #[arg(long, default_value_t = 7)]
        days: u64,
        /// Only this user ID ("sync now" on the dashboard).
        #[arg(long)]
        user: Option<String>,
    },
    /// Rebuild a user's tables for local dates `[from, to]` from their raw data.
    Load {
        #[arg(long)]
        user: String,
        #[arg(long)]
        from: NaiveDate,
        /// Inclusive end date (default: today).
        #[arg(long)]
        to: Option<NaiveDate>,
    },
    /// Register a user with an existing refresh token (e.g. from Secret Manager).
    ImportUser {
        #[arg(long)]
        user: String,
        #[arg(long)]
        email: String,
        /// `StoredToken` JSON: `sm://<secret>` or a file path.
        #[arg(long)]
        refresh_token: String,
    },
    /// Interactive OAuth consent; stores the refresh token of a user.
    Login {
        #[arg(long)]
        user: String,
        #[arg(long)]
        email: String,
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Show identity, profile and paired devices of a user.
    Whoami {
        #[arg(long)]
        user: String,
    },
    /// Check every data type: accessible? has data? newest point?
    Probe {
        #[arg(long)]
        user: String,
    },
    /// Fetch all history of a user, walking back month by month (resumable).
    Backfill {
        #[arg(long)]
        user: String,
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Do not go further back than this date (default: account creation).
        #[arg(long)]
        since: Option<NaiveDate>,
        /// Stop a type after this many consecutive months without data.
        #[arg(long, default_value_t = 3)]
        max_empty_months: u32,
    },
    /// Re-fetch a date range of a user (default: the last 7 days).
    Sync {
        #[arg(long)]
        user: String,
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

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let telemetry = telemetry::Telemetry::init();
    // One trace per job run; the server makes one per request instead.
    let span = match &cli.cmd {
        Cmd::Job { days, .. } => tracing::info_span!("job", otel.name = "sync job", days),
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
    let http = reqwest::Client::builder()
        .user_agent("fitdata/0.1")
        .build()?;
    let project = cli.gcp_project.as_deref();
    let client_ref = SecretRef::parse(&cli.oauth_client, project)?;
    let creds = ClientCredentials::from_json(
        &client_ref
            .read()
            .await?
            .with_context(|| format!("no OAuth client in {client_ref}"))?,
    )?;
    let store = Store::open(&cli.storage).await?;
    let kms = Kms::new(&cli.kms_key)?;
    let ctx = job::Ctx {
        http: &http,
        creds: &creds,
        store: &store,
        kms: &kms,
    };

    let user = match &cli.cmd {
        Cmd::Serve {
            port,
            public_url,
            allowed_emails,
            session_key,
            static_dir,
            dev_user,
            sync_job,
            cache_dir,
        } => {
            let session_key = SecretRef::parse(session_key, project)?
                .read()
                .await?
                .context("session key secret is empty")?;
            if let Some(uid) = dev_user {
                users::check_uid(uid)?;
            }
            let store = Arc::new(store.clone());
            return web::serve(web::Config {
                port: *port,
                static_dir: static_dir.clone(),
                public_url: public_url.trim_end_matches('/').to_string(),
                allowed_emails: allowed_emails
                    .iter()
                    .map(|e| e.trim().to_lowercase())
                    .collect(),
                session_key,
                dev_user: dev_user.clone(),
                creds: creds.clone(),
                warehouse: warehouse::Warehouse::new(store.clone(), cache_dir.clone()),
                store,
                kms,
                sync_job: cloudrun::SyncJob::new(
                    http.clone(),
                    project.context("GCP_PROJECT is required")?,
                    &cli.region,
                    sync_job,
                )?,
                http: http.clone(),
            })
            .await;
        }
        Cmd::Job { days, user } => return job::run(&ctx, *days, user.as_deref()).await,
        Cmd::Load { user, from, to } => {
            let u = users::get(&store, user)
                .await?
                .with_context(|| format!("no user {user}"))?;
            let to = to.unwrap_or_else(|| job::today(&u));
            let raw = store.scoped(&tables::user_prefix(user));
            return job::load(&store, &raw, user, *from, to).await;
        }
        Cmd::ImportUser {
            user,
            email,
            refresh_token,
        } => {
            users::check_uid(user)?;
            let source = SecretRef::parse(refresh_token, project)?;
            let token = StoredToken::from_json(
                &source
                    .read()
                    .await?
                    .with_context(|| format!("no refresh token in {source}"))?,
            )?;
            users::save_token(&store, &kms, user, email, &token).await?;
            println!("Stored the refresh token of {user} ({email})");
            return Ok(());
        }
        Cmd::Login { user, email, port } => {
            users::check_uid(user)?;
            let token = oauth::login(&http, &creds, *port).await?;
            users::save_token(&store, &kms, user, email, &token).await?;
            println!("Stored the refresh token of {user}");
            return Ok(());
        }
        Cmd::Whoami { user }
        | Cmd::Probe { user }
        | Cmd::Backfill { user, .. }
        | Cmd::Sync { user, .. } => user.clone(),
    };

    let u = users::get(&store, &user)
        .await?
        .with_context(|| format!("no user {user}"))?;
    let client = job::client(&ctx, &u).await?;
    let raw = store.scoped(&tables::user_prefix(&user));

    match cli.cmd {
        Cmd::Whoami { .. } => {
            let info = json!({
                "identity": client.identity().await?,
                "profile": client.profile().await.unwrap_or_else(|e| json!({"error": e.to_string()})),
                "pairedDevices": client.paired_devices().await.unwrap_or_else(|e| json!({"error": e.to_string()})),
            });
            println!("{}", serde_json::to_string_pretty(&info)?);
            raw.put_json("meta/whoami.json", &info).await?;
        }
        Cmd::Probe { .. } => {
            let report = sync::probe(&client, &select_types(&[])?).await;
            for r in &report {
                println!(
                    "{:<36} {:<12} {}",
                    r["dataType"].as_str().unwrap_or_default(),
                    r["status"].as_str().unwrap_or_default(),
                    r["newest"].as_str().unwrap_or("")
                );
            }
            raw.put_json("meta/probe.json", &Value::Array(report))
                .await?;
            println!(
                "\nFull report incl. one sample point per type: {}/meta/probe.json",
                raw.location()
            );
        }
        Cmd::Backfill {
            types,
            since,
            max_empty_months,
            ..
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
                &raw,
                &select_types(&types)?,
                since,
                max_empty_months,
            )
            .await?;
            println!("Backfill done: {n} new data points in {}", raw.location());
        }
        Cmd::Sync {
            types,
            days,
            from,
            to,
            ..
        } => {
            let today = job::today(&u);
            let to = to.unwrap_or(today + Days::new(1));
            let from = from.unwrap_or(today - Days::new(days));
            if from >= to {
                bail!("--from must be before --to");
            }
            let fetched =
                sync::fetch_range(&client, &raw, &select_types(&types)?, from, to).await?;
            println!("Sync {from}..{to} done: {} data points", fetched.points);
            if !fetched.skipped.is_empty() {
                println!("Not readable: {}", fetched.skipped.join(", "));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}
