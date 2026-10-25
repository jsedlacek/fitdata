# Plan: multiple users, Parquet + DuckDB

> **Done (2026-10-24).** Compared with the plan: DataFusion instead of DuckDB (Rust, SQL with no path to files or the network, a simpler build; comparison below), so the MCP `query` runs right in the web on its own runtime instead of a separate service. Refresh tokens are encrypted with a symmetric KMS key with separate roles (the web only encrypts, the job also decrypts) instead of an asymmetric one. The rollout took three steps instead of seven. The current description is in [production.md](production.md).
>
> Comparison on a year of data (2 threads): day page DataFusion 20 ms, DuckDB 27 ms; yearly heart rate aggregation 43 ms and 65 ms. DataFusion rejected every attempt to read files, reach the network, or run DDL, DML and `SET` already at planning.

Goal: turn the single-user fitdata into a service for several people (invite only for now, but ready for wider use) and replace BigQuery with Parquet in GCS read through DuckDB.

## Decisions

- **User** = Google `sub` (not the e-mail). Data is never shared between users.
- **Access** through `ALLOWED_EMAILS` (invites) for now; the first sign-in of an allowed e-mail creates the user.
- **Data:** Parquet in GCS per user, no BigQuery. No analytics across users is needed.
- **Reading:** DuckDB in the web for the dashboard and the structured MCP tools (we write the SQL).
- **MCP `query`** (foreign SQL) runs in a separate Cloud Run service `fitdata-query` whose SA has no roles. The service gets only the given user's files in the request and has no keys, no other data and no permissions, so even with a bug in DuckDB an attacker sees at most their own data. The DuckDB settings are a second layer, not the boundary.
- **Refresh tokens** encrypted with an asymmetric Cloud KMS key: the web encrypts with the public key, only `fitdata-job` may decrypt.
- **The owner's existing data** moves under their ID (just like for the other users).
- The OAuth app is *In production*, so refresh tokens do not expire after 7 days.

Measurements (Cloud Run europe-west1, 1 vCPU, 512 MiB, a year of data): day page in BigQuery 309 ms (median), in DuckDB with a warm cache 57 ms including the GCS change check, with a cold cache 112 ms. Yearly heart rate aggregation (10 M rows) in DuckDB 206 ms. A year of heart rate is 48.5 MB of Parquet.

## GCS layout

```
users/{uid}.json                           profile: e-mail, time zone, encrypted refresh token
u/{uid}/raw/{type}/{day|month}.jsonl        Health API responses (today raw/{type}/…)
u/{uid}/data/{table}/{YYYY-MM}.parquet      heart_rate, activity, spo2, hrv
u/{uid}/data/{table}.parquet                daily, nights, exercises (small, one file)
u/{uid}/data/manifest.json                  list of files with their generation, written last
u/{uid}/status.json                         last sync run: time, ok, auth_error, error, running
```

Everything about a user except the registry record is under one prefix `u/{uid}/`, so deleting an account is deleting the prefix.

`manifest.json` gives the web a consistent snapshot (the job rewrites it only after all files) and one change check per request instead of one per file.

## Phases

Each phase can be deployed on its own (push to `main` = production) and production works between phases.

### Phase 1: writing Parquet (next to BigQuery)

One user, but the paths already by `uid`, so the data is not migrated twice.

