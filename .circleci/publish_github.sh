#!/usr/bin/env bash
# Publish a GitHub release for the resolved tag, attaching every archive
# under dist/ + checksums.sha256.
#
# Uses GITHUB_TOKEN from CircleCI project env. The token needs
# Contents:Write on the artui repo. Releases are created via the gh CLI
# (already on cimg/rust:1.95) so we don't need a separate gh action.

set -euo pipefail

if [ -z "${GITHUB_TOKEN:-}" ]; then
  echo "ERROR: GITHUB_TOKEN not set in CircleCI project env" >&2
  echo "       Generate at https://github.com/settings/tokens with Contents:Write" >&2
  exit 1
fi

# Strip any whitespace / CR / LF from the token. CircleCI's env var
# UI doesn't trim trailing newlines on paste; gh CLI then sends an
# Authorization header with a stray \n which GitHub returns as either
# 401 Bad credentials (classic PATs) or 404 Not Found (fine-grained).
# Same defensive normalization we do in upload_r2.sh.
GITHUB_TOKEN="$(printf '%s' "$GITHUB_TOKEN" | tr -d ' \t\r\n')"

# Validate the token shape. GitHub PATs come in two flavours:
#   classic:        ghp_<36 chars>   → 40 chars total
#   fine-grained:   github_pat_<82+ chars>
# Length sanity-check both. A 16-char or 32-char token is wrong (likely
# a Cloudflare token pasted into the wrong slot).
TOKEN_LEN="${#GITHUB_TOKEN}"
if [ "$TOKEN_LEN" -lt 30 ] || [ "$TOKEN_LEN" -gt 200 ]; then
  echo "ERROR: GITHUB_TOKEN is $TOKEN_LEN chars; expected 40 (classic ghp_) or 90+ (github_pat_)." >&2
  echo "       Did you paste a Cloudflare token into this slot?" >&2
  exit 1
fi
echo "GITHUB_TOKEN sanity check: ${GITHUB_TOKEN:0:6}…${GITHUB_TOKEN: -4} ($TOKEN_LEN chars)"

TAG="$(cat /tmp/release-meta/tag)"
REPO="${GITHUB_REPO:-lucasram20/artui}"

# gh authenticates via GH_TOKEN/GITHUB_TOKEN automatically.
export GH_TOKEN="$GITHUB_TOKEN"

# Install gh on cimg/rust:1.95 if it isn't already present.
if ! command -v gh >/dev/null 2>&1; then
  echo "Installing gh CLI"
  curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | \
    sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | \
    sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
  sudo apt-get update -qq
  sudo apt-get install -y -qq gh
fi
gh --version

cd dist
shopt -s nullglob
ASSETS=(artui-*.tar.gz artui-*.zip checksums.sha256)
shopt -u nullglob

if [ "${#ASSETS[@]}" -eq 0 ]; then
  echo "ERROR: dist/ has no artifacts to publish" >&2
  exit 1
fi

# Use --notes-from-tag so the release body comes from the annotated tag
# message when there is one, otherwise generate from commits since the
# previous tag.
if gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  echo "Release $TAG already exists — uploading artifacts (and overwriting any duplicates)."
  gh release upload "$TAG" "${ASSETS[@]}" -R "$REPO" --clobber
else
  echo "Creating release $TAG with ${#ASSETS[@]} assets"
  gh release create "$TAG" "${ASSETS[@]}" \
    -R "$REPO" \
    --title "artui $TAG" \
    --generate-notes
fi

echo "GitHub release $TAG published."
