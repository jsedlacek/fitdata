# Produkce na Google Cloud

```
Cloud Scheduler (denně 10:00 Europe/Prague)
  └─> Cloud Run job fitdata-sync      `fitdata job --days 7`
        ├─ Google Health API ─> GCS gs://fitdata-jsedlacek-raw/raw/{typ}/{den|měsíc}.jsonl
        └─ normalizace ─> BigQuery fitdata.{heart_rate, activity, spo2, hrv, nights, exercises, daily}
Cloud Run service fitdata-web          `fitdata serve`
  └─ Google přihlášení (ALLOWED_EMAILS) ─> dotazy do BigQuery
```

Jeden image, jedna binárka; job a web se liší jen argumenty.

## Nasazení

```bash
deploy/setup.sh    # jednorázově / idempotentně: API, bucket, dataset a tabulky, SA, IAM, session-key
deploy/deploy.sh   # build (Cloud Build), job + scheduler, web
```

Nastavení je v [deploy/config.sh](../deploy/config.sh). Schémata tabulek jsou v [deploy/schema/](../deploy/schema/). Nový sloupec stačí přidat do schématu i do `model.rs` a spustit znovu `setup.sh`.

OAuth Web client musí mít mezi *Authorized redirect URIs* adresu `https://fitdata-web-<číslo projektu>.europe-west3.run.app/auth/callback`.

## Data

- **Raw (GCS):** odpovědi API beze změny. Job při každém běhu přepíše posledních 7 dní, protože hodinky synchronizují se zpožděním.
- **BigQuery:** tabulky jsou partitionované podle lokálního data (`date`). `fitdata load` nahradí rozsah dní přes staging tabulku a transakci DELETE + INSERT, takže opakovaný běh nic nezdvojí.
- **Noc** (`nights`) patří ke dni probuzení a spojuje všechny spánky, které skončily před 14:00.

## Bezpečnost

| Identita | Smí |
|---|---|
| `fitdata-job` | číst `oauth-client` a `refresh-token`, zapisovat do bucketu, zapisovat do datasetu |
| `fitdata-web` | číst `oauth-client` a `session-key`, přidávat verze `refresh-token`, číst dataset |
| `fitdata-scheduler` | spustit job |

Web je na Cloud Runu veřejný, ale každá stránka i API vyžaduje přihlášení Googlem s e-mailem z `ALLOWED_EMAILS`. Session je HMAC podepsaná cookie (klíč `session-key`, platnost 30 dní).

Když přestane fungovat refresh token (odvolaný přístup), stačí na dashboardu kliknout na **Připojit Fitbit**.

## Provoz

```bash
gcloud run jobs execute fitdata-sync --region europe-west3 --wait     # spustit job hned
gcloud run jobs executions list --job fitdata-sync --region europe-west3
gcloud logging read 'resource.type="cloud_run_job"' --limit 50 --format 'value(textPayload)'
```

Načíst starší rozsah znovu (např. po změně normalizace), lokálně s ADC:

```bash
STORAGE=gs://fitdata-jsedlacek-raw cargo run -- load --from 2026-10-21
```
