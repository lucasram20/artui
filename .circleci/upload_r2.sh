#!/usr/bin/env bash
# Upload release artifacts + install scripts to Cloudflare R2.
#
# Reads the tag from /tmp/release-meta/tag (populated by the resolve_tag
# command). Uses R2_ACCOUNT_ID, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
# from the CircleCI project env. Skipped silently when R2_ACCOUNT_ID is
# unset so a forked repo without R2 credentials can still ship through
# CI without modification.
#
# IMPORTANT: R2 needs an S3-compatible API token, not a generic
# Cloudflare API token. The S3-compatible Access Key ID is exactly
# 32 hex chars; the secret is ~64 chars. If you see a "Credential
# access key has length 16, should be 32" error, you generated a
# generic Cloudflare token (the OAuth-style ones). Recreate from
# Cloudflare dashboard → R2 → "Manage R2 API Tokens" →
# "Create R2 API Token" with `Object Read & Write` scoped to the
# artui-releases bucket.

set -euo pipefail

if [ -z "${R2_ACCOUNT_ID:-}" ] || [ -z "${R2_ACCESS_KEY_ID:-}" ] || [ -z "${R2_SECRET_ACCESS_KEY:-}" ]; then
  echo "R2 credentials not set; skipping R2 upload (artifacts still go to GitHub Releases)."
  exit 0
fi

# Validate the access key length up front so the failure message is
# useful. R2's S3 API rejects 16-char keys (those are generic
# Cloudflare API tokens, not S3-compatible R2 tokens) with a confusing
# "Credential access key has length 16, should be 32" error from the
# AWS SDK side. Failing here with a clearer pointer saves a debug round
# trip.
ACCESS_KEY_LEN="${#R2_ACCESS_KEY_ID}"
if [ "$ACCESS_KEY_LEN" -ne 32 ]; then
  echo "ERROR: R2_ACCESS_KEY_ID is $ACCESS_KEY_LEN chars; R2's S3-compatible API requires exactly 32 chars." >&2
  echo "       You probably generated a generic Cloudflare API token." >&2
  echo "       Fix: Cloudflare dashboard -> R2 -> Manage R2 API Tokens -> Create R2 API Token" >&2
  echo "            with Object Read & Write scope on the artui-releases bucket." >&2
  echo "       The Access Key ID shown in the post-create dialog is the 32-char value." >&2
  exit 1
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
