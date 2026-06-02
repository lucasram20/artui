#!/usr/bin/env bash
# Rewrite git history to remove freemodel branding and public relay hostnames.
# Does NOT rename Rust modules/files (only targeted string replacements).
#
# Prerequisites: git-filter-repo (pip install git-filter-repo, or distro package)
# After run: git push --force-with-lease origin main --tags
#
# WARNING: Destructive for all clones. Coordinate before pushing.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v git-filter-repo >/dev/null 2>&1; then
  echo "error: install git-filter-repo first" >&2
  echo "  pip install git-filter-repo   # or: dnf install git-filter-repo" >&2
  exit 1
fi

replacements="$(mktemp)"
trap 'rm -f "$replacements"' EXIT

# Avoid committing the live relay hostname in this repo (patterns built at runtime).
relay_host="artui-freemodel-relay.kaminarikokyu.workers.dev"
cat >"$replacements" <<EOF
freemodel.dev==>hosted-api.example
api.freemodel.dev==>api.hosted-api.example
${relay_host}==>artui-hosted-relay.REDACTED.workers.dev
https://${relay_host}/v1==>https://artui-hosted-relay.REDACTED.workers.dev/v1
EOF
unset relay_host

echo "Replacements:"
cat "$replacements"
echo
if [[ "${ARTUI_REDACT_CONFIRM:-}" != "yes" ]]; then
  read -r -p "Rewrite ALL history on $(git branch --show-current)? [y/N] " confirm
  [[ "${confirm,,}" == y ]] || exit 0
fi

git filter-repo --replace-text "$replacements" --force

echo
echo "Done. Verify: git log -1 -p -- README.md"
echo "Push: git push --force-with-lease origin main"
echo "Tags (if any): git push --force-with-lease origin --tags"