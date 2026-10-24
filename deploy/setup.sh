#!/usr/bin/env bash
# One-time (and idempotent) infrastructure for fitdata. Safe to re-run: it
# only creates what is missing and re-applies IAM bindings.
#
#   deploy/setup.sh
#
# Prerequisites done by hand: the project with billing, the OAuth consent
# screen and Web client (docs/setup.md), and the secrets `oauth-client` and
# `refresh-token`.
set -euo pipefail
cd "$(dirname "$0")/.."
source deploy/config.sh

say() { printf '\n== %s\n' "$*"; }

say "APIs"
gcloud services enable --project "$PROJECT" \
  run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com \
  cloudscheduler.googleapis.com bigquery.googleapis.com storage.googleapis.com \
  secretmanager.googleapis.com health.googleapis.com monitoring.googleapis.com \
  iamcredentials.googleapis.com sts.googleapis.com

say "Bucket gs://$BUCKET"
if ! gcloud storage buckets describe "gs://$BUCKET" --project "$PROJECT" >/dev/null 2>&1; then
  gcloud storage buckets create "gs://$BUCKET" --project "$PROJECT" --location "$REGION" \
    --uniform-bucket-level-access --public-access-prevention
fi
# Staged load files are only needed for the duration of a load job.
gcloud storage buckets update "gs://$BUCKET" --lifecycle-file=deploy/bucket-lifecycle.json

say "BigQuery dataset $DATASET"
if ! bq --project_id "$PROJECT" show "$DATASET" >/dev/null 2>&1; then
  bq --project_id "$PROJECT" --location "$REGION" mk --dataset \
    --description "Fitbit / Google Health data" "$PROJECT:$DATASET"
fi
for schema in deploy/schema/*.json; do
  t=$(basename "$schema" .json)
  for table in "$t" "_staging_$t"; do
    if bq --project_id "$PROJECT" show "$DATASET.$table" >/dev/null 2>&1; then
      # Adds new columns; BigQuery refuses incompatible changes.
      bq --project_id "$PROJECT" update "$PROJECT:$DATASET.$table" "$schema" >/dev/null
    elif [[ $table == _staging_* ]]; then
      bq --project_id "$PROJECT" mk --table "$PROJECT:$DATASET.$table" "$schema" >/dev/null
    else
      bq --project_id "$PROJECT" mk --table --time_partitioning_field date --time_partitioning_type DAY \
        "$PROJECT:$DATASET.$table" "$schema" >/dev/null
    fi
    echo "  $table"
  done
done

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
for sa in "$JOB_SA" "$WEB_SA" "$SCHEDULER_SA" "$DEPLOY_SA"; do
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
# Job: read the OAuth client + refresh token, raw data in the bucket, load into BigQuery.
secret_role oauth-client "$JOB_SA" roles/secretmanager.secretAccessor
secret_role refresh-token "$JOB_SA" roles/secretmanager.secretAccessor
# Web: sign-in needs the OAuth client and session key; "Connect Fitbit" adds a
# refresh-token version and disables the older ones (it cannot read any).
secret_role oauth-client "$WEB_SA" roles/secretmanager.secretAccessor
secret_role session-key "$WEB_SA" roles/secretmanager.secretAccessor
secret_role refresh-token "$WEB_SA" roles/secretmanager.secretVersionManager

gcloud storage buckets add-iam-policy-binding "gs://$BUCKET" \
  --member "serviceAccount:$JOB_SA" --role roles/storage.objectAdmin >/dev/null

for sa in "$JOB_SA" "$WEB_SA"; do
  gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:$sa" --role roles/bigquery.jobUser --condition=None >/dev/null
done
# Dataset-level roles via DCL (`bq add-iam-policy-binding` needs allowlisting).
bq --project_id "$PROJECT" --location "$REGION" query --nouse_legacy_sql --quiet "
  GRANT \`roles/bigquery.dataEditor\` ON SCHEMA \`$PROJECT.$DATASET\` TO 'serviceAccount:$JOB_SA';
  GRANT \`roles/bigquery.dataViewer\` ON SCHEMA \`$PROJECT.$DATASET\` TO 'serviceAccount:$WEB_SA';" >/dev/null

say "Alerting ($ALERT_EMAIL)"
# One e-mail channel and one policy: any failed run of the sync job opens an
# incident (one mail), which closes itself once runs succeed again.
channel=$(gcloud beta monitoring channels list --project "$PROJECT" \
  --filter='displayName="fitdata alerts" AND type="email"' --format='value(name)' | head -1)
if [[ -z $channel ]]; then
  channel=$(gcloud beta monitoring channels create --project "$PROJECT" --display-name 'fitdata alerts' \
    --type email --channel-labels "email_address=$ALERT_EMAIL" --format='value(name)')
fi
policy_file=$(mktemp)
sed -e "s|__CHANNEL__|$channel|; s|__JOB__|$JOB_NAME|g; s|__PROJECT__|$PROJECT|g; s|__REGION__|$REGION|g; s|__DOMAIN__|$DOMAIN|g" \
  deploy/alert-policy.json > "$policy_file"
policy=$(gcloud beta monitoring policies list --project "$PROJECT" \
  --filter='displayName="fitdata-sync failed"' --format='value(name)' | head -1)
if [[ -n $policy ]]; then
  gcloud beta monitoring policies update "$policy" --project "$PROJECT" --policy-from-file "$policy_file" >/dev/null
else
  gcloud beta monitoring policies create --project "$PROJECT" --policy-from-file "$policy_file" >/dev/null
fi
rm -f "$policy_file"
echo "  policy 'fitdata-sync failed' -> $channel"

say "GitHub Actions deploy ($GITHUB_REPO)"
# Deployer: what deploy/deploy.sh needs. roles/viewer is needed to stream logs
# of `gcloud builds submit` from the legacy Cloud Build logs bucket.
for role in roles/run.admin roles/cloudbuild.builds.editor roles/cloudscheduler.admin \
  roles/artifactregistry.reader roles/serviceusage.serviceUsageConsumer roles/viewer; do
  gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:$DEPLOY_SA" --role "$role" --condition=None >/dev/null
done
gcloud storage buckets add-iam-policy-binding "gs://${PROJECT}_cloudbuild" \
  --member "serviceAccount:$DEPLOY_SA" --role roles/storage.admin >/dev/null
# Run the job and web as their SAs, schedule as the scheduler SA, and run
# Cloud Build as its default (compute) SA.
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT" --format 'value(projectNumber)')
for sa in "$JOB_SA" "$WEB_SA" "$SCHEDULER_SA" "$PROJECT_NUMBER-compute@developer.gserviceaccount.com"; do
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