- `OWNER_ID` (the owner's Google `sub`) in `deploy/config.sh` and the job's env.
- New module `src/parquet.rs` (writing through DuckDB in the job, no other dependency):
  - rows from `model::table_rows` → DuckDB Appender → `COPY … TO … (FORMAT parquet, COMPRESSION zstd)` → `Store::put`;
  - only the months in the run's range are rewritten: the existing file without the days in the range `UNION ALL` the new rows;
  - finally `manifest.json`.
- `fitdata load` writes to both BigQuery and Parquet, so one run fills the whole history.
- `duckdb` crate with `bundled`; check the build time in Actions (cargo-chef) and that `distroless/cc` has `libstdc++`.
- Tests: writing and merging a month on a local `Store` (a range in the middle of a month, across a month boundary, an empty range).
- Done when: the files exist in production and a script comparing counts and aggregates by day and table with BigQuery finds no difference.

### Phase 2: reading through DuckDB (web and MCP)

- New module `src/warehouse.rs`:
  - cache of the user's files in `/tmp/fitdata/{uid}/` (on Cloud Run that is RAM, hence a size limit with LRU, for example 128 MB);
  - on every request it loads `manifest.json` and downloads only changed files;
  - queries through `spawn_blocking`, results into the existing serde structs (`DailyMetrics`, `NightRow`, `HrMinute`, …), probably through `serde_arrow`, to keep an API like `query_as`.
- `web.rs`: `day_detail` and `overview` rewritten in DuckDB SQL.
- `mcp.rs`: `get_daily_metrics`, `get_day`, `get_sleep`, `get_exercises` and `get_schema` (`DESCRIBE`) over DuckDB; switch the tool descriptions to the DuckDB dialect.
- **MCP `query` in a separate service `fitdata-query`** (the same binary, command `fitdata query-serve`):
  - A Cloud Run service with the SA `fitdata-query`, which **has no roles** (`setup.sh` creates it and checks it has no binding in the project or bucket IAM). No secrets and no env with configuration.
  - Only `fitdata-web` may call it (`roles/run.invoker` for it alone, no `allUsers`); the web proves itself with an ID token from the metadata server with the service URL as audience. Ingress stays `all`, because `internal` would need Direct VPC egress for the web; IAM holds the boundary.
  - `concurrency=1`: an instance serves only one query at a time, so different users' data never meet in memory or in `/tmp`. Further `--memory 1Gi`, `--cpu 1`, `--timeout 60`, `--max-instances` as a cost cap (when exhausted, the web returns the tool an error "try later").
  - Request: the SQL and the user's Parquet files from the web's cache (multipart). A year of heart rate is about 50 MB, more than Cloud Run's 32 MiB limit for HTTP/1, hence HTTP/2 end to end (`--use-http2`, h2c in the service, `http2_prior_knowledge` in the web's client).
  - The service writes the files to `/tmp/q-<random>/`, creates views over them and then locks DuckDB down: `allowed_directories=[that directory]`, `enable_external_access=false`, `autoload_known_extensions=false`, `autoinstall_known_extensions=false`, `allow_community_extensions=false`, `memory_limit=768MB`, `threads=1`, `lock_configuration=true`. After the query it deletes the directory.
  - Only one `SELECT` statement (statement type from `extract_statements`), at most 1000 rows and a limited response size, a 30 s timeout by interrupting the query.
  - Logs only to stdout (Cloud Run collects them without permissions); the service sends no spans to Cloud Trace, since it would need a role for that. The span of the call in the web captures the query's duration and errors.
  - The service's cold start (around a second) shows only for `query`; `min-instances` stays at 0.
- **Service tests** (`cargo test`), all must end in an error: a file outside the request's directory, `read_csv('/etc/passwd')`, `read_text`, `read_blob`, the glob `/**`, `COPY … TO`, `ATTACH`, `INSTALL` and `LOAD`, `SET` and `PRAGMA`, `CREATE`, `getenv()`, several statements at once, a query over the memory limit and over the timeout. These are second-layer tests; the boundary is the service with no permissions and no other data.
- Probably raise the web's memory to 1 GiB (depending on production measurements).
- Done when: the dashboard is checked in a browser (Playwright), the MCP tools including `query` work from Claude, a call to `fitdata-query` without the web's ID token ends with 403, and the latency in Cloud Trace matches the measurements.

### Phase 3: removing BigQuery

- `u/{uid}/status.json` replaces `sync_runs`; the run history stays in the log.
- Delete `bq.rs`, `google-cloud-bigquery-v2`, `deploy/schema/`, `deploy/schema.sh`, the staging in the bucket and the BigQuery roles in IAM (`setup.sh`).
- Delete the `health` dataset by hand only after confirmation (the source of truth is the raw data and Parquet).
- Done when: nothing of BigQuery is left in the code or IAM and both the job and the web work.

