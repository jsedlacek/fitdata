# Produkce na Google Cloud

```
Cloud Scheduler (každou hodinu)
  └─> Cloud Run job fitdata-sync      `fitdata job --days 7`
        ├─ Google Health API ─> GCS gs://fitdata-jsedlacek-data/raw/{typ}/{den|měsíc}.jsonl
        └─ normalizace ─> BigQuery health.{heart_rate, activity, spo2, hrv, nights, exercises, daily}
Cloud Run service fitdata-web          `fitdata serve`   https://fitdata.jakub.contact
  └─ Google přihlášení (ALLOWED_EMAILS) ─> dotazy do BigQuery
```

Jeden image, jedna binárka; job a web se liší jen argumenty. Všechno běží v jednom regionu, `europe-west1` (Belgie). Frankfurt (`europe-west3`) by byl blíž, ale neumí Cloud Run domain mapping.

## Nasazení

```bash
deploy/setup.sh    # jednorázově / idempotentně: API, bucket, dataset a tabulky, SA, IAM, session-key
deploy/deploy.sh   # build (Cloud Build), job + scheduler, web
```

Každý push do `main` nasadí GitHub Actions ([.github/workflows/deploy.yml](../.github/workflows/deploy.yml)) spuštěním `deploy/deploy.sh`. Ručně jde workflow spustit v záložce Actions (Run workflow). Přihlašuje se bez klíče přes Workload Identity Federation (pool `github`) jako `fitdata-deployer`; token dostane jen workflow z `jsedlacek/fitdata` na `main`. Pool i role nastaví `setup.sh`. Image se staví znovu jen při změně `Dockerfile`, `.dockerignore`, `Cargo.*`, `src/` nebo `frontend/`, takže push samotných docs jen znovu nasadí stávající image. V Actions se image staví přímo na runneru (Buildx) a pushne do Artifact Registry; `deploy.sh` ho pak najde a Cloud Build přeskočí. Závislosti Rustu staví [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) ve vlastní vrstvě, kterou drží cache GitHub Actions, takže se znovu kompilují jen při změně `Cargo.*`. Lokální `deploy.sh` staví dál v Cloud Buildu, bez cache.

Nastavení je v [deploy/config.sh](../deploy/config.sh). Schémata tabulek jsou v [deploy/schema/](../deploy/schema/). Nový sloupec stačí přidat do schématu i do `model.rs` a spustit znovu `setup.sh`.

OAuth Web client musí mít mezi *Authorized redirect URIs* adresu `https://fitdata.jakub.contact/auth/callback`.

### Vlastní doména

- Doména musí být ověřená pro účet, pod kterým běží `gcloud` (`gcloud domains list-user-verified`). Pokud není, ověří ji `gcloud domains verify jakub.contact`.
- `deploy.sh` vytvoří domain mapping a vypíše DNS záznam: `CNAME fitdata → ghs.googlehosted.com`.
- V Cloudflare musí být záznam **DNS only** (šedý mráček). Přes proxy Google nevydá certifikát.
- Certifikát se vydává 15–60 minut po nastavení DNS. Stav ukáže `gcloud beta run domain-mappings describe --domain fitdata.jakub.contact --region europe-west1`.

## Data

- **Raw (GCS):** odpovědi API beze změny. Job při každém běhu přepíše posledních 7 dní, protože hodinky synchronizují se zpožděním.
- **BigQuery:** tabulky jsou partitionované podle lokálního data (`date`). `fitdata load` nahradí rozsah dní přes staging tabulku a transakci DELETE + INSERT, takže opakovaný běh nic nezdvojí.
- **Noc** (`nights`) patří ke dni probuzení a spojuje všechny spánky, které skončily před 14:00.

## Bezpečnost

| Identita | Smí |
|---|---|
| `fitdata-job` | číst `oauth-client` a `refresh-token`, zapisovat do bucketu, zapisovat do datasetu, zapisovat spany do Cloud Trace |
| `fitdata-web` | číst `oauth-client` a `session-key`, přidávat a vypínat verze `refresh-token` (číst ne), číst dataset, spustit job a číst jeho stav, zapisovat spany do Cloud Trace |
| `fitdata-scheduler` | spustit job |
| `fitdata-deployer` | nasadit job, scheduler a web (GitHub Actions), číst projekt |

Web je na Cloud Runu veřejný, ale každá stránka i API vyžaduje přihlášení Googlem s e-mailem z `ALLOWED_EMAILS`. Session je HMAC podepsaná cookie (klíč `session-key`, platnost 30 dní).

