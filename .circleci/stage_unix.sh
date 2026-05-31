#!/usr/bin/env bash
# Stage a Linux/macOS release archive after `cargo build --release`.
#
# Usage: stage_unix.sh <rust-target-triple> <asset-label>
#   rust-target-triple — e.g. x86_64-unknown-linux-gnu, aarch64-apple-darwin
#   asset-label        — e.g. linux-x86_64, macos-aarch64
#
# Reads the version from /tmp/release-meta/version (populated by the
# resolve_tag command) and stages a .tar.gz under dist/.

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <rust-target-triple> <asset-label>" >&2
  exit 1
fi

TARGET_TRIPLE="$1"
ASSET_LABEL="$2"

if [ ! -f /tmp/release-meta/version ]; then
  echo "ERROR: /tmp/release-meta/version missing — resolve_tag step did not run" >&2
  exit 1
fi

VERSION="$(cat /tmp/release-meta/version)"
NAME="artui-${VERSION}-${ASSET_LABEL}"

mkdir -p "dist/${NAME}"
cp "target/${TARGET_TRIPLE}/release/artui" "dist/${NAME}/artui"

# Optional license/readme — best-effort.
[ -f README.md ] && cp README.md "dist/${NAME}/" || true
[ -f LICENSE ] && cp LICENSE "dist/${NAME}/" || true
[ -f LICENSE-MIT ] && cp LICENSE-MIT "dist/${NAME}/" || true
[ -f LICENSE-APACHE ] && cp LICENSE-APACHE "dist/${NAME}/" || true

(cd dist && tar -czf "${NAME}.tar.gz" "${NAME}" && rm -rf "${NAME}")
ls -la dist/
