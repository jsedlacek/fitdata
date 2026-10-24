# Plán: více uživatelů, Parquet + DuckDB

> **Hotovo (2026-10-24).** Oproti plánu: místo DuckDB je DataFusion (Rust, SQL bez cesty k souborům a síti, jednodušší build; srovnání níže) a MCP `query` proto běží přímo ve webu na vlastním runtime místo samostatné služby. Refresh tokeny šifruje symetrický klíč KMS s oddělenými rolemi (web jen šifruje, job i dešifruje) místo asymetrického. Nasazení proběhlo ve třech krocích místo sedmi. Aktuální popis je v [production.md](production.md).
>
> Srovnání na roce dat (2 vlákna): stránka dne DataFusion 20 ms, DuckDB 27 ms; roční agregace tepu 43 ms, resp. 65 ms. Všechny pokusy o čtení souborů, síť, DDL, DML a `SET` DataFusion odmítl už při plánování.

Cíl: z fitdata pro jednoho uživatele udělat službu pro víc lidí (zatím na pozvánku, ale připravenou na širší nasazení) a BigQuery nahradit Parquetem v GCS čteným přes DuckDB.

## Rozhodnutí

- **Uživatel** = Google `sub` (ne e-mail). Data mezi uživateli se nesdílí.
- **Přístup** zatím přes `ALLOWED_EMAILS` (pozvánky); první přihlášení povoleného e-mailu založí uživatele.
- **Data:** Parquet v GCS podle uživatele, žádný BigQuery. Vlastní analytika napříč uživateli není potřeba.
- **Čtení:** DuckDB ve webu pro dashboard a strukturované MCP nástroje (SQL píšeme my).
- **MCP `query`** (cizí SQL) běží v samostatné Cloud Run službě `fitdata-query`, jejíž SA nemá žádné role. Služba dostane v požadavku jen soubory daného uživatele a nemá klíče, cizí data ani oprávnění, takže i při chybě v DuckDB útočník uvidí nanejvýš svá vlastní data. Nastavení DuckDB je druhá vrstva, ne hranice.
- **Refresh tokeny** zašifrované asymetrickým klíčem Cloud KMS: web šifruje veřejným klíčem, dešifrovat smí jen `fitdata-job`.
- **Stávající data** vlastníka se přesunou pod jeho ID (stejně jako u ostatních uživatelů).
- OAuth app je *In production*, takže refresh tokeny nevyprší po 7 dnech.

Měření (Cloud Run europe-west1, 1 vCPU, 512 MiB, rok dat): stránka dne v BigQuery 309 ms (medián), v DuckDB s teplou cache 57 ms včetně kontroly změn v GCS, se studenou cache 112 ms. Roční agregace tepu (10 mil. řádků) v DuckDB 206 ms. Rok tepu má 48,5 MB Parquetu.

## Rozložení v GCS

```
users/{uid}.json                           profil: e-mail, časová zóna, šifrovaný refresh token
u/{uid}/raw/{typ}/{den|měsíc}.jsonl         odpovědi Health API (dnes raw/{typ}/…)
u/{uid}/data/{tabulka}/{YYYY-MM}.parquet    heart_rate, activity, spo2, hrv
u/{uid}/data/{tabulka}.parquet              daily, nights, exercises (malé, jeden soubor)
u/{uid}/data/manifest.json                  seznam souborů s generation, zapisuje se jako poslední
u/{uid}/status.json                         poslední běh syncu: čas, ok, auth_error, chyba, běží
```

Všechno o uživateli kromě záznamu v registru je pod jedním prefixem `u/{uid}/`, takže smazání účtu je smazání prefixu.

`manifest.json` dává webu konzistentní snímek (job ho přepíše až po všech souborech) a jednu kontrolu změn na požadavek místo jedné na soubor.

## Fáze

Každá fáze jde samostatně nasadit (push do `main` = produkce) a produkce mezi fázemi funguje.

### Fáze 1: zápis Parquetu (vedle BigQuery)

Jeden uživatel, ale cesty už podle `uid`, aby se data nemigrovala dvakrát.

- `OWNER_ID` (Google `sub` vlastníka) v `deploy/config.sh` a env jobu.
- Nový modul `src/parquet.rs` (zápis přes DuckDB v jobu, žádná další závislost):
  - řádky z `model::table_rows` → DuckDB Appender → `COPY … TO … (FORMAT parquet, COMPRESSION zstd)` → `Store::put`;
  - přepisují se jen měsíce v rozsahu běhu: stávající soubor bez dní z rozsahu `UNION ALL` nové řádky;
  - nakonec `manifest.json`.
