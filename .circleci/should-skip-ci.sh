#!/usr/bin/env bash
# Halt the current CircleCI job when the commit only touches docs /
# config files that don't affect compiled artifacts.
#
# Mirrors the original GHA `paths:` filter shape — CircleCI doesn't
# have an equivalent declarative filter on the OSS plan, so we do
# the check at the start of each CI job and exit gracefully via
# `circleci-agent step halt` when nothing build-relevant changed.
#
# Files that DO trigger CI:
#   - src/**, tests/**, benches/**, examples/**
#   - Cargo.toml, Cargo.lock, build.rs
#   - .circleci/**, scripts/install.*, .github/workflows/**
#
# Files that DON'T trigger CI (this list is what we filter out):
#   - **/*.md, **/*.mdx, **/LICENSE*, **/NOTICE*
#   - docs/**, README.md, CHANGELOG.md
#   - .gitignore, .gitattributes, .editorconfig
#   - npm/** (the npm wrapper publishes separately)
#   - cloudflare/** (the freemodel relay deploys separately)
#   - .vscode/**, .idea/**
#
# Override: any commit message containing `[ci force]` runs CI even on
# docs-only changes. `[ci skip]` / `[skip ci]` is honored by CircleCI
# natively at the webhook layer, so we don't have to handle it here.

set -euo pipefail

if ! command -v git >/dev/null 2>&1; then
  echo "git missing — skipping path-filter check"
  exit 0
fi

# CircleCI checks out detached at the commit, so HEAD is the commit
# we want. `git diff-tree` is more reliable than `git log --name-only`
# for merge commits because it diffs against the first parent only.
CHANGED="$(git diff-tree --no-commit-id --name-only -r --no-renames HEAD 2>/dev/null || true)"

if [ -z "$CHANGED" ]; then
  echo "No changed files detected (initial commit or shallow clone); running CI."
  exit 0
fi

COMMIT_MSG="$(git log -1 --pretty=%B 2>/dev/null || true)"
if echo "$COMMIT_MSG" | grep -qiE '\[ci force\]|\[force ci\]'; then
  echo "[ci force] marker found in commit message; running CI."
  exit 0
fi

# Patterns that match files we DON'T want CI to fire on. If every
# changed file matches at least one of these, the job halts.
IGNORE_PATTERNS=(
  '^.*\.md$'
  '^.*\.mdx$'
  '^.*/LICENSE.*$'
  '^LICENSE.*$'
  '^.*/NOTICE.*$'
  '^NOTICE.*$'
  '^docs/'
  '^README\.md$'
  '^CHANGELOG\.md$'
  '^\.gitignore$'
  '^\.gitattributes$'
  '^\.editorconfig$'
  '^npm/'
  '^cloudflare/'
  '^\.vscode/'
  '^\.idea/'
  '^scripts/sync-helix-lsp\.py$'
)

ALL_IGNORED=true
RELEVANT_COUNT=0
for file in $CHANGED; do
  matched=false
  for pat in "${IGNORE_PATTERNS[@]}"; do
    if echo "$file" | grep -qE "$pat"; then
      matched=true
      break
    fi
  done
  if [ "$matched" = false ]; then
    ALL_IGNORED=false
    RELEVANT_COUNT=$((RELEVANT_COUNT + 1))
    # Print first few mismatches so the log shows why we ran.
    if [ "$RELEVANT_COUNT" -le 5 ]; then
      echo "Build-relevant: $file"
    fi
  fi
done

if [ "$ALL_IGNORED" = true ]; then
  echo ""
  echo "All $(echo "$CHANGED" | wc -w) changed file(s) match docs/config-only patterns:"
  echo "$CHANGED" | sed 's/^/  - /'
  echo ""
  echo "Halting CI to save credits. Add [ci force] to the commit message to override."
  circleci-agent step halt
  exit 0
fi

echo ""
echo "Found build-relevant changes; running CI."
