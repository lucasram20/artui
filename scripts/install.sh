#!/usr/bin/env bash
# artui install script — Linux / macOS
#
# Usage:
#   curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh
#   curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh -s -- --version v0.7.0
#
# Detects arch (x86_64/aarch64), downloads the matching release binary
# from the public Cloudflare R2 mirror, falls back to GitHub releases on
# miss. Installs to ~/.local/bin/artui (override with INSTALL_DIR).
#
# Skip download and build from source: ARTUI_FROM_SOURCE=1
set -euo pipefail

REPO="${ARTUI_REPO:-lucasram20/artui}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${ARTUI_VERSION:-latest}"
ASSUME_YES="${ARTUI_INSTALL_YES:-0}"

# Public Cloudflare R2 mirror — primary download source. Lets users
# without GitHub access (or while the source repo is private) install
# without auth. Override with $ARTUI_MIRROR_BASE for self-hosted CDNs.
R2_BASE="${ARTUI_MIRROR_BASE:-https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev}"

# Optional GitHub token for private-repo access. Friends granted
# collaborator access can generate a fine-grained PAT (Contents: read,
# Metadata: read) and pass it as $GITHUB_TOKEN. Public installs leave
# this empty.
TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
if [ -n "$TOKEN" ]; then
  AUTH_HEADER="Authorization: Bearer $TOKEN"
else
  AUTH_HEADER=""
fi

# ── Tiny TTY UI helpers ─────────────────────────────────────────────────
_is_tty() { [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ -z "${CI:-}" ]; }
if _is_tty; then
  C_RESET="$(printf '\033[0m')"
  C_DIM="$(printf '\033[2m')"
  C_BOLD="$(printf '\033[1m')"
  C_CYAN="$(printf '\033[36m')"
  C_GREEN="$(printf '\033[32m')"
  C_YELLOW="$(printf '\033[33m')"
  C_RED="$(printf '\033[31m')"
else
  C_RESET=""; C_DIM=""; C_BOLD=""; C_CYAN=""; C_GREEN=""; C_YELLOW=""; C_RED=""
fi

print_logo() {
  if ! _is_tty; then
    echo "artui installer"
    return
  fi
  printf '%s' "$C_CYAN$C_BOLD"
  cat <<'LOGO'

  █████╗ ██████╗ ████████╗██╗   ██╗██╗
 ██╔══██╗██╔══██╗╚══██╔══╝██║   ██║██║
 ███████║██████╔╝   ██║   ██║   ██║██║
 ██╔══██║██╔══██╗   ██║   ██║   ██║██║
 ██║  ██║██║  ██║   ██║   ╚██████╔╝██║
 ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝
LOGO
  printf '%s' "$C_RESET"
  printf '  %sinteractive coding-agent CLI%s\n\n' "$C_DIM" "$C_RESET"
}

step() { printf '%s›%s %s\n' "$C_DIM" "$C_RESET" "$*"; }
ok()   { printf '%s✔%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s!%s %s\n' "$C_YELLOW" "$C_RESET" "$*"; }
err()  { printf '%s✖%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; }

# Run "$@" while a unicode braille spinner ticks on a single line.
# Falls back to plain logging on non-TTY contexts so curl|sh logs stay clean.
spin() {
  local label="$1"; shift
  if ! _is_tty; then
    step "$label"
    "$@"
    return $?
  fi
  local frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
  local pid status i=0
  ( "$@" ) &
  pid=$!
  printf '\033[?25l'
  while kill -0 "$pid" 2>/dev/null; do
    local c="${frames:i++ % ${#frames}:1}"
    printf '\r\033[2K%s%s%s %s' "$C_CYAN" "$c" "$C_RESET" "$label"
    sleep 0.08
  done
  wait "$pid"
  status=$?
  printf '\r\033[2K\033[?25h'
  if [ "$status" -eq 0 ]; then ok "$label"; else err "$label"; fi
  return $status
}

# ── CLI parsing ────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dir)     INSTALL_DIR="$2"; shift 2 ;;
    --repo)    REPO="$2"; shift 2 ;;
    -y|--yes)  ASSUME_YES=1; shift ;;
    -h|--help)
      cat <<EOF
artui installer

Options:
  --version <tag>   release tag to install (default: latest)
  --dir <path>      install directory (default: ~/.local/bin)
  --repo <owner/r>  GitHub repo (default: $REPO)
  -y, --yes         skip the interactive confirmation
  -h, --help        show this help

Env:
  ARTUI_FROM_SOURCE=1   build via 'cargo install --git' instead of binary download
  ARTUI_INSTALL_YES=1   skip the interactive confirmation
  NO_COLOR=1            disable colored output
  ARTUI_REPO=…          override the GitHub repo
