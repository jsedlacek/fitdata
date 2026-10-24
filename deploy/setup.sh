#!/usr/bin/env bash
# One-time (and idempotent) infrastructure for fitdata. Safe to re-run: it
# only creates what is missing and re-applies IAM bindings.
#
#   deploy/setup.sh
#
# Prerequisites done by hand: the project with billing, the OAuth consent
# screen and Web client (docs/setup.md), and the secret `oauth-client`.
set -euo pipefail
cd "$(dirname "$0")/.."
source deploy/config.sh

say() { printf '\n== %s\n' "$*"; }

say "APIs"
gcloud services enable --project "$PROJECT" \
  run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com \
  cloudscheduler.googleapis.com cloudkms.googleapis.com storage.googleapis.com \
  secretmanager.googleapis.com health.googleapis.com monitoring.googleapis.com \
  iamcredentials.googleapis.com sts.googleapis.com cloudresourcemanager.googleapis.com \
  logging.googleapis.com cloudtrace.googleapis.com telemetry.googleapis.com \
  clouderrorreporting.googleapis.com

say "Bucket gs://$BUCKET"
if ! gcloud storage buckets describe "gs://$BUCKET" --project "$PROJECT" >/dev/null 2>&1; then
  gcloud storage buckets create "gs://$BUCKET" --project "$PROJECT" --location "$REGION" \
    --uniform-bucket-level-access --public-access-prevention
fi

say "KMS key $KMS_KEY_NAME"
if ! gcloud kms keyrings describe "$KMS_KEYRING" --project "$PROJECT" --location "$REGION" >/dev/null 2>&1; then
  gcloud kms keyrings create "$KMS_KEYRING" --project "$PROJECT" --location "$REGION"
fi
if ! gcloud kms keys describe "$KMS_KEY_NAME" --keyring "$KMS_KEYRING" --project "$PROJECT" --location "$REGION" >/dev/null 2>&1; then
  gcloud kms keys create "$KMS_KEY_NAME" --keyring "$KMS_KEYRING" --project "$PROJECT" --location "$REGION" \
    --purpose encryption --rotation-period 365d --next-rotation-time "$(date -u -d '+365 days' +%Y-%m-%dT%H:%M:%SZ)"
fi

say "Artifact Registry $REPO"
if ! gcloud artifacts repositories describe "$REPO" --project "$PROJECT" --location "$REGION" >/dev/null 2>&1; then
  gcloud artifacts repositories create "$REPO" --project "$PROJECT" --location "$REGION" \
    --repository-format docker --description "fitdata images"
fi

say "Secrets"
if ! gcloud secrets describe session-key --project "$PROJECT" >/dev/null 2>&1; then
  gcloud secrets create session-key --project "$PROJECT" --replication-policy automatic
  openssl rand 32 | gcloud secrets versions add session-key --project "$PROJECT" --data-file=-
fi

say "Service accounts"
for sa in "$JOB_SA" "$WEB_SA" "$SCHEDULER_SA" "$QUERY_SA" "$DEPLOY_SA"; do
  name=${sa%%@*}
  if ! gcloud iam service-accounts describe "$sa" --project "$PROJECT" >/dev/null 2>&1; then
    gcloud iam service-accounts create "$name" --project "$PROJECT" --display-name "fitdata $name"
  fi
done

say "IAM"
secret_role() { # secret member role
  gcloud secrets add-iam-policy-binding "$1" --project "$PROJECT" \
    --member "serviceAccount:$2" --role "$3" --condition=None >/dev/null
}
# Job: the OAuth client. Web: the OAuth client and the session key.
secret_role oauth-client "$JOB_SA" roles/secretmanager.secretAccessor
secret_role oauth-client "$WEB_SA" roles/secretmanager.secretAccessor
secret_role session-key "$WEB_SA" roles/secretmanager.secretAccessor
# Refresh tokens: "Connect Fitbit" on the web encrypts one, only the job
# decrypts them (and encrypts with `fitdata import-user`).
key_role() { # member role
  gcloud kms keys add-iam-policy-binding "$KMS_KEY_NAME" --keyring "$KMS_KEYRING" \
    --project "$PROJECT" --location "$REGION" --member "serviceAccount:$1" --role "$2" >/dev/null
}
key_role "$WEB_SA" roles/cloudkms.cryptoKeyEncrypter
key_role "$JOB_SA" roles/cloudkms.cryptoKeyEncrypterDecrypter

gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" \
  --member "serviceAccount:$JOB_SA" --role roles/storage.objectAdmin >/dev/null
# Web: reads the users' Parquet tables (src/warehouse.rs) and their status,
# and writes only the user registry (users/).
gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" \
  --member "serviceAccount:$WEB_SA" --role roles/storage.objectViewer >/dev/null
gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" \
  --member "serviceAccount:$WEB_SA" --role roles/storage.objectUser \
  --condition "title=user-registry,expression=resource.name.startsWith(\"projects/_/buckets/$BUCKET/objects/users/\")" >/dev/null

# The query service gets the data of one user with each request and must not
# be able to read anything else: its service account has no role anywhere.
# Fails setup if one was added by hand.
if gcloud projects get-iam-policy "$PROJECT" --format=json | grep -q "serviceAccount:$QUERY_SA" ||
  gcloud storage buckets get-iam-policy "gs://$BUCKET" --format=json | grep -q "serviceAccount:$QUERY_SA"; then
  echo "ERROR: $QUERY_SA has a role in the project or the bucket; remove it" >&2
  exit 1
fi

# Spans go to Cloud Trace via the Telemetry API (src/telemetry.rs).
for sa in "$JOB_SA" "$WEB_SA"; do
  gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:$sa" --role roles/telemetry.tracesWriter --condition=None >/dev/null
done

say "Alerting ($ALERT_EMAIL)"
# One e-mail channel and two policies: any failed run of the sync job opens an
# incident (one mail), which closes itself once runs succeed again; an ERROR
# log of the web service (incl. 5xx requests) mails at most once an hour.
channel=$(gcloud beta monitoring channels list --project "$PROJECT" \
  --filter='displayName="fitdata alerts" AND type="email"' --format='value(name)' | head -1)
if [[ -z $channel ]]; then
  channel=$(gcloud beta monitoring channels create --project "$PROJECT" --display-name 'fitdata alerts' \
    --type email --channel-labels "email_address=$ALERT_EMAIL" --format='value(name)')
fi
apply_policy() { # display name, template
  local file policy
  file=$(mktemp)
  sed -e "s|__CHANNEL__|$channel|; s|__JOB__|$JOB_NAME|g; s|__SERVICE__|$SERVICE_NAME|g; s|__PROJECT__|$PROJECT|g; s|__REGION__|$REGION|g; s|__DOMAIN__|$DOMAIN|g" \
    "$2" > "$file"
  policy=$(gcloud beta monitoring policies list --project "$PROJECT" \
    --filter="displayName=\"$1\"" --format='value(name)' | head -1)
  if [[ -n $policy ]]; then
    gcloud beta monitoring policies update "$policy" --project "$PROJECT" --policy-from-file "$file" >/dev/null
  else
    gcloud beta monitoring policies create --project "$PROJECT" --policy-from-file "$file" >/dev/null
  fi
  rm -f "$file"
  echo "  policy '$1' -> $channel"
}
apply_policy "fitdata-sync failed" deploy/alert-policy.json
apply_policy "fitdata-web errors" deploy/alert-web-errors.json

say "GitHub Actions deploy ($GITHUB_REPO)"
# Deployer: what the workflow (pushes the image) and deploy/deploy.sh need.
# roles/viewer is needed to stream logs of `gcloud builds submit` from the
# legacy Cloud Build logs bucket.
for role in roles/run.admin roles/cloudbuild.builds.editor roles/cloudscheduler.admin \
  roles/artifactregistry.writer roles/serviceusage.serviceUsageConsumer roles/viewer; do
  gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:$DEPLOY_SA" --role "$role" --condition=None >/dev/null
done
gcloud storage buckets add-iam-policy-binding "gs://${PROJECT}_cloudbuild" \
  --member "serviceAccount:$DEPLOY_SA" --role roles/storage.admin >/dev/null
# Run the job and web as their SAs, schedule as the scheduler SA, and run
# Cloud Build as its default (compute) SA.
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT" --format 'value(projectNumber)')
for sa in "$JOB_SA" "$WEB_SA" "$SCHEDULER_SA" "$QUERY_SA" "$PROJECT_NUMBER-compute@developer.gserviceaccount.com"; do
  gcloud iam service-accounts add-iam-policy-binding "$sa" --project "$PROJECT" \
    --member "serviceAccount:$DEPLOY_SA" --role roles/iam.serviceAccountUser >/dev/null
done
if ! gcloud iam workload-identity-pools describe "$WIF_POOL" --project "$PROJECT" --location global >/dev/null 2>&1; then
  gcloud iam workload-identity-pools create "$WIF_POOL" --project "$PROJECT" --location global \
    --display-name "GitHub Actions"
fi
# Only workflows of this repo running on main can get a token.
WIF_ARGS=(--project "$PROJECT" --location global --workload-identity-pool "$WIF_POOL"
  --attribute-mapping "google.subject=assertion.sub,attribute.repository=assertion.repository,attribute.ref=assertion.ref"
  --attribute-condition "assertion.repository == '$GITHUB_REPO' && assertion.ref == 'refs/heads/main'")
if gcloud iam workload-identity-pools providers describe "$WIF_PROVIDER" --project "$PROJECT" \
  --location global --workload-identity-pool "$WIF_POOL" >/dev/null 2>&1; then
  gcloud iam workload-identity-pools providers update-oidc "$WIF_PROVIDER" "${WIF_ARGS[@]}" >/dev/null
else
  gcloud iam workload-identity-pools providers create-oidc "$WIF_PROVIDER" "${WIF_ARGS[@]}" \
    --issuer-uri https://token.actions.githubusercontent.com >/dev/null
fi
gcloud iam service-accounts add-iam-policy-binding "$DEPLOY_SA" --project "$PROJECT" \
  --member "principalSet://iam.googleapis.com/projects/$PROJECT_NUMBER/locations/global/workloadIdentityPools/$WIF_POOL/attribute.repository/$GITHUB_REPO" \
  --role roles/iam.workloadIdentityUser >/dev/null
echo "  provider projects/$PROJECT_NUMBER/locations/global/workloadIdentityPools/$WIF_POOL/providers/$WIF_PROVIDER"

say "Done"
