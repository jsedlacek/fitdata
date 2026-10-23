mod api;
mod bq;
mod datatypes;
mod model;
mod oauth;
mod secrets;
mod store;
mod sync;
mod web;

use anyhow::{Context, Result, bail};
use chrono::{Days, NaiveDate};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

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
        /// Skip sign-in (local development only).
        #[arg(long, env = "AUTH_DISABLED")]
        auth_disabled: bool,
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

/// The day the Fitbit / Google Health account was created.
async fn membership_start(client: &HealthClient) -> Result<Option<NaiveDate>> {
    let d = &client.profile().await?["membershipStartDate"];
    Ok(
        match (d["year"].as_i64(), d["month"].as_u64(), d["day"].as_u64()) {
            (Some(y), Some(m), Some(day)) => {
                NaiveDate::from_ymd_opt(y as i32, m as u32, day as u32)
            }
            _ => None,
        },
    )
}

/// Rebuilds the BigQuery tables for local dates `[from, to]` from raw data.
async fn load(store: &Store, bq: &Bq, from: NaiveDate, to: NaiveDate) -> Result<()> {
    if from > to {
        bail!("--from must not be after --to");
    }
    // One extra day on each side: nights start the evening before, and
    // intervals near midnight may be stored in the neighbouring day's file.
    let raw = sync::read_raw(store, from - Days::new(1), to + Days::new(2)).await?;
    let ds = model::Dataset::from_raw(&raw)?;
    for (table, rows) in model::table_rows(&ds, from, to) {
        bq.replace_range(store, table, &rows, from, to).await?;
    }
    println!("Loaded {from}..{to} into BigQuery dataset {}", bq.dataset);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fitdata=info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
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
        auth_disabled,
    } = &cli.cmd
    {
        let session_key = SecretRef::parse(session_key, project)?
            .read()
            .await?
            .context("session key secret is empty")?;
        return web::serve(web::Config {
            port: *port,
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
            http,
        })
        .await;
    }
    if let Cmd::Load { from, to } = &cli.cmd {
        let to = to.unwrap_or_else(|| chrono::Local::now().date_naive());
        return load(&store, &bq().await?, *from, to).await;
    }

    if let Cmd::Login { port } = cli.cmd {
        let token = oauth::login(&http, &creds, port).await?;
        token_ref.write(&token.to_json()?).await?;
        println!("Saved refresh token to {token_ref}");
        return Ok(());
    }

    let refresh_token =
        StoredToken::from_json(&token_ref.read().await?.with_context(|| {
            format!("no refresh token in {token_ref}; run `fitdata login` first")
        })?)?
        .refresh_token;
    let client = HealthClient::new(http.clone(), TokenSource::new(http, creds, refresh_token));

    match cli.cmd {
        Cmd::Login { .. } | Cmd::Serve { .. } | Cmd::Load { .. } => unreachable!(),
        Cmd::Job { days } => {
            let today = chrono::Local::now().date_naive();
            let from = today - Days::new(days);
            let n = sync::fetch_range(
                &client,
                &store,
                &select_types(&[])?,
                from,
                today + Days::new(1),
            )
            .await?;
            println!("Synced {from}..{today}: {n} data points");
            load(&store, &bq().await?, from, today).await?;
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
                None => membership_start(&client).await?,
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
            let n = sync::fetch_range(&client, &store, &select_types(&types)?, from, to).await?;
            println!("Sync {from}..{to} done: {n} data points");
        }
    }
    Ok(())
}
