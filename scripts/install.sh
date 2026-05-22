#!/usr/bin/env bash
# artui install script — Linux / macOS
#
# Usage:
#   curl -fsSL https://artui.dev/install.sh | sh
#   curl -fsSL https://artui.dev/install.sh | sh -s -- --version v0.0.1
#
# Detects arch (x86_64/aarch64), downloads the matching release binary from
# GitHub, and installs it to ~/.local/bin/artui (override with INSTALL_DIR).
#
# Skip download and build from source: ARTUI_FROM_SOURCE=1
set -euo pipefail

REPO="${ARTUI_REPO:-lucasram20/artui}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${ARTUI_VERSION:-latest}"

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir)     INSTALL_DIR="$2"; shift 2 ;;
    --repo)    REPO="$2"; shift 2 ;;
    -h|--help)
      cat <<EOF
artui installer

Options:
  --version <tag>   release tag to install (default: latest)
  --dir <path>      install directory (default: ~/.local/bin)
  --repo <owner/r>  GitHub repo (default: $REPO)

Env:
  ARTUI_FROM_SOURCE=1  build via 'cargo install --git' instead of binary download
EOF
      exit 0
      ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done

UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"

case "$UNAME_S" in
  Linux)  OS="unknown-linux-gnu" ;;
  Darwin) OS="apple-darwin" ;;
  *) echo "unsupported OS: $UNAME_S" >&2; exit 1 ;;
esac

case "$UNAME_M" in
  x86_64|amd64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "unsupported arch: $UNAME_M" >&2; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"

mkdir -p "$INSTALL_DIR"

if [ "${ARTUI_FROM_SOURCE:-0}" = "1" ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found — install Rust from https://rustup.rs" >&2
    exit 1
  fi
  echo "Building artui from source via cargo install --git…"
  cargo install --git "https://github.com/${REPO}.git" --root "${INSTALL_DIR%/bin}" artui
  echo "Installed to $INSTALL_DIR/artui"
  exit 0
fi

if [ "$VERSION" = "latest" ]; then
  TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": "\(.*\)".*/\1/p' | head -1 || true)"
  if [ -z "$TAG" ]; then
    echo "Could not resolve latest release for ${REPO}." >&2
    echo "Set ARTUI_FROM_SOURCE=1 to build from source instead." >&2
    exit 1
  fi
else
  TAG="$VERSION"
fi

ASSET="artui-${TAG#v}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

echo "Downloading $URL"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "$URL" -o "$TMP/artui.tar.gz"
tar -xzf "$TMP/artui.tar.gz" -C "$TMP"

if [ -f "$TMP/artui" ]; then
  install -m 755 "$TMP/artui" "$INSTALL_DIR/artui"
elif [ -f "$TMP/artui-${TAG#v}-${TARGET}/artui" ]; then
  install -m 755 "$TMP/artui-${TAG#v}-${TARGET}/artui" "$INSTALL_DIR/artui"
else
  echo "Could not locate artui binary inside the downloaded archive." >&2
  exit 1
fi

echo "Installed artui ${TAG} to ${INSTALL_DIR}/artui"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Note: $INSTALL_DIR is not on \$PATH. Add it to your shell rc to run 'artui'." ;;
esac
