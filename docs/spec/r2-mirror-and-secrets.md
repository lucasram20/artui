# R2 mirror & GitHub secrets

Operational spec for the public **Cloudflare R2** install mirror (`artui-releases`) and the three GitHub Actions secrets that upload to it.

Related: [`docs/distribution.md`](../distribution.md) (overview), [`scripts/sync-r2-mirror.sh`](../../scripts/sync-r2-mirror.sh), workflow [`.github/workflows/r2-sync.yml`](../../.github/workflows/r2-sync.yml).

## Architecture

```
GitHub Release (vX.Y.Z)          Cloudflare R2 (public CDN)
  artui-X.Y.Z-linux-x86_64.tar.gz   vX.Y.Z/…
  artui-X.Y.Z-windows-x86_64.zip    latest/…   ← overwritten each release
  checksums.sha256                  install.sh, install.ps1
         │                                    ▲
         └──── release.yml / r2-sync.yml ─────┘
```

| Path on R2 | Purpose |
|------------|---------|
| `v0.7.0/artui-0.7.0-linux-x86_64.tar.gz` | Immutable release artifacts |
| `latest/*` | Same files as current release (fast `curl \| sh` downloads) |
| `latest/VERSION` | Plain text, e.g. `0.7.0` |
| `install.sh`, `install.ps1` | Copied from `scripts/` on each sync |

**Public base URL (default):** `https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev`

Override for installs: `ARTUI_MIRROR_BASE=https://…`

## GitHub secrets (required)

| Secret | Length | Source |
|--------|--------|--------|
| `R2_ACCOUNT_ID` | 32 chars | Cloudflare dashboard URL: `https://dash.cloudflare.com/<ACCOUNT_ID>/r2/overview` |
| `R2_ACCESS_KEY_ID` | 32 chars | R2 → **Manage R2 API Tokens** → Create → S3 **Access Key ID** |
| `R2_SECRET_ACCESS_KEY` | 64 chars | Same dialog → S3 **Secret Access Key** (shown once) |

**Not valid for upload:** global Cloudflare API tokens, Workers tokens, or email/password.

Repo settings: https://github.com/lucasram20/artui/settings/secrets/actions

## One-time R2 bucket setup

