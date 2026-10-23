# Shared settings for deploy/*.sh (sourced, not executed).
PROJECT=${PROJECT:-fitdata-jsedlacek}
REGION=${REGION:-europe-west3}
BUCKET=${BUCKET:-$PROJECT-raw}
DATASET=${DATASET:-fitdata}
REPO=${REPO:-fitdata}
TIME_ZONE=${TIME_ZONE:-Europe/Prague}
# The dashboard runs in a region with Cloud Run domain mappings ($REGION has
# none); it still queries BigQuery in $REGION.
WEB_REGION=${WEB_REGION:-europe-west1}
# Custom domain of the dashboard; must be verified for the gcloud account
# (gcloud domains list-user-verified). Empty means the Cloud Run URL.
DOMAIN=${DOMAIN:-fitdata.jakub.contact}
# Google accounts allowed to sign in to the dashboard (comma separated).
ALLOWED_EMAILS=${ALLOWED_EMAILS:-sedlacek.jakub@gmail.com}

JOB_SA=fitdata-job@$PROJECT.iam.gserviceaccount.com
WEB_SA=fitdata-web@$PROJECT.iam.gserviceaccount.com
SCHEDULER_SA=fitdata-scheduler@$PROJECT.iam.gserviceaccount.com

JOB_NAME=fitdata-sync
SERVICE_NAME=fitdata-web
