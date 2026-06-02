#!/usr/bin/env bash
# Legacy helper — delegates to complete-release-0.7.1.sh
set -euo pipefail
cd "$(dirname "$0")/.."
exec ./scripts/safe-push-0.7.1.sh