1. **Create bucket** `artui-releases` at [Cloudflare R2](https://dash.cloudflare.com/?to=/:account/r2).
2. **Public access** — bucket Settings → enable public `r2.dev` URL; note the `https://pub-….r2.dev` host.
3. **Create S3-compatible token** (see rotation below) and add the three GitHub secrets.
4. **Sync first release** — Actions → **Sync R2 mirror** → `tag: v0.7.0` (or your current tag).

## Rotate R2 GitHub secrets (step-by-step)

Use this when uploads fail with **`SignatureDoesNotMatch`**, when `latest/` still serves an old version, or after revoking a leaked token.

### 1. Cloudflare — new R2 API token

1. Open [Cloudflare Dashboard](https://dash.cloudflare.com/) → account that owns `artui-releases`.
2. **R2 object storage** → confirm bucket **`artui-releases`** exists.
3. **Manage R2 API Tokens** (on the R2 overview — not global “API Tokens” for Workers).
4. **Create API Token**:
   - Permission: **Object Read & Write**
   - Scope: **this bucket only** → `artui-releases`
5. Copy immediately (secret shown once):
   - **Access Key ID** — exactly **32** characters
   - **Secret Access Key** — exactly **64** characters
6. **Account ID** — 32-char hex from `https://dash.cloudflare.com/<ACCOUNT_ID>/r2/…` (not your login email).

Store all three in a password manager until pasted into GitHub.

### 2. GitHub — update repository secrets

1. Open https://github.com/lucasram20/artui/settings/secrets/actions
2. For each name, **Update** or **Remove** + **New repository secret**:

| Name | Value |
|------|--------|
| `R2_ACCOUNT_ID` | Account id (32 chars) |
| `R2_ACCESS_KEY_ID` | Access key (32 chars) |
| `R2_SECRET_ACCESS_KEY` | Secret key (64 chars) |

**Paste rules**

- No leading/trailing spaces or newlines.
- No quotes around values.
- Re-check lengths if anything fails.

### 3. Verify — run **Sync R2 mirror**

1. https://github.com/lucasram20/artui/actions/workflows/r2-sync.yml
2. **Run workflow** → branch `main` → **tag:** `v0.7.0` (or target release) → **Run**
3. Job must finish green.

| Log error | Action |
|-----------|--------|
| `SignatureDoesNotMatch` | Wrong account id or key pair; recreate token and re-paste secrets |
| `R2_ACCESS_KEY_ID is N chars; expected 32` | Used wrong token type or corrupted paste |
| `R2_SECRET_ACCESS_KEY is N chars; expected 64` | Same |

### 4. Confirm public mirror

```bash
curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/latest/VERSION
# → 0.7.0

curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/latest/checksums.sha256
# → lines containing artui-0.7.0-...
```

### 5. Revoke old token (after success)

Cloudflare → **Manage R2 API Tokens** → delete the previous token so only the new pair is valid.

### 6. Optional local preflight

Requires [AWS CLI v2](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html):

```bash
export R2_ACCOUNT_ID='<32-char>'
export R2_ACCESS_KEY_ID='<32-char>'
export R2_SECRET_ACCESS_KEY='<64-char>'

aws s3 ls s3://artui-releases/ \
  --endpoint-url "https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"
```

List without auth error → credentials OK:

```bash
git clone https://github.com/lucasram20/artui.git
cd artui
./scripts/sync-r2-mirror.sh v0.7.0
```

## Install script behavior (current)

| Step | Source |
|------|--------|
| Resolve `latest` tag | **GitHub** `releases/latest` first; R2 `latest/checksums.sha256` only if GitHub is down |
| Download binary | R2 `vX.Y.Z/…` if present, else GitHub Release asset |

So a stale R2 `latest/` does **not** pin installs to an old version anymore; only the CDN copy of old files remains until sync succeeds.

Pin explicitly:

```bash
curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh \
  | sh -s -- --version v0.7.0 --yes
```

Or use raw script from GitHub:

```bash
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh \
  | sh -s -- --version v0.7.0 --yes
```

After install: `artui --version` → `artui 0.7.0`. If not, see **Stale local binary** below.

## Workflows

| Workflow | When |
|----------|------|
| [Release](https://github.com/lucasram20/artui/actions/workflows/release.yml) | New tag published — builds binaries, uploads R2 + GitHub; **fails** if R2 upload fails when secrets are set |
| [Sync R2 mirror](https://github.com/lucasram20/artui/actions/workflows/r2-sync.yml) | Re-publish existing GitHub release assets to R2 without rebuilding |

Manual sync from maintainer machine: `./scripts/sync-r2-mirror.sh v0.7.0` with the same three env vars.

## Troubleshooting

### `SignatureDoesNotMatch` on `aws s3 cp`

- Token is not an **R2 S3-compatible** pair (32 + 64 chars).
- `R2_ACCOUNT_ID` does not match the account that owns the bucket.
- Secret rotated in Cloudflare but GitHub still has the old secret.
- Trailing newline/space in a GitHub secret.

**Fix:** Full rotation (sections 1–5 above).

### R2 `latest/` shows 0.6.1 but GitHub has 0.7.0

- v0.7.0 release R2 step failed; GitHub assets still published.
- **Fix:** Rotate secrets if needed, run **Sync R2 mirror** for `v0.7.0`.

### Installed `artui` is 0.3.6 / 0.6.1 on disk

Not R2 cache — an **old binary** on `PATH`:

```bash
type -a artui
command -v artui
artui --version
```

Typical paths: `~/.local/bin/artui`, `~/.cargo/bin/artui`. Reinstall with install script or `cargo install --git … --tag v0.7.0 --force`, then `hash -r`.

### Release job green but mirror old

R2 steps used `continue-on-error` historically; current `release.yml` verifies mirror and fails the job if upload steps fail. Re-run **Sync R2 mirror** after fixing secrets.

## Security notes

- Scope token to bucket `artui-releases` only.
- Rotate after any suspected leak; revoke old token in Cloudflare.
- Never commit keys; only GitHub Actions secrets and local env for `sync-r2-mirror.sh`.

## References

- [Cloudflare R2 — AWS CLI](https://developers.cloudflare.com/r2/examples/aws/aws-cli/)
- [GitHub encrypted secrets](https://docs.github.com/en/actions/security-for-github-actions/security-guides/using-secrets-in-github-actions)