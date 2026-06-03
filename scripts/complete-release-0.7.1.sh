#!/usr/bin/env bash
# Complete release 0.7.1: commit (if needed), history redaction, tag, push, GitHub release.
# Logs to /tmp/artui-release-0.7.1.log (override with ARTUI_RELEASE_LOG).
set -euo pipefail
cd "$(dirname "$0")/.."

LOG="${ARTUI_RELEASE_LOG:-/tmp/artui-release-0.7.1.log}"
exec > >(tee -a "$LOG") 2>&1

echo "=== artui release 0.7.1 === $(date -Is)"
echo "Log: $LOG"
echo "Repo: $(pwd)"
echo

phase1() {
  echo "=== PHASE 1: Pre-redact commit ==="
  git status -sb
  echo

  if ! git diff --quiet || ! git diff --cached --quiet || [ -n "$(git ls-files --others --exclude-standard)" ]; then
    echo "Uncommitted changes detected — staging release files..."
    git add \
      src/ docs/ README.md scripts/ \
      Cargo.toml Cargo.lock \
      npm/ .env.example .github 2>/dev/null || true
    # Catch any remaining tracked release paths without graphify-out / secrets
    git add -u
    if git diff --cached --quiet; then
      echo "Nothing staged after git add; check .gitignore / paths."
      git status -sb
    else
      if grep -q '^version = "0.7.1"' Cargo.toml; then
        (cargo update -p artui 2>/dev/null || true)
        if [ -d npm/src ]; then
          (cd npm/src && npm version --no-git-tag-version 0.7.1) 2>/dev/null || true
          if [ -f npm/src/package-lock.json ] && [ -d npm ]; then
            cp -f npm/src/package-lock.json npm/package-lock.json 2>/dev/null || true
          fi
        fi
        git add Cargo.lock npm/ 2>/dev/null || true
      fi
      git commit -m "chore(release): 0.7.1" -m "TUI startup/rendering fixes, artui hosted provider docs, relay URL out of public tree."
      echo "Created commit: $(git rev-parse HEAD)"
    fi
  else
    echo "Working tree clean — skipping release commit."
  fi
  echo "HEAD: $(git rev-parse HEAD)"
  echo
}

history_has_sensitive_strings() {
  git log --all -S 'kaminarikokyu' --oneline -1 2>/dev/null | grep -q .
}

phase2() {
  echo "=== PHASE 2: History redaction ==="
  if history_has_sensitive_strings; then
    echo "Sensitive hostname found in git history — running redact..."
    if ! command -v git-filter-repo >/dev/null 2>&1; then
      pip install git-filter-repo 2>/dev/null || sudo dnf install -y git-filter-repo
    fi
    ORIGIN="$(git remote get-url origin 2>/dev/null || true)"
    ARTUI_REDACT_CONFIRM=yes ./scripts/redact-freemodel-from-git-history.sh
    if [ -n "${ORIGIN:-}" ] && ! git remote get-url origin >/dev/null 2>&1; then
      git remote add origin "$ORIGIN"
      echo "Re-added origin: $ORIGIN"
    fi
  else
    echo "History check: no kaminarikokyu in commits — skip filter-repo."
  fi
  echo
  echo "--- Verification (history) ---"
  if history_has_sensitive_strings; then
    echo "WARN: kaminarikokyu still in history" >&2
    git log --all -S 'kaminarikokyu' --oneline | head -5
  else
    echo "kaminarikokyu in history: OK"
  fi
  echo "--- freemodel.dev in tree (first 15) ---"
  git grep freemodel.dev 2>/dev/null | head -15 || echo "freemodel.dev in tree: none"
  echo
}

phase3() {
  echo "=== PHASE 3: Tag v0.7.1 ==="
  if ! grep -q '^version = "0.7.1"' Cargo.toml; then
    echo "error: Cargo.toml version is not 0.7.1" >&2
    exit 1
  fi
  git tag -f v0.7.1
  echo "Tagged v0.7.1 -> $(git rev-parse v0.7.1)"
  echo
}

phase4() {
  echo "=== PHASE 4: Push ==="
  git push --force-with-lease origin main
  git push --force-with-lease origin v0.7.1
  echo
}

phase5() {
  echo "=== PHASE 5: GitHub release ==="
  if ! command -v gh >/dev/null 2>&1; then
    echo "gh not installed — skip GitHub release."
    return 0
  fi
  if ! gh auth status >/dev/null 2>&1; then
    echo "gh not authenticated — skip GitHub release."
    return 0
  fi
  NOTES="$(mktemp)"
  sed -n '/## \[0.7.1\]/,/## \[0.7.0\]/p' docs/changelogs/CHANGELOG.md | head -n -1 >"$NOTES"
  if gh release view v0.7.1 >/dev/null 2>&1; then
    gh release edit v0.7.1 --title v0.7.1 --notes-file "$NOTES"
    gh release edit v0.7.1 --draft=false
    echo "Updated and published existing release v0.7.1"
  else
    gh release create v0.7.1 --title v0.7.1 --notes-file "$NOTES"
    gh release edit v0.7.1 --draft=false 2>/dev/null || true
    echo "Created and published release v0.7.1"
  fi
  rm -f "$NOTES"
  gh release view v0.7.1 --json url,isDraft,publishedAt 2>/dev/null || true
  echo
}

phase1
phase2
phase3
phase4
phase5

echo "=== DONE ==="
echo "HEAD=$(git rev-parse HEAD)"
echo "tag v0.7.1=$(git rev-parse v0.7.1)"
echo "origin/main=$(git rev-parse origin/main 2>/dev/null || echo n/a)"
echo "Full log: $LOG"