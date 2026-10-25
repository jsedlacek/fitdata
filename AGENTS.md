# AGENTS.md

Instructions for AI agents working in this repo.

## Git

- After every commit, **always push automatically** to `origin main` (`git push origin main`); no need to ask.
- Work directly on `main`, without feature branches or PRs, unless the user says otherwise.
- Commit messages in English, a short subject + a body with the reason for the change.
- Everything in the repo (code, comments, docs, UI) is in English.

## Project

Downloads data from Fitbit Air through the Google Health API v4 and shows it on a dashboard.

- Rust binary `fitdata` (`src/`): CLI, hourly job and API in one.
- The Google Cloud clients (KMS, Secret Manager, GCS) live in the workspace crate `crates/gcp` (`fitdata-gcp`), so their large code is not recompiled with every change to `fitdata`. Its async methods return `BoxFuture`, not `async fn` (an `async fn` body is compiled in the crate that awaits it); keep it that way for new ones too.
- Multiple users (invite only, `ALLOWED_EMAILS`), a user = Google `sub`; each user's data in GCS under `u/{sub}/` as raw JSON and Parquet, read through DataFusion. One user's data must never be visible to another.
- MCP server for Claude/ChatGPT at `/mcp` (`src/mcp.rs`) with its own OAuth server (`src/authz.rs`) on top of Google sign-in.
- Dashboard in `frontend/` (React, Vite, Tailwind, shadcn-style, TanStack, ECharts); `fitdata serve` serves its `dist`. UI text is English only; dates and numbers are formatted as `en-GB` (24-hour clock).
- Production on Google Cloud, all in `europe-west1`: Cloud Run job `fitdata-sync` (every hour), Cloud Run service `fitdata-web` at https://fitdata.jakub.contact, GCS `fitdata-jsedlacek-data`, KMS key `fitdata/refresh-token`. Description: [docs/production.md](docs/production.md).
- A push to `main` deploys to production automatically (GitHub Actions, `.github/workflows/deploy.yml`); after pushing, check the run with `gh run watch`.
- Deployment: `deploy/setup.sh` (infrastructure, idempotent) and `deploy/deploy.sh` (build + deploy). Settings in `deploy/config.sh`; table columns are the fields of the row structs in `src/model.rs` (`#[derive(Columns)]` from `crates/derive`).
- Secrets live only in Secret Manager (`oauth-client`, `session-key`), and users' refresh tokens only KMS-encrypted in `users/{sub}.json`; never print them or write them to files.

## Before committing

```bash
cargo fmt && cargo clippy --all-targets && cargo test
cd frontend && npm run build   # typecheck + build of the dashboard
```

Check dashboard changes in a browser (headless Chromium through Playwright in the scratchpad), not just by compiling.
