#!/usr/bin/env bash
# Builds the image with Cloud Build and deploys the daily job, its schedule
# and the dashboard. Run deploy/setup.sh first.
#
#   deploy/deploy.sh
set -euo pipefail
cd "$(dirname "$0")/.."
source deploy/config.sh
source deploy/schema.sh

PROJECT_NUMBER=$(gcloud projects describe "$PROJECT" --format 'value(projectNumber)')
# Google redirects to $PUBLIC_URL/auth/callback, which must be an authorized
# redirect URI of the OAuth Web client.
if [[ -n $DOMAIN ]]; then
  PUBLIC_URL=https://$DOMAIN
else
  PUBLIC_URL=https://$SERVICE_NAME-$PROJECT_NUMBER.$REGION.run.app
fi

# APP_VERSION tags traces and Error Reporting groups with the image.
ENV=GCP_PROJECT=$PROJECT,GCP_REGION=$REGION,STORAGE=gs://$BUCKET,BQ_DATASET=$DATASET,OWNER_ID=$OWNER_ID,TZ=$TIME_ZONE,RUST_LOG=fitdata=info,APP_VERSION=$TAG

# GitHub Actions builds and pushes the image itself (with a layer cache)
# before running this, so there it is always found here.
if [[ $TAG != *-dirty ]] && gcloud artifacts docker images describe "$IMAGE" >/dev/null 2>&1; then
  echo "== Image $IMAGE already built"
else
  echo "== Build $IMAGE"
  gcloud builds submit --project "$PROJECT" --region "$REGION" --tag "$IMAGE" .
fi

# Before the job, so new code never writes to the old schema.
echo "== BigQuery tables"
apply_schemas

echo "== Job $JOB_NAME"
gcloud run jobs deploy "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args job,--days=7 \
  --service-account "$JOB_SA" --set-env-vars "$ENV" \
  --cpu 1 --memory 1Gi --task-timeout 30m --max-retries 1

echo "== Schedule ($SCHEDULE $TIME_ZONE)"
gcloud run jobs add-iam-policy-binding "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --member "serviceAccount:$SCHEDULER_SA" --role roles/run.invoker >/dev/null
# The dashboard's "sync now" button starts the job and watches its executions.
for role in roles/run.jobsExecutor roles/run.viewer; do
  gcloud run jobs add-iam-policy-binding "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
    --member "serviceAccount:$WEB_SA" --role "$role" >/dev/null
done
SCHEDULE_ARGS=(--project "$PROJECT" --location "$REGION" --schedule "$SCHEDULE" --time-zone "$TIME_ZONE"
  --uri "https://run.googleapis.com/v2/projects/$PROJECT/locations/$REGION/jobs/$JOB_NAME:run"
  --http-method POST --oauth-service-account-email "$SCHEDULER_SA")
if gcloud scheduler jobs describe "$JOB_NAME-daily" --project "$PROJECT" --location "$REGION" >/dev/null 2>&1; then
  gcloud scheduler jobs update http "$JOB_NAME-daily" "${SCHEDULE_ARGS[@]}"
else
  gcloud scheduler jobs create http "$JOB_NAME-daily" "${SCHEDULE_ARGS[@]}"
fi

echo "== Service $SERVICE_NAME"
# Public endpoint; the app itself requires Google sign-in (ALLOWED_EMAILS).
gcloud run deploy "$SERVICE_NAME" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args serve \
  --service-account "$WEB_SA" \
  --set-env-vars "^;^${ENV//,/;};PUBLIC_URL=$PUBLIC_URL;ALLOWED_EMAILS=$ALLOWED_EMAILS;SYNC_JOB=$JOB_NAME" \
  --allow-unauthenticated --cpu 1 --memory 512Mi --min-instances 0 --max-instances 2

if [[ -n $DOMAIN ]]; then
  echo "== Domain mapping $DOMAIN"
  if ! gcloud beta run domain-mappings describe --domain "$DOMAIN" --project "$PROJECT" --region "$REGION" >/dev/null 2>&1; then
    gcloud beta run domain-mappings create --service "$SERVICE_NAME" --domain "$DOMAIN" \
      --project "$PROJECT" --region "$REGION"
  fi
  echo "DNS records (Cloudflare: DNS only, not proxied):"
  gcloud beta run domain-mappings describe --domain "$DOMAIN" --project "$PROJECT" --region "$REGION" \
    --format 'table(status.resourceRecords[].name, status.resourceRecords[].type, status.resourceRecords[].rrdata)'
fi

echo
echo "Dashboard: $PUBLIC_URL"
echo "Run the job now: gcloud run jobs execute $JOB_NAME --project $PROJECT --region $REGION --wait"