## MCP server

`https://fitdata.jakub.contact/mcp` je MCP server (Streamable HTTP) s nástroji jen pro čtení: `get_daily_metrics`, `get_day`, `get_sleep`, `get_exercises`, `get_schema` a `query` (jeden SELECT nad datasetem, nejvýš 1 GB skenu a 1000 řádků; zápis brání IAM, protože `fitdata-web` smí dataset jen číst). Kód je v [src/mcp.rs](../src/mcp.rs).

Připojení:

- **Claude:** Settings → Connectors → Add custom connector, URL `https://fitdata.jakub.contact/mcp`.
- **ChatGPT:** Settings → Apps & Connectors → Advanced → Developer mode, pak Create, URL stejná, autentizace OAuth.

Klient se zaregistruje sám (dynamická registrace), pošle uživatele na přihlášení Googlem (stejné `ALLOWED_EMAILS`) a na stránku se souhlasem. Autorizační server je v [src/authz.rs](../src/authz.rs) a nic neukládá: client ID, kódy i tokeny jsou podepsané klíčem `session-key`. Access token platí hodinu, refresh token 30 dní od posledního obnovení. Odebrání přístupu všem klientům i všem přihlášeným prohlížečům najednou: nová verze `session-key` a nové nasazení webu.

Když přestane fungovat refresh token (odvolaný přístup), stačí na dashboardu kliknout na **Připojit Fitbit**.

## Provoz

Synchronizaci jde spustit i ručně ikonou ⟳ vedle „Synchronizováno“ v hlavičce dashboardu. Web spustí job přes Cloud Run Admin API (`POST /api/sync`), ikona se točí, dokud běží poslední execution jobu (i plánovaná), a pak se data na stránce načtou znovu.

Každý neúspěšný běh jobu otevře v Cloud Monitoring incident **fitdata-sync failed** a pošle e-mail na `ALERT_EMAIL` (`deploy/config.sh`). Při trvalé poruše přijde jeden e-mail, ne jeden za hodinu. Až běhy znovu projdou, incident se sám zavře. Politika je v [deploy/alert-policy.json](../deploy/alert-policy.json) a nastaví ji `setup.sh`.

Druhá politika, **fitdata-web errors** ([deploy/alert-web-errors.json](../deploy/alert-web-errors.json)), pošle e-mail, když web zaloguje chybu nebo odpoví 5xx, nejvýš jednou za hodinu.

### Logy, trace a chyby

Nastavení je v [src/telemetry.rs](../src/telemetry.rs).

- **Logy:** na Cloud Runu (`K_SERVICE` nebo `CLOUD_RUN_JOB`) píše binárka JSON řádky ve formátu Cloud Loggingu: `severity`, `message`, zdrojový řádek a pole události i nadřazených spanů (např. `data_type`, `table`, `http.route`) v `jsonPayload`. Lokálně je výstup čitelný text. Formát jde vynutit přes `LOG_FORMAT=json` nebo `LOG_FORMAT=text`, úroveň přes `RUST_LOG`.
- **Trace:** spany se posílají přes OpenTelemetry (OTLP) do Cloud Trace (Telemetry API `telemetry.googleapis.com`). Každý HTTP požadavek na web je jeden trace, který navazuje na trace Cloud Runu (`traceparent`). Každý běh jobu je jeden trace `sync job` se spany pro datové typy, volání Health API, čtení a zápis v GCS a dotazy do BigQuery.
- **Korelace:** každý log nese `logging.googleapis.com/trace` a `spanId`. V Logs Explorer se logy požadavku seskupí pod jeho request log a v Cloud Trace se u spanu zobrazí jeho logy.
- **Error Reporting:** logy úrovně ERROR (i panic) mají typ `ReportedErrorEvent` a verzi image (`APP_VERSION`), takže je [Error Reporting](https://console.cloud.google.com/errors?project=fitdata-jsedlacek) seskupí.

```bash
gcloud run jobs execute fitdata-sync --region europe-west1 --wait     # spustit job hned
gcloud run jobs executions list --job fitdata-sync --region europe-west1
gcloud logging read 'resource.type="cloud_run_job"' --limit 50 --format 'value(severity, jsonPayload.message)'
```

Načíst starší rozsah znovu (např. po změně normalizace), lokálně s ADC:

```bash
STORAGE=gs://fitdata-jsedlacek-data cargo run -- load --from 2026-10-21
```
