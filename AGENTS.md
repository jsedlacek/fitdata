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
- Multiple users (open sign-up), a user = Google Health user ID (`healthUserId`; sign-in asks for no `openid`/`email`, so we never know names or e-mails); each user's data in GCS under `u/{uid}/` as raw JSON and Parquet, read through DataFusion. One user's data must never be visible to another.
- MCP server for Claude/ChatGPT at `/mcp` (`src/mcp.rs`) with its own OAuth server (`src/authz.rs`) on top of Google sign-in.
- The product is called **Fitdata** (capital F) in all user-facing text; `fitdata` stays for the binary, crate, commands and resource names.
- Dashboard in `frontend/` (React, Vite, Tailwind, shadcn-style, TanStack; charts are our own SVG kit in `frontend/src/components/chart/kit.tsx`); `fitdata serve` serves its `dist`: the home page on `/`, the dashboard on `/dash`, a public demo on made-up data on `/demo`. UI text is English only; dates and numbers are formatted as `en-GB` (24-hour clock).
- User docs at `/docs` (public; `frontend/docs.html`, pages in `frontend/src/docs/pages/`, ⌘K search indexed in the browser). The dashboard's (?) icons explain a metric in a sentence and link there (`frontend/src/lib/help.ts`). When a metric, chart or behavior changes, update its docs page and help text too.
- Production on Google Cloud, all in `europe-west1`: Cloud Run job `fitdata-sync` (every hour), Cloud Run service `fitdata-web` at https://fitdata.jakub.contact, GCS `fitdata-jsedlacek-data`, KMS key `fitdata/refresh-token`. Description: [docs/production.md](docs/production.md).
- A push to `main` deploys to production automatically (GitHub Actions, `.github/workflows/deploy.yml`); after pushing, check the run with `gh run watch`.
- Deployment: `deploy/setup.sh` (infrastructure, idempotent) and `deploy/deploy.sh` (build + deploy). Settings in `deploy/config.sh`; table columns are the fields of the row structs in `src/model.rs` (`#[derive(Columns)]` from `crates/derive`).
- Secrets live only in Secret Manager (`oauth-client`, `session-key`), and users' refresh tokens only KMS-encrypted in `users/{uid}.json`; never print them or write them to files.

## Before committing

```bash
cargo fmt && cargo clippy --all-targets && cargo test
cd frontend && npm run build   # typecheck + build of the dashboard
```

Check dashboard changes in a browser (headless Chromium through Playwright in the scratchpad), not just by compiling.