- `fitdata load` zapisuje do BigQuery i do Parquetu, takže se jím jednorázově naplní celá historie.
- `duckdb` crate s `bundled`; ověřit čas buildu v Actions (cargo-chef) a že `distroless/cc` má `libstdc++`.
- Testy: zápis a slučování měsíce na lokálním `Store` (rozsah uprostřed měsíce, přes hranici měsíce, prázdný rozsah).
- Hotovo, když: v produkci vzniknou soubory a skript porovná počty a agregace po dnech a tabulkách s BigQuery bez rozdílu.

### Fáze 2: čtení přes DuckDB (web a MCP)

- Nový modul `src/warehouse.rs`:
  - cache souborů uživatele v `/tmp/fitdata/{uid}/` (na Cloud Runu je to RAM, proto limit velikosti s LRU, například 128 MB);
  - na každý požadavek načte `manifest.json` a stáhne jen změněné soubory;
  - dotazy přes `spawn_blocking`, výsledky do stávajících serde struktur (`DailyMetrics`, `NightRow`, `HrMinute`, …), nejspíš přes `serde_arrow`, aby zůstalo API jako `query_as`.
- `web.rs`: `day_detail` a `overview` přepsané na DuckDB SQL.
- `mcp.rs`: `get_daily_metrics`, `get_day`, `get_sleep`, `get_exercises` a `get_schema` (`DESCRIBE`) nad DuckDB; popis nástrojů přepnout na DuckDB dialekt.
- **MCP `query` v samostatné službě `fitdata-query`** (stejná binárka, příkaz `fitdata query-serve`):
  - Cloud Run služba se SA `fitdata-query`, který **nemá žádné role** (`setup.sh` ho založí a ověří, že nemá žádnou vazbu v IAM projektu ani bucketu). Bez secretů a bez env s konfigurací.
  - Volat ji smí jen `fitdata-web` (`roles/run.invoker` jen pro něj, žádné `allUsers`); web se prokáže ID tokenem z metadata serveru s audience URL služby. Ingress zůstane `all`, protože `internal` by vyžadoval Direct VPC egress webu; hranici drží IAM.
  - `concurrency=1`: instance obsluhuje vždy jen jeden dotaz, takže se data různých uživatelů nepotkají v paměti ani v `/tmp`. Dále `--memory 1Gi`, `--cpu 1`, `--timeout 60`, `--max-instances` jako strop ceny (při vyčerpání vrátí web nástroji chybu „zkuste později“).
  - Požadavek: SQL a Parquet soubory uživatele z cache webu (multipart). Rok tepu má asi 50 MB, víc než limit 32 MiB pro HTTP/1 na Cloud Runu, proto HTTP/2 end-to-end (`--use-http2`, h2c na straně služby, `http2_prior_knowledge` v klientovi webu).
  - Služba soubory zapíše do `/tmp/q-<náhodné>/`, vytvoří nad nimi views a pak DuckDB uzamkne: `allowed_directories=[ten adresář]`, `enable_external_access=false`, `autoload_known_extensions=false`, `autoinstall_known_extensions=false`, `allow_community_extensions=false`, `memory_limit=768MB`, `threads=1`, `lock_configuration=true`. Po dotazu adresář smaže.
  - Jen jeden příkaz `SELECT` (typ příkazu z `extract_statements`), nejvýš 1000 řádků a omezená velikost odpovědi, timeout 30 s přes přerušení dotazu.
  - Logy jen na stdout (Cloud Run je sbírá bez oprávnění); spany do Cloud Trace služba neposílá, protože by k tomu potřebovala roli. Trvání a chyby dotazu zachytí span volání ve webu.
  - Studený start služby (kolem sekundy) se projeví jen u `query`; `min-instances` necháme na 0.
- **Testy služby** (`cargo test`), všechny musí skončit chybou: soubor mimo adresář požadavku, `read_csv('/etc/passwd')`, `read_text`, `read_blob`, glob `/**`, `COPY … TO`, `ATTACH`, `INSTALL` a `LOAD`, `SET` a `PRAGMA`, `CREATE`, `getenv()`, víc příkazů najednou, dotaz přes limit paměti a přes timeout. Jsou to testy druhé vrstvy; hranici tvoří služba bez oprávnění a bez cizích dat.
- Paměť webu nejspíš zvednout na 1 GiB (podle měření v produkci).
- Hotovo, když: dashboard ověřený v prohlížeči (Playwright), MCP nástroje včetně `query` fungují z Claude, volání `fitdata-query` bez ID tokenu webu skončí 403, latence v Cloud Trace odpovídá měření.

### Fáze 3: odstranění BigQuery

- `sync_runs` nahradí `u/{uid}/status.json`; historie běhů zůstane v logu.
- Smazat `bq.rs`, `google-cloud-bigquery-v2`, `deploy/schema/`, `deploy/schema.sh`, staging v bucketu a role BigQuery v IAM (`setup.sh`).
- Dataset `health` smazat ručně až po potvrzení (zdrojem pravdy jsou raw data a Parquet).
- Hotovo, když: v kódu ani v IAM nezůstane nic z BigQuery a job i web fungují.

