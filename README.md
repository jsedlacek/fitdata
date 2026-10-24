# fitdata

Stahuje data z Fitbitu (Fitbit Air) přes [Google Health API v4](https://developers.google.com/health) a ukládá je pro analýzu.

Fitbit Web API se vypíná 30. 10. 2026; tohle používá jeho náhradu, Google Health API.

## Stav

Běží v produkci na Google Cloud pro víc uživatelů (na pozvánku): hodinový Cloud Run job stahuje data každého uživatele do GCS jako raw JSON a Parquet tabulky a dashboard s MCP serverem je čte přes DataFusion za přihlášením Googlem. Popis je v [docs/production.md](docs/production.md).

## Použití

Nejdřív nastav GCP projekt a OAuth klienta: [docs/setup.md](docs/setup.md). OAuth client je v Secret Manageru, lokálně se k němu přistupuje přes přihlášený `gcloud`. Příkazy pracují s jedním uživatelem (`--user` je jeho Google `sub`):

```bash
cargo run -- login --user <sub> --email <e-mail>   # OAuth souhlas, uloží refresh token uživatele
cargo run -- whoami --user <sub>     # identita, profil, spárovaná zařízení
cargo run -- probe --user <sub>      # které datové typy jsou dostupné a mají data
cargo run -- backfill --user <sub>   # celá historie (lze přerušit a pustit znovu)
cargo run -- sync --user <sub>       # posledních 7 dní
cargo run -- load --user <sub> --from 2026-10-21   # raw data -> Parquet tabulky
cargo run -- job                     # sync + load posledních 7 dní všech uživatelů (hodinový job)
cargo run -- serve --dev-user <sub>  # API + dashboard + MCP, lokálně bez přihlášení
```

`backfill` i `sync` berou `--types heart-rate,sleep,...`, aby šlo stahovat jen vybrané typy.

## Uložená data

```
data/
  users/{sub}.json                          # profil a šifrovaný refresh token
  u/{sub}/raw/{data-type}/{YYYY-MM-DD}.jsonl   # intraday typy (tep, kroky, HRV, ...)
  u/{sub}/raw/{data-type}/{YYYY-MM}.jsonl      # řídké, denní a session typy (spánek, cvičení, ...)
  u/{sub}/data/{tabulka}[/{YYYY-MM}].parquet   # normalizované tabulky (schema/)
  u/{sub}/status.json                       # poslední synchronizace
```

Každý řádek obsahuje jeden data point přesně tak, jak ho vrátilo API. Seznam typů a způsob jejich filtrování je v [src/datatypes.rs](src/datatypes.rs).

## Dashboard (frontend/)

React + Vite + TypeScript, Tailwind v4, komponenty ve stylu shadcn/ui (Radix), TanStack Router a Query, grafy ECharts.

```bash
cd frontend && npm install
npm run dev     # http://devbox.test:5173, /api a /auth jdou na `fitdata serve` (port 8080)
npm run build   # frontend/dist, který servíruje `fitdata serve` (STATIC_DIR)
```

`fitdata serve` lokálně čte data z `STORAGE` (adresář nebo `gs://…`) a OAuth client ze Secret Manageru (ADC). Bez `gcloud auth application-default login` stačí
`GOOGLE_APPLICATION_CREDENTIALS=~/.config/gcloud/legacy_credentials/<účet>/adc.json`.
