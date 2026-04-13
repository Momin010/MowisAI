#!/bin/bash

# Setup gcloud with service account
if [ ! -f ~/google-cloud-sdk/bin/gcloud ]; then
    curl -sSL https://sdk.cloud.google.com | bash -s -- --disable-prompts
fi

source ~/google-cloud-sdk/path.bash.inc

# Authenticate with service account secret
if [ -n "$GCP_SERVICE_ACCOUNT_KEY" ]; then
    echo "$GCP_SERVICE_ACCOUNT_KEY" > /tmp/sa-key.json
    gcloud auth activate-service-account --key-file=/tmp/sa-key.json
    gcloud config set project company-internal-tools-490516
    rm /tmp/sa-key.json
fi

echo "✅ gcloud ready"