### Fáze 4: uživatelé a tokeny

- **Identita:** `sub` z ID tokenu. `Claims::Session`, `Consent`, `Code`, `Access` a `Refresh` ponesou `uid` (a e-mail jen pro zobrazení). Po nasazení se všichni znovu přihlásí a MCP klienti znovu autorizují (staré tokeny formát nepřečtou).
- **Registr:** `users/{uid}.json`; první přihlášení e-mailu z `ALLOWED_EMAILS` ho založí. Pozdější krok pro širší nasazení: pozvánky v GCS místo env.
- **KMS:** keyring `fitdata`, klíč `refresh-token` (`ASYMMETRIC_DECRYPT`, `RSA_DECRYPT_OAEP_3072_SHA256`).
  - web: `cloudkms.cryptoKeyVersions.viewPublicKey`, veřejný klíč v cache, šifruje lokálně (`rsa` crate);
  - job: `roles/cloudkms.cryptoKeyDecrypter`; dešifruje přes KMS REST;
  - když Google vrátí nový refresh token, job ho zašifruje a uloží sám.
- **Připojení Fitbitu:** stávající flow `connect_fitbit` uloží šifrovaný token do záznamu uživatele a spustí job pro něj (historie od `membership_start`).
- **Migrace vlastníka:** jednorázový příkaz přesune token ze secretu `refresh-token` do `users/{OWNER_ID}.json` a `raw/{typ}/…` do `u/{OWNER_ID}/raw/{typ}/…`. Secret `refresh-token` a jeho IAM se smažou až po potvrzení.
- `fitdata login` (CLI) uloží token pro zadaného uživatele, nebo ho zrušíme.

### Fáze 5: job pro všechny uživatele

- `fitdata job` projde `users/` a zpracuje uživatele s tokenem; selhání jednoho neshodí ostatní.
- `AuthError` je stav uživatele (`needs_reconnect` v `status.json`), ne chyba jobu. Job skončí chybou (a alert se spustí) jen při jiné chybě.
- `--user <uid>` pro ruční sync: web spustí execution s přepsanými argumenty; „běží“ se čte ze `status.json`, ne ze seznamu executions.
- „Dnes“ podle časové zóny uživatele (`users/{uid}.json`, výchozí `Europe/Prague`), ne podle `TZ` kontejneru.
- Škálování: uživatelé rozdělení mezi tasky jobu podle `hash(uid) % CLOUD_RUN_TASK_COUNT`; zatím 1 task, ale kód připravený. Health API retry a backoff už existuje.

### Fáze 6: web a MCP pro více uživatelů

- Každý handler bere `uid` ze session nebo bearer tokenu; `warehouse` čte jen adresář toho uživatele a `fitdata-query` dostane jen jeho soubory.
- Dashboard: přihlášený e-mail a odhlášení, stav „bez dat / probíhá první načtení“, banner pro znovupřipojení podle `status.json`.
- MCP instrukce s časovou zónou uživatele místo napevno `Europe/Prague`.
- Smazání účtu: odvolá token u Googlu a smaže `users/{uid}.json` a celý prefix `u/{uid}/`.
- Test: dva uživatelé, každý vidí jen svá data (web, MCP nástroje, MCP `query`).

### Fáze 7: úklid a dokumentace

- `docs/production.md` (schéma, IAM tabulka včetně `fitdata-query` bez rolí, provoz), `docs/setup.md`, `AGENTS.md`, `README.md`.
- Smazat zdroje benchmarku: dataset `bench`, `gs://fitdata-jsedlacek-data/bench/`, job `fitdata-bench`, image `fitdata/bench`.

## Rizika

- **MCP `query`:** hranici tvoří služba `fitdata-query` bez oprávnění, která dostane jen data jednoho uživatele. Kdyby jí někdo omylem přidal roli nebo secret, hranice zmizí, proto to `setup.sh` při každém běhu kontroluje. Chyba v nastavení DuckDB nebo v DuckDB samotné ohrozí nanejvýš data uživatele, který dotaz poslal.
- **Paměť instance:** cache webu v `/tmp` i DuckDB jsou v RAM; limit cache je nutné nastavit spolu s pamětí Cloud Runu. U `fitdata-query` se do 1 GiB musí vejít soubory požadavku i `memory_limit`; u uživatelů s mnohaletou historií bude potřeba posílat jen měsíce v rozsahu dotazu, nebo paměť zvýšit.
- **Studený start:** první požadavek uživatele po startu instance stahuje soubory (asi 110 ms pro den, víc pro dotazy přes celou historii).
- **Kvóty Health API** při mnoha uživatelích: zjistit limity v konzoli a případně rozložit uživatele v hodině.