EOF
      exit 0
      ;;
    *) err "unknown flag: $1"; exit 2 ;;
  esac
done

print_logo

# Confirmation prompt — skip when piped (no controlling tty), --yes, env, or CI.
prompt_confirm() {
  if [ "$ASSUME_YES" = "1" ] || [ -n "${CI:-}" ]; then
    return 0
  fi
  # When run via `curl | sh`, stdin is the pipe; read from /dev/tty.
  if [ ! -r /dev/tty ]; then
    warn "No interactive terminal detected; pass --yes to bypass this prompt."
    return 1
  fi
  printf '\n%sInstall artui to %s?%s [Y/n] ' "$C_BOLD" "$INSTALL_DIR" "$C_RESET"
  local reply
  IFS= read -r reply < /dev/tty || reply=""
  case "$reply" in
    ""|y|Y|yes|YES|Yes) return 0 ;;
    *) return 1 ;;
  esac
}

if ! prompt_confirm; then
  warn "Install aborted."
  exit 0
fi

UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"

case "$UNAME_S" in
  Linux)
    OS="linux"
    ;;
  Darwin)
    # macOS binaries aren't published — CircleCI free-tier macOS minutes
    # are 0, and we don't have budget for a paid plan. Mac users build
    # from source instead. Cargo is the standard Rust toolchain so
    # this is the same flow every Rust developer uses.
    err "macOS binaries aren't published. Build from source:"
    err "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    err "  cargo install --git https://github.com/lucasram20/artui"
    err "Or set ARTUI_FROM_SOURCE=1 and re-run this script after installing Rust."
    if [ "${ARTUI_FROM_SOURCE:-0}" != "1" ]; then
      exit 1
    fi
    OS="macos"
    ;;
  *) err "unsupported OS: $UNAME_S"; exit 1 ;;
esac

case "$UNAME_M" in
  x86_64|amd64)  ARCH="x86_64" ;;
  aarch64|arm64)
    # ARM64 handling: Linux ARM and macOS Apple Silicon both fall
    # through to source-build. Windows ARM runs x86_64 via emulation
    # so install.ps1 handles it differently.
    err "ARM64 binaries aren't published. Build from source:"
    err "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    err "  cargo install --git https://github.com/lucasram20/artui"
    if [ "${ARTUI_FROM_SOURCE:-0}" != "1" ]; then
      exit 1
    fi
    ARCH="aarch64"
    ;;
  *) err "unsupported arch: $UNAME_M"; exit 1 ;;
esac

TARGET="${OS}-${ARCH}"
step "Target ${C_CYAN}${TARGET}${C_RESET}"

