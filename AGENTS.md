# AGENTS.md

Pokyny pro AI agenty pracující v tomto repu.

## Git

- Po každém commitu **vždy automaticky pushni** do `origin main` (`git push origin main`); není potřeba se ptát.
- Pracuje se přímo na `main`, bez feature větví a PR, pokud uživatel neřekne jinak.
- Commit message anglicky, stručný předmět + tělo s důvodem změny.

## Projekt

Stahuje data z Fitbit Air přes Google Health API v4 a ukazuje je na dashboardu.

- Rust binárka `fitdata` (`src/`): CLI, denní job i API v jednom.
- Dashboard v `frontend/` (React, Vite, Tailwind, shadcn-style, TanStack, ECharts); `fitdata serve` servíruje jeho `dist`.
- Produkce na Google Cloud, vše v `europe-west1`: Cloud Run job `fitdata-sync` (každou hodinu), Cloud Run service `fitdata-web` na https://fitdata.jakub.contact, GCS `fitdata-jsedlacek-data`, BigQuery dataset `health`. Popis: [docs/production.md](docs/production.md).
- Nasazení: `deploy/setup.sh` (infrastruktura, idempotentní) a `deploy/deploy.sh` (build + nasazení). Nastavení v `deploy/config.sh`, schémata tabulek v `deploy/schema/`.
- Tajemství jsou jen v Secret Manageru (`oauth-client`, `refresh-token`, `session-key`); nikdy je nevypisuj ani neukládej do souborů.

## Než commitneš

```bash
cargo fmt && cargo clippy --all-targets && cargo test
cd frontend && npm run build   # typecheck + build dashboardu
```

Změny dashboardu ověř v prohlížeči (headless Chromium přes Playwright ve scratchpadu), ne jen kompilací.
