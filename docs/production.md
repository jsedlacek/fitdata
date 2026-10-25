# Produkce na Google Cloud

```
Cloud Scheduler (každou hodinu)
  └─> Cloud Run job fitdata-sync      `fitdata job --days 7`, pro každého uživatele
        ├─ users/{uid}.json: šifrovaný refresh token ─> Cloud KMS (dešifruje jen job)
        ├─ Google Health API ─> GCS u/{uid}/raw/{typ}/{den|měsíc}.jsonl
        └─ normalizace (DataFusion) ─> GCS u/{uid}/data/{tabulka}[/{YYYY-MM}].parquet + manifest.json
Cloud Run service fitdata-web          `fitdata serve`   https://fitdata.jakub.contact
  └─ Google přihlášení (ALLOWED_EMAILS) ─> Parquet uživatele v cache instance ─> DataFusion
```

Jeden image, jedna binárka; job a web se liší jen argumenty. Všechno běží v jednom regionu, `europe-west1` (Belgie). Frankfurt (`europe-west3`) by byl blíž, ale neumí Cloud Run domain mapping.

## Nasazení

```bash
deploy/setup.sh    # jednorázově / idempotentně: API, bucket, KMS klíč, SA, IAM, session-key
deploy/deploy.sh   # build (Cloud Build), job + scheduler, web
```

