#!/usr/bin/env bash
# Sync a published GitHub release to the public R2 mirror (versioned + latest/).
#
# Usage:
#   export R2_ACCOUNT_ID=... R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=...
#   ./scripts/sync-r2-mirror.sh v0.7.0
#
# Requires: aws (AWS CLI v2), gh (authenticated), curl
set -euo pipefail

TAG="${1:-}"
REPO="${ARTUI_REPO:-lucasram20/artui}"
BUCKET="${ARTUI_R2_BUCKET:-artui-releases}"

if [ -z "$TAG" ]; then
  echo "usage: $0 <tag>   e.g. v0.7.0" >&2
  exit 1
fi

for var in R2_ACCOUNT_ID R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY; do
  if [ -z "${!var:-}" ]; then
    echo "missing env: $var" >&2
    exit 1
  fi
done

if ! command -v aws >/dev/null 2>&1; then
  echo "aws CLI not found" >&2
  exit 1
fi
if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI not found" >&2
  exit 1
fi

R2_ACCOUNT_ID="$(printf '%s' "$R2_ACCOUNT_ID" | tr -d ' \t\r\n')"
R2_ACCESS_KEY_ID="$(printf '%s' "$R2_ACCESS_KEY_ID" | tr -d ' \t\r\n')"
R2_SECRET_ACCESS_KEY="$(printf '%s' "$R2_SECRET_ACCESS_KEY" | tr -d ' \t\r\n')"

if [ "${#R2_ACCESS_KEY_ID}" -ne 32 ]; then
  echo "R2_ACCESS_KEY_ID must be 32 chars (S3-compatible token), got ${#R2_ACCESS_KEY_ID}" >&2
  exit 1
fi
if [ "${#R2_SECRET_ACCESS_KEY}" -ne 64 ]; then
  echo "R2_SECRET_ACCESS_KEY must be 64 chars, got ${#R2_SECRET_ACCESS_KEY}" >&2
  exit 1
fi

export AWS_ACCESS_KEY_ID="$R2_ACCESS_KEY_ID"
export AWS_SECRET_ACCESS_KEY="$R2_SECRET_ACCESS_KEY"
export AWS_DEFAULT_REGION="auto"
export AWS_EC2_METADATA_DISABLED="true"
R2_ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"

VER="${TAG#v}"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

echo "Downloading GitHub release assets for ${TAG}…"
gh release download "$TAG" --repo "$REPO" --dir "$WORKDIR"

echo "Uploading to s3://${BUCKET}/${TAG}/ and latest/…"
echo "$VER" > "$WORKDIR/VERSION"
aws s3 cp "$WORKDIR/VERSION" "s3://${BUCKET}/latest/VERSION" \
  --endpoint-url "$R2_ENDPOINT" --content-type "text/plain" --no-progress

shopt -s nullglob
for asset in "$WORKDIR"/artui-*.tar.gz "$WORKDIR"/artui-*.zip "$WORKDIR"/checksums.sha256; do
  base="$(basename "$asset")"
  aws s3 cp "$asset" "s3://${BUCKET}/${TAG}/${base}" \
    --endpoint-url "$R2_ENDPOINT" --no-progress
  aws s3 cp "$asset" "s3://${BUCKET}/latest/${base}" \
    --endpoint-url "$R2_ENDPOINT" --no-progress
  echo "  ↑ ${base}"
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
aws s3 cp "$ROOT/scripts/install.sh" "s3://${BUCKET}/install.sh" \
  --endpoint-url "$R2_ENDPOINT" \
  --content-type "text/x-shellscript" \
  --cache-control "public, max-age=300" --no-progress
aws s3 cp "$ROOT/scripts/install.ps1" "s3://${BUCKET}/install.ps1" \
  --endpoint-url "$R2_ENDPOINT" \
  --content-type "text/plain; charset=utf-8" \
  --cache-control "public, max-age=300" --no-progress

PUBLIC_BASE="${ARTUI_MIRROR_BASE:-https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev}"
echo "Verifying public mirror…"
CHECKSUMS="$(curl -fsSL "${PUBLIC_BASE}/latest/checksums.sha256")"
if echo "$CHECKSUMS" | grep -q "artui-${VER}-"; then
  echo "OK: latest/checksums.sha256 references artui-${VER}-*"
else
  echo "WARN: latest/checksums.sha256 does not mention artui-${VER}:" >&2
  echo "$CHECKSUMS" >&2
  exit 1
fi

REMOTE_VER="$(curl -fsSL "${PUBLIC_BASE}/latest/VERSION" | tr -d '\r\n')"
if [ "$REMOTE_VER" = "$VER" ]; then
  echo "OK: latest/VERSION = ${VER}"
else
  echo "WARN: latest/VERSION is '${REMOTE_VER}', expected '${VER}'" >&2
  exit 1
fi

echo "R2 mirror synced for ${TAG}."