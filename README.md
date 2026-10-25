# fitdata

Downloads data from Fitbit (Fitbit Air) through the [Google Health API v4](https://developers.google.com/health) and stores it for analysis.

The Fitbit Web API shuts down on 30 October 2026; this uses its replacement, the Google Health API.

## Status

Runs in production on Google Cloud for multiple users (invite only): an hourly Cloud Run job downloads each user's data to GCS as raw JSON and Parquet tables, and the dashboard and MCP server read it through DataFusion behind Google sign-in. See [docs/production.md](docs/production.md).

## Usage

First set up the GCP project and the OAuth client: [docs/setup.md](docs/setup.md). The OAuth client lives in Secret Manager; locally it is read through a signed-in `gcloud`. The commands work on one user (`--user` is their Google `sub`):

```bash
cargo run -- login --user <sub> --email <e-mail>   # OAuth consent, stores the user's refresh token
cargo run -- whoami --user <sub>     # identity, profile, paired devices
cargo run -- probe --user <sub>      # which data types are available and have data
cargo run -- backfill --user <sub>   # the whole history (can be interrupted and run again)
cargo run -- sync --user <sub>       # the last 7 days
cargo run -- load --user <sub> --from 2026-10-21   # raw data -> Parquet tables
cargo run -- job                     # sync + load of the last 7 days for all users (the hourly job)
cargo run -- serve --dev-user <sub>  # API + dashboard + MCP, locally without sign-in
```

Both `backfill` and `sync` take `--types heart-rate,sleep,...` to download only selected types.

## Stored data

```
data/
  users/{sub}.json                          # profile and encrypted refresh token
  u/{sub}/raw/{data-type}/{YYYY-MM-DD}.jsonl   # intraday types (heart rate, steps, HRV, ...)
  u/{sub}/raw/{data-type}/{YYYY-MM}.jsonl      # sparse, daily and session types (sleep, exercise, ...)
  u/{sub}/data/{table}[/{YYYY-MM}].parquet     # normalized tables (src/model.rs)
  u/{sub}/status.json                       # last sync
```

Each line holds one data point exactly as the API returned it. The list of types and how they are filtered is in [src/datatypes.rs](src/datatypes.rs).

## Dashboard (frontend/)

React + Vite + TypeScript, Tailwind v4, shadcn/ui-style components (Radix), TanStack Router and Query, ECharts charts.

```bash
cd frontend && npm install
npm run dev     # http://devbox.test:5173, /api and /auth go to `fitdata serve` (port 8080)
npm run build   # frontend/dist, served by `fitdata serve` (STATIC_DIR)
```

Locally, `fitdata serve` reads data from `STORAGE` (a directory or `gs://…`) and the OAuth client from Secret Manager (ADC). Without `gcloud auth application-default login`,
`GOOGLE_APPLICATION_CREDENTIALS=~/.config/gcloud/legacy_credentials/<account>/adc.json` is enough.

## License

[MIT](LICENSE). The hosted service at https://fitdata.jakub.contact has its own [privacy policy](https://fitdata.jakub.contact/privacy) and [terms](https://fitdata.jakub.contact/terms).
