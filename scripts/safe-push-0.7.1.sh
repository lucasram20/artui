#!/usr/bin/env bash
# One clean 0.7.1 commit, rebase onto origin/main (no merge commits), then push.
set -euo pipefail
cd "$(dirname "$0")/.."

LOG="${ARTUI_PUSH_LOG:-/tmp/artui-safe-push.log}"
exec > >(tee "$LOG") 2>&1

echo "=== safe-push 0.7.1 === $(date -Is)"

if ! grep -q '^version = "0.7.1"' Cargo.toml; then
  echo "error: Cargo.toml must be version 0.7.1" >&2
  exit 1
fi

git checkout main 2>/dev/null || true
git fetch origin

echo "--- before ---"
git status -sb
echo "HEAD: $(git rev-parse --short HEAD)"
echo "origin/main: $(git rev-parse --short origin/main)"

stage_release() {
  git add \
    src/ docs/ README.md scripts/ \
    Cargo.toml Cargo.lock \
    npm/ .env.example \
    .github/workflows/release.yml 2>/dev/null || true
  git add -u
  if git ls-files --error-unmatch graphify-out >/dev/null 2>&1; then
    git reset HEAD graphify-out 2>/dev/null || true
  fi
}

if ! git diff --quiet || ! git diff --cached --quiet || [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "Staging release files..."
  stage_release
  if git diff --cached --quiet; then
    echo "Nothing staged (check paths / .gitignore)."
    git status -sb
    exit 1
  fi
  git commit -m "chore(release): 0.7.1" -m "TUI startup and rendering fixes; artui hosted provider docs; relay URL via env/build only.

- CR-TERMINAL-WOLF: dirty frames, transcript cache, terminal presets
- Defer index rebuild; immediate first paint
- Provider UI label artui; public docs for maintainer-hosted API
- See docs/changelogs/CHANGELOG.md § [0.7.1]"
  echo "Commit: $(git rev-parse HEAD)"
else
  echo "Working tree clean."
fi

REMOTE="$(git rev-parse origin/main)"
LOCAL="$(git rev-parse HEAD)"
HISTORY_REWRITTEN=0
if [ -f .git/filter-repo/already_ran ] || [ "${GIT_HISTORY_REWRITTEN:-}" = "1" ]; then
  HISTORY_REWRITTEN=1
fi

if [ "$LOCAL" = "$REMOTE" ]; then
  echo "Already matches origin/main."
elif [ "$HISTORY_REWRITTEN" = "1" ]; then
  echo "History was rewritten locally; not rebasing onto old origin/main."
elif git merge-base --is-ancestor "$REMOTE" "$LOCAL" 2>/dev/null; then
  echo "Fast-forward push (local ahead of origin)."
elif git merge-base --is-ancestor "$LOCAL" "$REMOTE" 2>/dev/null; then
  echo "Rebasing onto origin/main (local was behind)..."
  git rebase origin/main
else
  echo "Histories diverged — rebasing onto origin/main (no merge commit)..."
  git rebase origin/main
fi

echo "--- after rebase ---"
git log --oneline -3
git status -sb

# History rewrite (filter-repo) requires force-with-lease; normal release does not.
if [ "$HISTORY_REWRITTEN" = "1" ]; then
  echo "Pushing with --force-with-lease (history was rewritten)."
  git push --force-with-lease origin main
else
  echo "Pushing main (no force)."
  git push origin main
fi

git tag -f v0.7.1
git push --force-with-lease origin v0.7.1 2>/dev/null || git push origin v0.7.1

echo "=== done ==="
echo "Log: $LOG"
git status -sb
