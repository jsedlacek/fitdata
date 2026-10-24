# Nastavení Google Cloud a OAuth

Jednorázové kroky, aby `fitdata` mohl číst tvoje data z Google Health API.

## Region

**`europe-west1` (Belgie)**: všechno (bucket, KMS klíč, job, web) běží v jednom regionu. Frankfurt (`europe-west3`) je k Praze blíž, ale neumí Cloud Run domain mapping, který potřebuje vlastní doména dashboardu. Belgie ho umí a Cloud Run je tam v levnější cenové úrovni. Rozdíl v latenci je na dashboardu nepostřehnutelný.

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
   - Health scopes jsou „restricted“, ale neověřená aplikace smí mít až 100 uživatelů. Při přihlášení se jen zobrazí varování *„Google hasn't verified this app“*. Klikni na *Advanced → Go to fitdata (unsafe)*. Pro víc uživatelů (nebo bez varování) je potřeba ověření aplikace u Googlu.
   - Pokud by konzole publikování bez ověření nepovolila, zůstaň zatím v Testing. Prototyp to nezablokuje, jen bude potřeba jednou týdně spustit `fitdata login`.

## 3. OAuth klient

Google Auth Platform → **Clients** → *Create client* → **Web application**:

- *Authorized redirect URIs*: `http://localhost:8080/callback` (lokální `fitdata login`) a `http://localhost:8080/auth/callback` (lokální dashboard). Pro produkci `https://fitdata.jakub.contact/auth/callback`.
- Volbu *Use this client for an AI-powered agent* nech nezaškrtnutou.

Stažený JSON nahraj **z Macu rovnou do Secret Manageru**, ať neleží v repu ani v sandboxu:

```bash
gcloud secrets create oauth-client --project fitdata-jsedlacek --data-file=<stazeny-client-secret.json>
```

Pak soubor smaž.

## 4. Uživatelé

Uživatelé se přihlašují na dashboardu Googlem (e-mail musí být v `ALLOWED_EMAILS`) a Fitbit si připojí tlačítkem **Připojit Fitbit**. Lokálně jde refresh token uložit i z terminálu:

```bash
cp .env.example .env     # GCP_PROJECT=fitdata-jsedlacek
cargo run -- login --user <google-sub> --email <e-mail>
```

1. Otevři vypsanou URL v prohlížeči a povol přístup.
2. Google přesměruje na `http://localhost:8080/callback?...`. Pokud se stránka nenačte, zkopíruj celou adresu z adresního řádku a vlož ji do terminálu.
3. Refresh token se zašifruje (`KMS_KEY`, lokálně výchozí `plain` = nešifrovaně) a uloží do `users/<sub>.json`.

## Kde jsou tajemství

| Co | Kde | Kdo čte |
|---|---|---|
| OAuth client (ID + secret) | Secret Manager `oauth-client` | `fitdata` lokálně (přes ADC), v produkci job a web |
| Klíč session | Secret Manager `session-key` | web |
| Refresh tokeny uživatelů | `users/<sub>.json` v bucketu, zašifrované klíčem Cloud KMS `fitdata/refresh-token` | dešifrovat smí jen job |

Do souborů se nic nešifrovaného neukládá. Pro testy jde místo `sm://...` zadat cestu k souboru (`OAUTH_CLIENT`, `SESSION_KEY`).
