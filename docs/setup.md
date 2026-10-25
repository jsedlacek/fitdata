# Google Cloud and OAuth setup

One-time steps so that `fitdata` can read your data from the Google Health API.

## Region

**`europe-west1` (Belgium)**: everything (bucket, KMS key, job, web) runs in one region. Frankfurt (`europe-west3`) is closer to Prague but has no Cloud Run domain mapping, which the dashboard's custom domain needs. Belgium has it, and Cloud Run is in the cheaper price tier there. The latency difference is unnoticeable on the dashboard.

## 1. Project and API

In the [Cloud Console](https://console.cloud.google.com/), create a project (e.g. `fitbit-data-<something>`) and link a billing account. Then either in the console (APIs & Services → Library → "Google Health API" → Enable) or with gcloud:

```bash
gcloud services enable health.googleapis.com --project <PROJECT_ID>
```

## 2. OAuth consent screen (Google Auth Platform)

Console → **Google Auth Platform**:

1. **Branding**: app name (e.g. "fitdata"), support e-mail = your Gmail, logo, and the public pages the web serves without sign-in: home page `https://fitdata.jakub.contact/`, privacy policy `/privacy`, terms `/terms` (`frontend/home.html`, `privacy.html`, `terms.html`). Authorized domain `jakub.contact`.
2. **Audience**: User type **External**. Add your Gmail to *Test users*.
3. **Data Access** → *Add or remove scopes*: add the four `googlehealth.*.readonly` scopes listed in `src/datatypes.rs` (`READ_SCOPES`), and no others: Google verification asks for the narrowest scopes.
4. **Audience** → **Publish app** (status *In production*).
   - In *Testing*, the refresh token expires after 7 days and the daily job would stop working.
   - Health scopes are "restricted", but an unverified app may have up to 100 users. Sign-in only shows the warning *"Google hasn't verified this app"*. Click *Advanced → Go to fitdata (unsafe)*. More users (or no warning) need the app verified by Google.
   - If the console does not allow publishing without verification, stay in Testing for now. It does not block a prototype; you just have to run `fitdata login` once a week.

## 3. OAuth client

Google Auth Platform → **Clients** → *Create client* → **Web application**:

- *Authorized redirect URIs*: `http://localhost:8080/callback` (local `fitdata login`) and `http://localhost:8080/auth/callback` (local dashboard). For production, `https://fitdata.jakub.contact/auth/callback`.
- Leave *Use this client for an AI-powered agent* unchecked.

Upload the downloaded JSON **from your Mac straight to Secret Manager**, so it is neither in the repo nor in the sandbox:

```bash
gcloud secrets create oauth-client --project fitdata-jsedlacek --data-file=<downloaded-client-secret.json>
```

Then delete the file.

## 4. Users

Users sign in to the dashboard with Google (the e-mail must be in `ALLOWED_EMAILS`) and connect their Fitbit with the **Connect Fitbit** button. Locally, a refresh token can also be stored from the terminal:

```bash
cp .env.example .env     # GCP_PROJECT=fitdata-jsedlacek
cargo run -- login --user <google-sub> --email <e-mail>
```

1. Open the printed URL in a browser and allow access.
2. Google redirects to `http://localhost:8080/callback?...`. If the page does not load, copy the whole address from the address bar and paste it into the terminal.
3. The refresh token is encrypted (`KMS_KEY`, locally `plain` by default = unencrypted) and stored in `users/<sub>.json`.

## Where the secrets are

| What | Where | Who reads it |
|---|---|---|
| OAuth client (ID + secret) | Secret Manager `oauth-client` | `fitdata` locally (through ADC), in production the job and the web |
| Session key | Secret Manager `session-key` | web |
| Users' refresh tokens | `users/<sub>.json` in the bucket, encrypted with the Cloud KMS key `fitdata/refresh-token` | only the job may decrypt them |

Nothing unencrypted is stored in files. For tests, a file path can be given instead of `sm://...` (`OAUTH_CLIENT`, `SESSION_KEY`).
