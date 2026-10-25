# AGENTS.md

Pokyny pro AI agenty pracující v tomto repu.

## Git

- Po každém commitu **vždy automaticky pushni** do `origin main` (`git push origin main`); není potřeba se ptát.
- Pracuje se přímo na `main`, bez feature větví a PR, pokud uživatel neřekne jinak.
- Commit message anglicky, stručný předmět + tělo s důvodem změny.

## Projekt

Stahuje data z Fitbit Air přes Google Health API v4 a ukazuje je na dashboardu.

- Rust binárka `fitdata` (`src/`): CLI, hodinový job i API v jednom.
- Google Cloud klienti (KMS, Secret Manager, GCS) jsou ve workspace crate `crates/gcp` (`fitdata-gcp`), aby se jejich velký kód nepřekládal s každou změnou `fitdata`. Jeho async metody vrací `BoxFuture`, ne `async fn` (tělo `async fn` se kompiluje v crate, který ho awaituje); tak to drž i u nových.
- Víc uživatelů (na pozvánku, `ALLOWED_EMAILS`), uživatel = Google `sub`; data každého v GCS pod `u/{sub}/` jako raw JSON a Parquet, čtená přes DataFusion. Data jednoho uživatele nikdy nesmí uvidět jiný.
- MCP server pro Claude/ChatGPT na `/mcp` (`src/mcp.rs`) s vlastním OAuth serverem (`src/authz.rs`) nad Google přihlášením.
- Dashboard v `frontend/` (React, Vite, Tailwind, shadcn-style, TanStack, ECharts); `fitdata serve` servíruje jeho `dist`.
- Produkce na Google Cloud, vše v `europe-west1`: Cloud Run job `fitdata-sync` (každou hodinu), Cloud Run service `fitdata-web` na https://fitdata.jakub.contact, GCS `fitdata-jsedlacek-data`, KMS klíč `fitdata/refresh-token`. Popis: [docs/production.md](docs/production.md).
- Push do `main` automaticky nasadí do produkce (GitHub Actions, `.github/workflows/deploy.yml`); po pushi zkontroluj běh přes `gh run watch`.
- Nasazení: `deploy/setup.sh` (infrastruktura, idempotentní) a `deploy/deploy.sh` (build + nasazení). Nastavení v `deploy/config.sh`; sloupce tabulek jsou pole row structů v `src/model.rs` (`#[derive(Columns)]` z `crates/derive`).
- Tajemství jsou jen v Secret Manageru (`oauth-client`, `session-key`) a refresh tokeny uživatelů jen zašifrované KMS v `users/{sub}.json`; nikdy je nevypisuj ani neukládej do souborů.

## Než commitneš

```bash
cargo fmt && cargo clippy --all-targets && cargo test
cd frontend && npm run build   # typecheck + build dashboardu
```

Změny dashboardu ověř v prohlížeči (headless Chromium přes Playwright ve scratchpadu), ne jen kompilací.
