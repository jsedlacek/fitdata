# Shared settings for deploy/*.sh (sourced, not executed).
PROJECT=${PROJECT:-fitdata-jsedlacek}
# Everything (data, job, web) lives here; europe-west1 has Cloud Run domain
# mappings, which e.g. europe-west3 lacks.
REGION=${REGION:-europe-west1}
BUCKET=${BUCKET:-$PROJECT-data}
DATASET=${DATASET:-health}
REPO=${REPO:-fitdata}
TIME_ZONE=${TIME_ZONE:-Europe/Prague}
# When the sync job runs (cron in TIME_ZONE); each run re-fetches the last 7 days.
SCHEDULE=${SCHEDULE:-"0 * * * *"}
# Custom domain of the dashboard; must be verified for the gcloud account
# (gcloud domains list-user-verified). Empty means the Cloud Run URL.
DOMAIN=${DOMAIN:-fitdata.jakub.contact}
# Google accounts allowed to sign in to the dashboard (comma separated).
ALLOWED_EMAILS=${ALLOWED_EMAILS:-sedlacek.jakub@gmail.com}
# Where Cloud Monitoring sends an e-mail when a sync run fails.
ALERT_EMAIL=${ALERT_EMAIL:-sedlacek.jakub@gmail.com}

JOB_SA=fitdata-job@$PROJECT.iam.gserviceaccount.com
WEB_SA=fitdata-web@$PROJECT.iam.gserviceaccount.com
SCHEDULER_SA=fitdata-scheduler@$PROJECT.iam.gserviceaccount.com
# GitHub Actions deploys every push to main of this repo as this SA, keyless
# via Workload Identity Federation (.github/workflows/deploy.yml).
DEPLOY_SA=fitdata-deployer@$PROJECT.iam.gserviceaccount.com
GITHUB_REPO=${GITHUB_REPO:-jsedlacek/fitdata}
WIF_POOL=github
WIF_PROVIDER=github

# Image tag: the last commit that touched what goes into the image, so changes
# to deploy scripts or docs reuse the existing image.
IMAGE_SOURCES=(Dockerfile .dockerignore Cargo.toml Cargo.lock src frontend)
TAG=$(git log -1 --format=%h -- "${IMAGE_SOURCES[@]}")$(git diff --quiet HEAD -- "${IMAGE_SOURCES[@]}" || echo "-dirty")
IMAGE=$REGION-docker.pkg.dev/$PROJECT/$REPO/fitdata:$TAG

JOB_NAME=fitdata-sync
SERVICE_NAME=fitdata-web
