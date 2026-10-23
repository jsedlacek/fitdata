# fitbit-data

Stahuje data z Fitbitu (Fitbit Air) přes [Google Health API v4](https://developers.google.com/health) a ukládá je pro analýzu.

Fitbit Web API se vypíná 30. 10. 2026; tohle používá jeho náhradu, Google Health API.

## Stav

Běží v produkci na Google Cloud: denní Cloud Run job stahuje data do GCS a BigQuery a dashboard běží na Cloud Runu za přihlášením Googlem. Popis je v [docs/production.md](docs/production.md).

## Použití

Nejdřív nastav GCP projekt a OAuth klienta: [docs/setup.md](docs/setup.md). Tajemství (OAuth client, refresh token) jsou v Secret Manageru, lokálně se k nim přistupuje přes přihlášený `gcloud`.

```bash
cargo run -- login       # jednorázově: OAuth souhlas, uloží refresh token do Secret Manageru
cargo run -- whoami      # identita, profil, spárovaná zařízení
cargo run -- probe       # které datové typy jsou dostupné a mají data
cargo run -- backfill    # celá historie (lze přerušit a pustit znovu)
cargo run -- sync        # posledních 7 dní
cargo run -- load --from 2026-10-21   # normalizace raw dat do BigQuery
cargo run -- job         # sync + load posledních 7 dní (to dělá denní job)
cargo run -- serve       # dashboard nad BigQuery (lokálně AUTH_DISABLED=1)
```

`backfill` i `sync` berou `--types heart-rate,sleep,...`, aby šlo stahovat jen vybrané typy.

## Uložená data

```
data/
  meta/whoami.json, meta/probe.json
  raw/{data-type}/{YYYY-MM-DD}.jsonl   # intraday typy (tep, kroky, HRV, ...)
  raw/{data-type}/{YYYY-MM}.jsonl      # řídké, denní a session typy (spánek, cvičení, ...)
```

Každý řádek obsahuje jeden data point přesně tak, jak ho vrátilo API. Seznam typů a způsob jejich filtrování je v [src/datatypes.rs](src/datatypes.rs).
