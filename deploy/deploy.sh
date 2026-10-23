#!/usr/bin/env bash
# Builds the image with Cloud Build and deploys the daily job, its schedule
# and the dashboard. Run deploy/setup.sh first.
#
#   deploy/deploy.sh
set -euo pipefail
cd "$(dirname "$0")/.."
source deploy/config.sh

TAG=$(git rev-parse --short HEAD)$(git diff --quiet HEAD -- || echo "-dirty")
IMAGE=$REGION-docker.pkg.dev/$PROJECT/$REPO/fitdata:$TAG
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT" --format 'value(projectNumber)')
# Cloud Run's deterministic URL; it must be an authorized redirect URI
# (+ /auth/callback) of the OAuth Web client.
PUBLIC_URL=https://$SERVICE_NAME-$PROJECT_NUMBER.$REGION.run.app

ENV=GCP_PROJECT=$PROJECT,GCP_REGION=$REGION,STORAGE=gs://$BUCKET,BQ_DATASET=$DATASET,TZ=$TIME_ZONE,RUST_LOG=fitdata=info

echo "== Build $IMAGE"
gcloud builds submit --project "$PROJECT" --region "$REGION" --tag "$IMAGE" .

echo "== Job $JOB_NAME"
gcloud run jobs deploy "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --image "$IMAGE" --args job,--days=7 \
  --service-account "$JOB_SA" --set-env-vars "$ENV" \
  --cpu 1 --memory 1Gi --task-timeout 30m --max-retries 1

echo "== Schedule (daily 10:00 $TIME_ZONE)"
gcloud run jobs add-iam-policy-binding "$JOB_NAME" --project "$PROJECT" --region "$REGION" \
  --member "serviceAccount:$SCHEDULER_SA" --role roles/run.invoker >/dev/null
SCHEDULE_ARGS=(--project "$PROJECT" --location "$REGION" --schedule "0 10 * * *" --time-zone "$TIME_ZONE"
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
  --set-env-vars "^;^${ENV//,/;};PUBLIC_URL=$PUBLIC_URL;ALLOWED_EMAILS=$ALLOWED_EMAILS" \
  --allow-unauthenticated --cpu 1 --memory 512Mi --min-instances 0 --max-instances 2

echo
echo "Dashboard: $PUBLIC_URL"
echo "Run the job now: gcloud run jobs execute $JOB_NAME --project $PROJECT --region $REGION --wait"