### Phase 4: users and tokens

- **Identity:** `sub` from the ID token. `Claims::Session`, `Consent`, `Code`, `Access` and `Refresh` carry `uid` (and the e-mail only for display). After the deployment everyone signs in again and MCP clients authorize again (old tokens do not parse in the new format).
- **Registry:** `users/{uid}.json`; the first sign-in of an e-mail from `ALLOWED_EMAILS` creates it. A later step for wider use: invites in GCS instead of env.
- **KMS:** keyring `fitdata`, key `refresh-token` (`ASYMMETRIC_DECRYPT`, `RSA_DECRYPT_OAEP_3072_SHA256`).
  - web: `cloudkms.cryptoKeyVersions.viewPublicKey`, the public key cached, encrypts locally (`rsa` crate);
  - job: `roles/cloudkms.cryptoKeyDecrypter`; decrypts through the KMS REST API;
  - when Google returns a new refresh token, the job encrypts and stores it itself.
- **Connecting Fitbit:** the existing `connect_fitbit` flow stores the encrypted token in the user's record and starts the job for them (history from `membership_start`).
- **Owner migration:** a one-time command moves the token from the `refresh-token` secret to `users/{OWNER_ID}.json` and `raw/{type}/…` to `u/{OWNER_ID}/raw/{type}/…`. The `refresh-token` secret and its IAM are deleted only after confirmation.
- `fitdata login` (CLI) stores the token for the given user, or we drop it.

### Phase 5: the job for all users

- `fitdata job` walks `users/` and processes the users with a token; one failing does not bring down the others.
- `AuthError` is a user state (`needs_reconnect` in `status.json`), not a job error. The job ends with an error (and the alert fires) only on another error.
- `--user <uid>` for a manual sync: the web starts an execution with overridden arguments; "running" is read from `status.json`, not from the list of executions.
- "Today" by the user's time zone (`users/{uid}.json`, by default `Europe/Prague`), not by the container's `TZ`.
- Scaling: users split between the job's tasks by `hash(uid) % CLOUD_RUN_TASK_COUNT`; 1 task for now, but the code is ready. Health API retry and backoff already exist.

### Phase 6: web and MCP for multiple users

- Every handler takes `uid` from the session or the bearer token; `warehouse` reads only that user's directory and `fitdata-query` gets only their files.
- Dashboard: the signed-in e-mail and sign-out, a "no data / first load in progress" state, a reconnect banner based on `status.json`.
- MCP instructions with the user's time zone instead of a hard-coded `Europe/Prague`.
- Deleting an account: revokes the token at Google and deletes `users/{uid}.json` and the whole `u/{uid}/` prefix.
- Test: two users, each sees only their own data (web, MCP tools, MCP `query`).

### Phase 7: cleanup and documentation

- `docs/production.md` (diagram, IAM table including `fitdata-query` with no roles, operations), `docs/setup.md`, `AGENTS.md`, `README.md`.
- Delete the benchmark resources: the `bench` dataset, `gs://fitdata-jsedlacek-data/bench/`, the `fitdata-bench` job, the `fitdata/bench` image.

## Risks

- **MCP `query`:** the boundary is the `fitdata-query` service with no permissions, which gets only one user's data. If someone accidentally gave it a role or a secret, the boundary would be gone, so `setup.sh` checks it on every run. A bug in the DuckDB settings or in DuckDB itself endangers at most the data of the user who sent the query.
- **Instance memory:** both the web's cache in `/tmp` and DuckDB live in RAM; the cache limit has to be set together with the Cloud Run memory. For `fitdata-query`, the request's files and `memory_limit` must fit in 1 GiB; for users with many years of history it will need sending only the months in the query's range, or more memory.
- **Cold start:** a user's first request after an instance starts downloads files (about 110 ms for a day, more for queries over the whole history).
- **Health API quotas** with many users: find the limits in the console and possibly spread users over the hour.
