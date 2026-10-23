# Nastavení Google Cloud a OAuth

Jednorázové kroky, aby `fitdata` mohl číst tvoje data z Google Health API.

## Region

**`europe-west1` (Belgie)**: všechno (bucket, BigQuery, job, web) běží v jednom regionu. Frankfurt (`europe-west3`) je k Praze blíž, ale neumí Cloud Run domain mapping, který potřebuje vlastní doména dashboardu. Belgie ho umí a Cloud Run je tam v levnější cenové úrovni. Rozdíl v latenci je na dashboardu nepostřehnutelný.

## 1. Projekt a API

V [Cloud Console](https://console.cloud.google.com/) založ projekt (např. `fitbit-data-<něco>`) a navaž billing účet. Pak buď v konzoli (APIs & Services → Library → „Google Health API“ → Enable), nebo přes gcloud:

```bash
gcloud services enable health.googleapis.com --project <PROJECT_ID>
```

## 2. OAuth consent screen (Google Auth Platform)

Console → **Google Auth Platform**:

1. **Branding**: název aplikace (např. „fitdata“), support e-mail = tvůj Gmail.
2. **Audience**: User type **External**. Do *Test users* přidej svůj Gmail.
3. **Data Access** → *Add or remove scopes*: přidej všechny `googlehealth.*.readonly` scopes (seznam je v `src/datatypes.rs`, `READ_SCOPES`).
4. **Audience** → **Publish app** (stav *In production*).
   - Ve stavu *Testing* vyprší refresh token po 7 dnech a denní job by přestal fungovat.
   - Health scopes jsou „restricted“, ale aplikace pro osobní použití (pod 100 uživatelů) ověření nepotřebuje. Při přihlášení se jen zobrazí varování *„Google hasn't verified this app“*. Klikni na *Advanced → Go to fitdata (unsafe)*.
   - Pokud by konzole publikování bez ověření nepovolila, zůstaň zatím v Testing. Prototyp to nezablokuje, jen bude potřeba jednou týdně spustit `fitdata login`.

## 3. OAuth klient

Google Auth Platform → **Clients** → *Create client* → **Web application**:

- *Authorized redirect URIs*: `http://localhost:8080/callback` (lokální login). Pro produkci se přidá `https://<cloud-run-url>/oauth/callback`.
- Volbu *Use this client for an AI-powered agent* nech nezaškrtnutou.

Stažený JSON nahraj **z Macu rovnou do Secret Manageru**, ať neleží v repu ani v sandboxu:

```bash
gcloud secrets create oauth-client --project fitdata-jsedlacek --data-file=<stazeny-client-secret.json>
```

Pak soubor smaž. Secret `refresh-token` vytvoř prázdný (`gcloud secrets create refresh-token --project fitdata-jsedlacek --replication-policy=automatic`).

## 4. Přihlášení

```bash
cp .env.example .env     # GCP_PROJECT=fitdata-jsedlacek
cargo run -- login
```

1. Otevři vypsanou URL v prohlížeči a povol přístup.
2. Google přesměruje na `http://localhost:8080/callback?...`. Pokud se stránka nenačte, zkopíruj celou adresu z adresního řádku a vlož ji do terminálu.
3. Refresh token se uloží jako nová verze secretu `refresh-token`.

## Kde jsou tajemství

| Co | Kde | Kdo čte |
|---|---|---|
| OAuth client (ID + secret) | Secret Manager `oauth-client` | `fitdata` lokálně (přes `gcloud auth print-access-token`), v produkci service account jobu |
| Refresh token | Secret Manager `refresh-token` | totéž |

Do souborů se nic neukládá. Pro testy jde místo `sm://...` zadat cestu k souboru (`OAUTH_CLIENT`, `REFRESH_TOKEN`).