# True when the release artifact exists on R2 or GitHub (skip tag-only releases).
asset_exists_for_tag() {
  local tag="$1"
  local asset="artui-${tag#v}-${TARGET}.tar.gz"
  local r2_url="${R2_BASE}/${tag}/${asset}"
  if curl -fIsS "$r2_url" >/dev/null 2>&1; then
    return 0
  fi
  if [ -n "$AUTH_HEADER" ]; then
    if curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/vnd.github+json" \
      "https://api.github.com/repos/${REPO}/releases/tags/${tag}" \
      | tr ',' '\n' | grep -q "\"name\": \"${asset}\""; then
      return 0
    fi
  fi
  if curl -fIsS "https://github.com/${REPO}/releases/download/${tag}/${asset}" >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

mkdir -p "$INSTALL_DIR"

if [ "${ARTUI_FROM_SOURCE:-0}" = "1" ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    err "cargo not found — install Rust from https://rustup.rs"
    exit 1
  fi
  spin "Building artui from source via cargo install --git" \
    cargo install --git "https://github.com/${REPO}.git" --root "${INSTALL_DIR%/bin}" artui
  ok "Installed to ${C_CYAN}${INSTALL_DIR}/artui${C_RESET}"
  exit 0
fi

if [ "$VERSION" = "latest" ]; then
  step "Resolving latest release"
  TAG=""
  R2_TAG=""
  GH_TAG=""

  # GitHub is authoritative for the release tag (R2 `latest/` can lag if mirror upload failed).
  if [ -n "$AUTH_HEADER" ]; then
    GH_TAG="$(curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/vnd.github+json" \
      "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name": "\(.*\)".*/\1/p' | head -1 || true)"
  else
    GH_TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name": "\(.*\)".*/\1/p' | head -1 || true)"
  fi

  if curl -fIsS "${R2_BASE}/latest/checksums.sha256" >/dev/null 2>&1; then
    R2_TAG="v$(curl -fsSL "${R2_BASE}/latest/checksums.sha256" \
      | sed -n 's/.*artui-\([0-9.][0-9.]*\)-.*/\1/p' | head -1 || true)"
  fi
  if [ "$R2_TAG" = "v" ]; then
    R2_TAG=""
  fi

  if [ -n "$GH_TAG" ] && asset_exists_for_tag "$GH_TAG"; then
    TAG="$GH_TAG"
    if [ -n "$R2_TAG" ] && [ "$R2_TAG" != "$GH_TAG" ]; then
      warn "R2 mirror latest (${R2_TAG}) differs from GitHub (${GH_TAG}); using GitHub."
    fi
  elif [ -n "$R2_TAG" ] && asset_exists_for_tag "$R2_TAG"; then
    TAG="$R2_TAG"
    if [ -n "$GH_TAG" ]; then
      warn "GitHub latest (${GH_TAG}) has no ${TARGET} binary on R2 or GitHub; using mirror tag ${R2_TAG}."
    else
      warn "GitHub latest unavailable; using R2 mirror tag ${R2_TAG}."
    fi
  elif [ -n "$GH_TAG" ]; then
    TAG="$GH_TAG"
    warn "GitHub latest (${GH_TAG}) has no published ${TARGET} asset yet; download may fail."
  fi

  if [ -z "$TAG" ]; then
    err "Could not resolve latest release for ${REPO}."
    if [ -z "$AUTH_HEADER" ]; then
      warn "Repo may be private. Set GITHUB_TOKEN to a fine-grained PAT with Contents:read and Metadata:read."
    fi
    warn "Set ARTUI_FROM_SOURCE=1 to build from source instead."
    exit 1
  fi
else
  TAG="$VERSION"
fi

ASSET="artui-${TAG#v}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
R2_URL="${R2_BASE}/${TAG}/${ASSET}"
step "Version ${C_CYAN}${TAG}${C_RESET}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Private-repo download path:
#   1. Resolve the asset id via the API (requires Bearer token).
#   2. Hit the API asset endpoint with Accept: application/octet-stream so
#      GitHub returns a signed redirect we can follow without leaking the
#      Authorization header to the redirect target.
download_private_asset() {
  local asset_id
  asset_id="$(curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/${REPO}/releases/tags/${TAG}" \
    | tr ',' '\n' \
    | awk -v name="$ASSET" '
        /"name":/ { gsub(/[",]/, ""); current_name=$2 }
        /"id":/   { gsub(/[",]/, ""); if (current_name == name) { print $2; exit } }
      ')"
  if [ -z "$asset_id" ]; then
    err "Could not locate asset '$ASSET' on release ${TAG}."
    return 1
  fi
  curl -fL -H "$AUTH_HEADER" -H "Accept: application/octet-stream" \
    "https://api.github.com/repos/${REPO}/releases/assets/${asset_id}" \
    -o "$TMP/artui.tar.gz"
}

# Try the public R2 mirror first (zero-auth, fastest, works for friends
# without GitHub access). Fall back to private GitHub asset endpoint when
# a token is set, then plain public GH releases as a last resort.
if curl -fIsS "$R2_URL" >/dev/null 2>&1; then
  if _is_tty; then
    printf '  Downloading '
    curl -fL --progress-bar "$R2_URL" -o "$TMP/artui.tar.gz"
  else
    step "Downloading $R2_URL"
    curl -fsSL "$R2_URL" -o "$TMP/artui.tar.gz"
  fi
elif [ -n "$AUTH_HEADER" ]; then
  step "R2 mirror miss; falling back to GitHub API for ${ASSET}"
  if ! download_private_asset; then
    err "Asset download failed. Verify the GITHUB_TOKEN scope (Contents:read, Metadata:read)."
    exit 1
  fi
elif _is_tty; then
  printf '  Downloading '
  curl -fL --progress-bar "$URL" -o "$TMP/artui.tar.gz"
else
  step "Downloading $URL"
  curl -fsSL "$URL" -o "$TMP/artui.tar.gz"
fi

spin "Extracting archive" tar -xzf "$TMP/artui.tar.gz" -C "$TMP"

if [ -f "$TMP/artui" ]; then
  install -m 755 "$TMP/artui" "$INSTALL_DIR/artui"
elif [ -f "$TMP/artui-${TAG#v}-${TARGET}/artui" ]; then
  install -m 755 "$TMP/artui-${TAG#v}-${TARGET}/artui" "$INSTALL_DIR/artui"
else
  err "Could not locate artui binary inside the downloaded archive."
  exit 1
fi

ok "Installed artui ${TAG} to ${C_CYAN}${INSTALL_DIR}/artui${C_RESET}"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) warn "$INSTALL_DIR is not on \$PATH. Add it to your shell rc to run 'artui'." ;;
esac
