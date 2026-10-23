# Shared settings for deploy/*.sh (sourced, not executed).
PROJECT=${PROJECT:-fitdata-jsedlacek}
REGION=${REGION:-europe-west3}
BUCKET=${BUCKET:-$PROJECT-raw}
DATASET=${DATASET:-fitdata}
REPO=${REPO:-fitdata}
TIME_ZONE=${TIME_ZONE:-Europe/Prague}
# Google accounts allowed to sign in to the dashboard (comma separated).
ALLOWED_EMAILS=${ALLOWED_EMAILS:-sedlacek.jakub@gmail.com}

JOB_SA=fitdata-job@$PROJECT.iam.gserviceaccount.com
WEB_SA=fitdata-web@$PROJECT.iam.gserviceaccount.com
SCHEDULER_SA=fitdata-scheduler@$PROJECT.iam.gserviceaccount.com

JOB_NAME=fitdata-sync
SERVICE_NAME=fitdata-web