Každý push do `main` nasadí GitHub Actions ([.github/workflows/deploy.yml](../.github/workflows/deploy.yml)) spuštěním `deploy/deploy.sh`. Ručně jde workflow spustit v záložce Actions (Run workflow). Přihlašuje se bez klíče přes Workload Identity Federation (pool `github`) jako `fitdata-deployer`; token dostane jen workflow z `jsedlacek/fitdata` na `main`. Pool i role nastaví `setup.sh`. Image se staví znovu jen při změně `Dockerfile*`, `.dockerignore`, `Cargo.*`, `src/`, `crates/` nebo `frontend/`, takže push samotných docs jen znovu nasadí stávající image. V Actions se binárka i dashboard staví přímo na runneru (`ubuntu-24.04`) a [Dockerfile.ci](../Dockerfile.ci) je jen zabalí do distroless image (Debian 13, kvůli glibc), který se pushne do Artifact Registry; `deploy.sh` ho pak najde a Cloud Build přeskočí. Přeložené crates drží [rust-cache](https://github.com/Swatinem/rust-cache) v cache GitHub Actions, takže změna jedné závislosti přeloží jen ji a to, co na ní závisí. Lokální `deploy.sh` staví dál plný `Dockerfile` (cargo-chef) v Cloud Buildu, bez cache.

Nastavení je v [deploy/config.sh](../deploy/config.sh). Sloupce tabulek jsou pole row structů v [src/model.rs](../src/model.rs) (`#[derive(Columns)]`, popis sloupce je doc komentář pole); nový sloupec stačí přidat tam a pushnout.

OAuth Web client musí mít mezi *Authorized redirect URIs* adresu `https://fitdata.jakub.contact/auth/callback`.

### Vlastní doména

- Doména musí být ověřená pro účet, pod kterým běží `gcloud` (`gcloud domains list-user-verified`). Pokud není, ověří ji `gcloud domains verify jakub.contact`.
- `deploy.sh` vytvoří domain mapping a vypíše DNS záznam: `CNAME fitdata → ghs.googlehosted.com`.
- V Cloudflare musí být záznam **DNS only** (šedý mráček). Přes proxy Google nevydá certifikát.
- Certifikát se vydává 15–60 minut po nastavení DNS. Stav ukáže `gcloud beta run domain-mappings describe --domain fitdata.jakub.contact --region europe-west1`.

## Uživatelé

Přihlásit se může každý účet z `ALLOWED_EMAILS` (pozvánky, `deploy/config.sh`). Uživatel je Google účet, klíčem je jeho `sub`. První přihlášení založí záznam `users/{uid}.json`, dashboard pak nabídne **Připojit Fitbit**. Po souhlasu web zašifruje refresh token klíčem KMS, uloží ho do záznamu a spustí job pro tohoto uživatele, který stáhne celou historii. Data mezi uživateli se nesdílí.

**Smazání účtu** (menu účtu na dashboardu) označí záznam jako smazaný a spustí job. Ten odvolá token u Googlu a smaže `u/{uid}/` i záznam. Web sám data mazat nesmí.

Přidat existujícího uživatele s tokenem (např. při migraci):

```bash
cargo run -- import-user --user <sub> --email <e-mail> --refresh-token <sm://secret | soubor>
```

## Data

```
users/{uid}.json                           profil, časová zóna, šifrovaný refresh token
u/{uid}/raw/{typ}/{den|měsíc}.jsonl         odpovědi Health API beze změny
u/{uid}/data/{tabulka}/{YYYY-MM}.parquet    heart_rate, activity, spo2, hrv (měsíc na soubor)
u/{uid}/data/{tabulka}.parquet              daily, nights, exercises
u/{uid}/data/manifest.json                  soubory s generation; job ho zapíše jako poslední
u/{uid}/status.json                         poslední běh syncu, poslední úspěšný, běží-li
```

- **Raw:** job při každém běhu přepíše posledních 7 dní, protože hodinky synchronizují se zpožděním. Nový uživatel dostane nejdřív celou historii (backfill).
- **Tabulky:** `fitdata load` nahradí v souborech dny z rozsahu a zbytek nechá, takže opakovaný běh nic nezdvojí. Sloupce jsou pole row structů v `model.rs`; nový sloupec stačí přidat tam, ve starších souborech se čte jako NULL.
- **Čtení:** web drží soubory uživatelů v cache instance (`/tmp`, tedy RAM, nejvýš 256 MB) a podle `manifest.json` stahuje jen změněné a jen měsíce, které požadavek potřebuje. Dotazy běží v DataFusion nad tabulkami zaregistrovanými jen pro daného uživatele ([src/warehouse.rs](../src/warehouse.rs), [src/engine.rs](../src/engine.rs)).
- **Noc** (`nights`) patří ke dni probuzení a spojuje všechny spánky, které skončily před 14:00.

## Bezpečnost

| Identita | Smí |
|---|---|
| `fitdata-job` | číst `oauth-client`, zapisovat do bucketu, šifrovat a dešifrovat klíčem `refresh-token` (KMS), zapisovat spany do Cloud Trace |
| `fitdata-web` | číst `oauth-client` a `session-key`, číst bucket, zapisovat jen `users/` (podmínka IAM), jen šifrovat klíčem `refresh-token`, spustit job (i s argumenty pro jednoho uživatele) a číst jeho stav, zapisovat spany do Cloud Trace |
| `fitdata-scheduler` | spustit job |
| `fitdata-deployer` | nasadit job, scheduler a web (GitHub Actions), číst projekt |

Web je na Cloud Runu veřejný, ale každá stránka i API vyžaduje přihlášení Googlem s e-mailem z `ALLOWED_EMAILS`. Session je HMAC podepsaná cookie (klíč `session-key`, platnost 30 dní) s ID uživatele; každý požadavek čte jen data toho uživatele.

Refresh tokeny jsou v záznamech uživatelů zašifrované klíčem Cloud KMS `fitdata/refresh-token` a ID uživatele je k šifře přivázané (additional authenticated data), takže token zkopírovaný k jinému uživateli nejde dešifrovat. Web token zašifruje, ale přečíst ho nedokáže.

## MCP server

`https://fitdata.jakub.contact/mcp` je MCP server (Streamable HTTP) s nástroji jen pro čtení nad daty přihlášeného uživatele: `get_daily_metrics`, `get_day`, `get_sleep`, `get_exercises`, `get_schema` a `query`. Kód je v [src/mcp.rs](../src/mcp.rs).

`query` je jeden SELECT v SQL DataFusion (blízké PostgreSQL), nejvýš 1000 řádků. Minutové tabulky (`heart_rate`, `activity`, `spo2`, `hrv`) obsahují jen dny `from`..`to` (výchozí posledních 90 dní, nejvýš 400), ostatní celé. Kód je v [src/query.rs](../src/query.rs):

- Dotaz dostane vlastní session, ve které jsou zaregistrované jen tabulky přihlášeného uživatele. DataFusion nemá SQL, které by sáhlo na soubory nebo síť (cesty a URL jako tabulky ani funkce typu `read_csv` neexistují), a DDL, DML i `SET` jsou vypnuté, takže `CREATE EXTERNAL TABLE` ani `COPY TO` neprojdou. Data jiných uživatelů ani server dotaz nevidí.
- Proti přetížení: dotazy běží na vlastním runtime se dvěma vlákny (nezablokují dashboard), nejvýš dva najednou, s limitem 256 MB na stav dotazu (bez zápisu na disk) a timeoutem 30 s.
- Testy v `query.rs` zkoušejí cesty a URL jako tabulky, `CREATE EXTERNAL TABLE`, `COPY`, `INSERT`, `DROP`, `SET`, `PREPARE`, víc příkazů najednou a limit paměti.

Připojení:

- **Claude:** Settings → Connectors → Add custom connector, URL `https://fitdata.jakub.contact/mcp`.
- **ChatGPT:** Settings → Apps & Connectors → Advanced → Developer mode, pak Create, URL stejná, autentizace OAuth.

Klient se zaregistruje sám (dynamická registrace), pošle uživatele na přihlášení Googlem (stejné `ALLOWED_EMAILS`) a na stránku se souhlasem. Tokeny nesou ID uživatele; tokeny vydané před zavedením více uživatelů ho nemají a dohledá se podle e-mailu. Autorizační server je v [src/authz.rs](../src/authz.rs) a nic neukládá: client ID, kódy i tokeny jsou podepsané klíčem `session-key`. Access token platí hodinu, refresh token 30 dní od posledního obnovení. Odebrání přístupu všem klientům i všem přihlášeným prohlížečům najednou: nová verze `session-key` a nové nasazení webu.

Když přestane fungovat refresh token (odvolaný přístup), dashboard to uživateli ukáže a stačí kliknout na **Obnovit přístup k datům**. Pro job to není chyba (běh uživatele skončí stavem `auth_error`), alert ho nehlásí.

## Provoz

Synchronizaci jde spustit i ručně ikonou ⟳ vedle „Synchronizováno“ v hlavičce dashboardu. Web spustí job jen pro přihlášeného uživatele (`job --user <uid>` přes Cloud Run Admin API, `POST /api/sync`), ikona se točí, dokud uživatelův běh (nebo poslední execution jobu) neskončí, a pak se data na stránce načtou znovu.

Job zpracuje až 8 uživatelů najednou a selhání jednoho neshodí ostatní. Při větším počtu uživatelů jde job pustit ve víc taskách (`--tasks N`); uživatelé se rozdělí podle hashe ID (`CLOUD_RUN_TASK_INDEX`).

**Kvóta Health API** je 300 požadavků za minutu na uživatele (klouzavé okno; odpověď 429 neříká, kdy čekat), 120 000 za minutu na projekt. Proto ([src/job.rs](../src/job.rs), [src/api.rs](../src/api.rs)):

- Uživatele synchronizuje vždy jen jeden proces: zámek `u/{uid}/lock` vzniká atomicky (GCS `ifGenerationMatch=0`), druhý běh uživatele přeskočí, zámek starší než 45 min se převezme.
- Požadavky jednoho uživatele hlídá rate limiter (`governor`, GCRA): burst 40, pak 240 za minutu, tedy v libovolné minutě nejvýš 280. Kdyby přesto přišla 429, požadavek počká 2–5 s.
- Hodinový běh stahuje jen včerejšek a dnešek a přeskakuje datové typy, pro které uživatel nemá žádná data. Jednou za 20 h je běh plný: celé okno 7 dní a u každého typu jeden dotaz, jestli data má (`status.json`: `last_full`, `absent`). Dny před založením účtu se nestahují (`since`).
- Na tvém účtu: hodinový běh 51 požadavků, plný 121 (dřív 173 každou hodinu).

Každý neúspěšný běh jobu (chyba u kteréhokoli uživatele kromě odvolaného tokenu) otevře v Cloud Monitoring incident **fitdata-sync failed** a pošle e-mail na `ALERT_EMAIL` (`deploy/config.sh`). Při trvalé poruše přijde jeden e-mail, ne jeden za hodinu. Až běhy znovu projdou, incident se sám zavře. Politika je v [deploy/alert-policy.json](../deploy/alert-policy.json) a nastaví ji `setup.sh`.

Druhá politika, **fitdata-web errors** ([deploy/alert-web-errors.json](../deploy/alert-web-errors.json)), pošle e-mail, když web zaloguje chybu nebo odpoví 5xx, nejvýš jednou za hodinu.

### Logy, trace a chyby

Nastavení je v [src/telemetry.rs](../src/telemetry.rs).

- **Logy:** na Cloud Runu (`K_SERVICE` nebo `CLOUD_RUN_JOB`) píše binárka JSON řádky ve formátu Cloud Loggingu: `severity`, `message`, zdrojový řádek a pole události i nadřazených spanů (např. `data_type`, `table`, `http.route`) v `jsonPayload`. Lokálně je výstup čitelný text. Formát jde vynutit přes `LOG_FORMAT=json` nebo `LOG_FORMAT=text`, úroveň přes `RUST_LOG`.
- **Trace:** spany se posílají přes OpenTelemetry (OTLP) do Cloud Trace (Telemetry API `telemetry.googleapis.com`). Každý HTTP požadavek na web je jeden trace, který navazuje na trace Cloud Runu (`traceparent`). Každý běh jobu je jeden trace `sync job` se spany pro uživatele, datové typy, volání Health API, čtení a zápis v GCS a zápis tabulek.
- **Korelace:** každý log nese `logging.googleapis.com/trace` a `spanId`. V Logs Explorer se logy požadavku seskupí pod jeho request log a v Cloud Trace se u spanu zobrazí jeho logy.
- **Error Reporting:** logy úrovně ERROR (i panic) mají typ `ReportedErrorEvent` a verzi image (`APP_VERSION`), takže je [Error Reporting](https://console.cloud.google.com/errors?project=fitdata-jsedlacek) seskupí.

```bash
gcloud run jobs execute fitdata-sync --region europe-west1 --wait     # spustit job hned
gcloud run jobs executions list --job fitdata-sync --region europe-west1
gcloud logging read 'resource.type="cloud_run_job"' --limit 50 --format 'value(severity, jsonPayload.message)'
```

Načíst starší rozsah uživatele znovu (např. po změně normalizace), lokálně s ADC:

```bash
STORAGE=gs://fitdata-jsedlacek-data cargo run -- load --user <uid> --from 2026-10-21
```
