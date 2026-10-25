# Production on Google Cloud

```
Cloud Scheduler (every hour)
  └─> Cloud Run job fitdata-sync      `fitdata job --days 7`, for each user
        ├─ users/{uid}.json: encrypted refresh token ─> Cloud KMS (only the job decrypts)
        ├─ Google Health API ─> GCS u/{uid}/raw/{type}/{day|month}.jsonl
        └─ normalization (DataFusion) ─> GCS u/{uid}/data/{table}[/{YYYY-MM}].parquet + manifest.json
Cloud Run service fitdata-web          `fitdata serve`   https://fitdata.jakub.contact
  └─ Google sign-in (ALLOWED_EMAILS) ─> the user's Parquet in the instance cache ─> DataFusion
```

One image, one binary; the job and the web differ only in their arguments. Everything runs in one region, `europe-west1` (Belgium). Frankfurt (`europe-west3`) would be closer, but it has no Cloud Run domain mapping.

## Deployment

```bash
deploy/setup.sh    # once / idempotent: APIs, bucket, KMS key, SAs, IAM, session-key
deploy/deploy.sh   # build (Cloud Build), job + scheduler, web
```

Every push to `main` is deployed by GitHub Actions ([.github/workflows/deploy.yml](../.github/workflows/deploy.yml)) running `deploy/deploy.sh`. The workflow can also be run by hand in the Actions tab (Run workflow). It authenticates without a key through Workload Identity Federation (pool `github`) as `fitdata-deployer`; only the workflow from `jsedlacek/fitdata` on `main` gets a token. `setup.sh` sets up the pool and the roles. The image is rebuilt only when `Dockerfile*`, `.dockerignore`, `Cargo.*`, `src/`, `crates/` or `frontend/` change, so a docs-only push just redeploys the current image. In Actions the binary and the dashboard are built right on the runner (`ubuntu-24.04`) and [Dockerfile.ci](../Dockerfile.ci) only packs them into a distroless image (Debian 13, for glibc) that is pushed to Artifact Registry; `deploy.sh` then finds it and skips Cloud Build. [rust-cache](https://github.com/Swatinem/rust-cache) keeps compiled crates in the GitHub Actions cache, so a change to one dependency recompiles only it and what depends on it. A local `deploy.sh` still builds the full `Dockerfile` (cargo-chef) in Cloud Build, without a cache.

Settings are in [deploy/config.sh](../deploy/config.sh). Table columns are the fields of the row structs in [src/model.rs](../src/model.rs) (`#[derive(Columns)]`, a column's description is the field's doc comment); a new column only needs adding there and a push.

The OAuth Web client must list `https://fitdata.jakub.contact/auth/callback` among its *Authorized redirect URIs*.

### Custom domain

- The domain must be verified for the account `gcloud` runs as (`gcloud domains list-user-verified`). If it is not, verify it with `gcloud domains verify jakub.contact`.
- `deploy.sh` creates the domain mapping and prints the DNS record: `CNAME fitdata → ghs.googlehosted.com`.
- In Cloudflare the record must be **DNS only** (grey cloud). Behind the proxy, Google will not issue a certificate.
- The certificate is issued 15–60 minutes after DNS is set. `gcloud beta run domain-mappings describe --domain fitdata.jakub.contact --region europe-west1` shows its state.

## Users

Any account in `ALLOWED_EMAILS` (invites, `deploy/config.sh`) can sign in. A user is a Google account, keyed by its `sub`. Sign-in asks for the Health scopes too (offline access), so a new user signs in and connects Fitbit on one Google consent screen. The first sign-in creates the record `users/{uid}.json`; when Google returns a refresh token with Health scopes (on the first consent, or a forced one), the web encrypts it with the KMS key, stores it in the record and starts the job for this user, which downloads the whole history. A returning user goes straight through without a consent screen and keeps the stored token. If the user unticked the Health scopes, the dashboard offers **Connect Fitbit**, which forces the consent screen (as does **Restore access to data**). An uninvited account's grant is revoked right away. Data is never shared between users.

**Deleting an account** (account menu on the dashboard) marks the record as deleted and starts the job. The job revokes the token at Google and deletes `u/{uid}/` and the record. The web itself must not delete data.

To add an existing user with a token (e.g. when migrating):

```bash
cargo run -- import-user --user <sub> --email <e-mail> --refresh-token <sm://secret | file>
```

## Data

```
users/{uid}.json                           profile, time zone, encrypted refresh token
u/{uid}/raw/{type}/{day|month}.jsonl        Health API responses, unchanged
u/{uid}/data/{table}/{YYYY-MM}.parquet      heart_rate, activity, spo2, hrv (a month per file)
u/{uid}/data/{table}.parquet                daily, nights, exercises
u/{uid}/data/manifest.json                  files with their generation; the job writes it last
u/{uid}/status.json                         last sync run, last successful one, whether one is running
```

- **Raw:** every run rewrites the last 7 days, because the watch syncs with a delay. A new user first gets the whole history (backfill).
- **Tables:** `fitdata load` replaces the days in its range in the files and keeps the rest, so running it again duplicates nothing. Columns are the fields of the row structs in `model.rs`; a new column only needs adding there and reads as NULL in older files.
- **Reading:** the web keeps users' files in the instance cache (`/tmp`, which is RAM, at most 256 MB) and, going by `manifest.json`, downloads only changed files and only the months a request needs. Queries run in DataFusion over tables registered for that user only ([src/warehouse.rs](../src/warehouse.rs), [src/engine.rs](../src/engine.rs)).
- **A night** (`nights`) belongs to the day the user woke up and merges all sleeps that ended before 14:00.
- **Time:** time columns are instants, stored in UTC in Parquet. The local date (`date`) and local times are computed in the user's time zone (`users/{uid}.json`, also written to `manifest.json`), not from the API's `utcOffset`: for HRV that holds for the whole night, by its end, so on the night the clocks change it is an hour off. Queries see times in the user's zone, with the offset (`2026-10-25T02:30:00+01:00`); the hour repeated when clocks go back stays two hours and nothing is merged. Changing a column's type requires reloading all days (`fitdata load` from the first day with data), or old files would be read wrong.

## Security

| Identity | May |
|---|---|
| `fitdata-job` | read `oauth-client`, write to the bucket, encrypt and decrypt with the `refresh-token` key (KMS), write spans to Cloud Trace |
| `fitdata-web` | read `oauth-client` and `session-key`, read the bucket, write only `users/` (an IAM condition), only encrypt with the `refresh-token` key, run the job (also with arguments for one user) and read its state, write spans to Cloud Trace |
| `fitdata-scheduler` | run the job |
| `fitdata-deployer` | deploy the job, scheduler and web (GitHub Actions), read the project |

The web is public on Cloud Run. The home page (`/` for visitors who aren't signed in), `/privacy`, `/terms`, the icons and the build assets need no sign-in; every other page and API call requires Google sign-in with an e-mail from `ALLOWED_EMAILS`. The session is an HMAC-signed cookie (key `session-key`, valid for 30 days) with the user ID; every request reads only that user's data.

Refresh tokens are encrypted in the user records with the Cloud KMS key `fitdata/refresh-token`, and the user ID is bound to the ciphertext (additional authenticated data), so a token copied to another user cannot be decrypted. The web encrypts the token but cannot read it.

## MCP server

`https://fitdata.jakub.contact/mcp` is an MCP server (Streamable HTTP) with read-only tools over the signed-in user's data: `get_daily_metrics`, `get_day`, `get_sleep`, `get_exercises`, `get_schema` and `query`. The code is in [src/mcp.rs](../src/mcp.rs).

`query` is a single SELECT in DataFusion SQL (close to PostgreSQL), at most 1000 rows. The per-minute tables (`heart_rate`, `activity`, `spo2`, `hrv`) hold only the days `from`..`to` (by default the last 90 days, at most 400); the others are complete. The code is in [src/query.rs](../src/query.rs):

- A query gets its own session in which only the signed-in user's tables are registered. DataFusion has no SQL that reaches files or the network (paths and URLs as tables, or functions like `read_csv`, do not exist), and DDL, DML and `SET` are disabled, so neither `CREATE EXTERNAL TABLE` nor `COPY TO` gets through. A query sees neither other users' data nor the server.
- Against overload: queries run on their own runtime with two threads (they cannot block the dashboard), at most two at once, with a 256 MB limit on query state (no spilling to disk) and a 30 s timeout.
- Tests in `query.rs` try paths and URLs as tables, `CREATE EXTERNAL TABLE`, `COPY`, `INSERT`, `DROP`, `SET`, `PREPARE`, several statements at once and the memory limit.

Connecting:

- **Claude:** Settings → Connectors → Add custom connector, URL `https://fitdata.jakub.contact/mcp`.
- **ChatGPT:** Settings → Apps & Connectors → Advanced → Developer mode, then Create, the same URL, OAuth authentication.

The dashboard has the instructions in the account menu (**Connect Claude / ChatGPT**) and in the footer. A browser on `/mcp` (GET with `Accept: text/html`) gets a page with the instructions; MCP clients still get 405 on GET, because we do not offer an SSE stream.

A client registers itself (dynamic registration), sends the user to Google sign-in (the same `ALLOWED_EMAILS`) and to a consent page. Tokens carry the user ID; tokens issued before multi-user support lack it and the user is looked up by e-mail. The authorization server is in [src/authz.rs](../src/authz.rs) and stores nothing: client IDs, codes and tokens are signed with the `session-key` key. An access token is valid for an hour, a refresh token for 30 days from its last refresh. To revoke access for all clients and all signed-in browsers at once: a new version of `session-key` and a new deployment of the web.

When a refresh token stops working (access revoked), the dashboard shows the user and a click on **Restore access to data** is enough. For the job it is not an error (the user's run ends with the state `auth_error`), and the alert does not report it.

## Operations

A sync can also be started by hand with the ⟳ icon next to "Synced" in the dashboard header. The web runs the job only for the signed-in user (`job --user <uid>` through the Cloud Run Admin API, `POST /api/sync`); the icon spins until the user's run (or the job's last execution) finishes, and then the page reloads its data.

The job processes up to 8 users at once, and one failing does not bring down the others. With more users the job can run in several tasks (`--tasks N`); users are split by a hash of their ID (`CLOUD_RUN_TASK_INDEX`).

**The Health API quota** is 300 requests per minute per user (a sliding window; a 429 response does not say how long to wait) and 120,000 per minute per project. Therefore ([src/job.rs](../src/job.rs), [src/api.rs](../src/api.rs)):

- Only one process syncs a user at a time: the lock `u/{uid}/lock` is created atomically (GCS `ifGenerationMatch=0`), a second run skips the user, and a lock older than 45 min is taken over.
- A rate limiter (`governor`, GCRA) guards one user's requests: a burst of 40, then 240 a minute, so at most 280 in any minute. Should a 429 come anyway, the request waits 2–5 s.
- The hourly run downloads only yesterday and today and skips data types for which the user has no data at all. Once every 20 h the run is full: the whole 7-day window and one query per type to see whether it has data (`status.json`: `last_full`, `absent`). Days before the account was created are not downloaded (`since`).
- On your account: the hourly run makes 51 requests, a full one 121 (it used to be 173 every hour).

Every failed job run (an error for any user other than a revoked token) opens the Cloud Monitoring incident **fitdata-sync failed** and sends an e-mail to `ALERT_EMAIL` (`deploy/config.sh`). A lasting failure sends one e-mail, not one per hour. Once runs succeed again, the incident closes itself. The policy is in [deploy/alert-policy.json](../deploy/alert-policy.json) and `setup.sh` sets it up.

A second policy, **fitdata-web errors** ([deploy/alert-web-errors.json](../deploy/alert-web-errors.json)), sends an e-mail when the web logs an error or answers with a 5xx, at most once an hour.

### Logs, traces and errors

Configured in [src/telemetry.rs](../src/telemetry.rs).

- **Logs:** on Cloud Run (`K_SERVICE` or `CLOUD_RUN_JOB`) the binary writes JSON lines in the Cloud Logging format: `severity`, `message`, the source line and the fields of the event and its parent spans (e.g. `data_type`, `table`, `http.route`) in `jsonPayload`. Locally the output is readable text. The format can be forced with `LOG_FORMAT=json` or `LOG_FORMAT=text`, the level with `RUST_LOG`.
- **Traces:** spans are sent through OpenTelemetry (OTLP) to Cloud Trace (Telemetry API `telemetry.googleapis.com`). Every HTTP request to the web is one trace, continuing Cloud Run's trace (`traceparent`). Every job run is one `sync job` trace with spans for users, data types, Health API calls, GCS reads and writes, and table writes.
- **Correlation:** every log carries `logging.googleapis.com/trace` and `spanId`. In Logs Explorer a request's logs are grouped under its request log, and Cloud Trace shows a span's logs.
- **Error Reporting:** ERROR-level logs (panics included) have the type `ReportedErrorEvent` and the image version (`APP_VERSION`), so [Error Reporting](https://console.cloud.google.com/errors?project=fitdata-jsedlacek) groups them.

```bash
gcloud run jobs execute fitdata-sync --region europe-west1 --wait     # run the job now
gcloud run jobs executions list --job fitdata-sync --region europe-west1
gcloud logging read 'resource.type="cloud_run_job"' --limit 50 --format 'value(severity, jsonPayload.message)'
```

To reload an older range for a user (e.g. after a normalization change), locally with ADC:

```bash
STORAGE=gs://fitdata-jsedlacek-data cargo run -- load --user <uid> --from 2026-10-21
```
