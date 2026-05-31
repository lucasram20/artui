#!/usr/bin/env bash
# Upload release artifacts + install scripts to Cloudflare R2.
#
# Reads the tag from /tmp/release-meta/tag (populated by the resolve_tag
# command). Uses R2_ACCOUNT_ID, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
# from the CircleCI project env. Skipped silently when R2_ACCOUNT_ID is
# unset so a forked repo without R2 credentials can still ship through
# CI without modification.

set -euo pipefail

if [ -z "${R2_ACCOUNT_ID:-}" ] || [ -z "${R2_ACCESS_KEY_ID:-}" ] || [ -z "${R2_SECRET_ACCESS_KEY:-}" ]; then
  echo "R2 credentials not set; skipping R2 upload (artifacts still go to GitHub Releases)."
  exit 0
fi

TAG="$(cat /tmp/release-meta/tag)"
ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"
BUCKET="artui-releases"

export AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID"
export AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
export AWS_DEFAULT_REGION="auto"
export AWS_EC2_METADATA_DISABLED="true"

cd dist
shopt -s nullglob
ASSETS=(artui-*.tar.gz artui-*.zip checksums.sha256)
shopt -u nullglob
if [ "${#ASSETS[@]}" -eq 0 ]; then
  echo "ERROR: dist/ has no artifacts to upload" >&2
  exit 1
fi

for asset in "${ASSETS[@]}"; do
  [ -f "$asset" ] || continue
  echo "Uploading $asset → s3://${BUCKET}/${TAG}/${asset}"
  aws s3 cp "$asset" "s3://${BUCKET}/${TAG}/${asset}" \
    --endpoint-url "$ENDPOINT" --no-progress

  echo "Uploading $asset → s3://${BUCKET}/latest/${asset}"
  aws s3 cp "$asset" "s3://${BUCKET}/latest/${asset}" \
    --endpoint-url "$ENDPOINT" --no-progress
done

echo "Uploading install scripts to R2"
cd ..
aws s3 cp scripts/install.sh "s3://${BUCKET}/install.sh" \
  --endpoint-url "$ENDPOINT" \
  --content-type "text/x-shellscript" \
  --cache-control "public, max-age=300" --no-progress
aws s3 cp scripts/install.ps1 "s3://${BUCKET}/install.ps1" \
  --endpoint-url "$ENDPOINT" \
  --content-type "text/plain; charset=utf-8" \
  --cache-control "public, max-age=300" --no-progress

echo "R2 upload complete for $TAG."
