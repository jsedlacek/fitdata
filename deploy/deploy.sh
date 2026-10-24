#!/usr/bin/env bash
# Builds the image with Cloud Build and deploys the daily job, its schedule
# and the dashboard. Run deploy/setup.sh first.
#
#   deploy/deploy.sh
set -euo pipefail
cd "$(dirname "$0")/.."
source deploy/config.sh

PROJECT_NUMBER=$(gcloud projects describe "$PROJECT" --format 'value(projectNumber)')
# Google redirects to $PUBLIC_URL/auth/callback, which must be an authorized
# redirect URI of the OAuth Web client.
if [[ -n $DOMAIN ]]; then
  PUBLIC_URL=https://$DOMAIN
else
  PUBLIC_URL=https://$SERVICE_NAME-$PROJECT_NUMBER.$REGION.run.app
fi

# APP_VERSION tags traces and Error Reporting groups with the image.
ENV=GCP_PROJECT=$PROJECT,GCP_REGION=$REGION,STORAGE=gs://$BUCKET,KMS_KEY=$KMS_KEY,TZ=$TIME_ZONE,RUST_LOG=fitdata=info,APP_VERSION=$TAG

# GitHub Actions builds and pushes the image itself (with a layer cache)
# before running this, so there it is always found here.
if [[ $TAG != *-dirty ]] && gcloud artifacts docker images describe "$IMAGE" >/dev/null 2>&1; then
  echo "== Image $IMAGE already built"
else
  echo "== Build $IMAGE"
  gcloud builds submit --project "$PROJECT" --region "$REGION" --tag "$IMAGE" .
fi

echo "== Job $JOB_NAME"
gcloud run jobs deploy "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args job,--days=7 \
  --service-account "$JOB_SA" --set-env-vars "$ENV" \
  --cpu 1 --memory 1Gi --task-timeout 30m --max-retries 1

echo "== Schedule ($SCHEDULE $TIME_ZONE)"
gcloud run jobs add-iam-policy-binding "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --member "serviceAccount:$SCHEDULER_SA" --role roles/run.invoker >/dev/null
# The dashboard's "sync now" button starts the job for one user (with
# overridden arguments) and watches its executions.
for role in roles/run.jobsExecutorWithOverrides roles/run.viewer; do
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

echo "== Service $QUERY_SERVICE"
# Runs MCP `query` SQL; only the web may call it. Its service account has no
# roles (setup.sh checks), it gets one user's files with each request and
# handles one request at a time. No GCP_PROJECT: it exports no traces, which
# would need a role.
QUERY_URL=https://$QUERY_SERVICE-$PROJECT_NUMBER.$REGION.run.app
gcloud run deploy "$QUERY_SERVICE" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args query-serve \
  --service-account "$QUERY_SA" \
  --set-env-vars "RUST_LOG=fitdata=info,APP_VERSION=$TAG" \
  --no-allow-unauthenticated --use-http2 --concurrency 1 --timeout 60 \
  --cpu 1 --memory 1Gi --min-instances 0 --max-instances 4
gcloud run services add-iam-policy-binding "$QUERY_SERVICE" --project "$PROJECT" --region "$REGION" \
  --member "serviceAccount:$WEB_SA" --role roles/run.invoker >/dev/null

echo "== Service $SERVICE_NAME"
# Public endpoint; the app itself requires Google sign-in (ALLOWED_EMAILS).
gcloud run deploy "$SERVICE_NAME" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args serve \
  --service-account "$WEB_SA" \
  --set-env-vars "^;^${ENV//,/;};PUBLIC_URL=$PUBLIC_URL;ALLOWED_EMAILS=$ALLOWED_EMAILS;SYNC_JOB=$JOB_NAME;QUERY_URL=$QUERY_URL" \
  --allow-unauthenticated --cpu 1 --memory 1Gi --min-instances 0 --max-instances 2

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
