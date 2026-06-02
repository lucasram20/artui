# Distribution & Cloudflare R2 mirror

artui ships binaries through a public **Cloudflare R2** bucket so friends, CI, and one-off users can install with `curl | sh` even while the source repo is private.

```
   ┌─ private repo (lucasram20/artui) ─┐    ┌─ public R2 mirror ─────────┐
   │ release.yml builds 6 archives    │ →  │ pub-artui-releases.r2.dev/ │
   │ + checksums on each tag push     │    │   v0.4.0/artui-…tar.gz     │
   │ Mirrors them to R2 + uploads to  │    │   latest/artui-…tar.gz     │
   │ the GitHub release as a backup.  │    │   …                        │
   └──────────────────────────────────┘    └────────────────────────────┘
                                                  ↑
                                                  │
   curl/PowerShell/npm scripts download from R2 first; fall back to
   GitHub only if the mirror is missing or empty.
```

## One-time R2 setup

1. **Create the bucket** at <https://dash.cloudflare.com/?to=/:account/r2>. Name: `artui-releases`.
2. **Enable public access** on the bucket → Settings → "Public access". Cloudflare prints a public URL like `https://pub-<hash>.r2.dev`. Copy it.
3. **Optional: custom domain** (e.g. `releases.artui.dev`) — add a CNAME to the same bucket. Skip for now if you don't have a domain.
4. **Mint an S3-compatible R2 token** (not a global Cloudflare API key):
   - R2 → **Manage R2 API Tokens** → **Create API Token**
   - Permission: **Object Read & Write**, scope: bucket `artui-releases` only
   - Copy **Access Key ID** (exactly **32** characters) and **Secret Access Key** (exactly **64** characters) — shown once
5. **Add three secrets to the GitHub repo** (`Settings → Secrets and variables → Actions`):
   - `R2_ACCOUNT_ID` — 32-char account id from the Cloudflare dashboard URL (`dash.cloudflare.com/<this-id>/r2`)
   - `R2_ACCESS_KEY_ID` — 32-char S3 access key from step 4
   - `R2_SECRET_ACCESS_KEY` — 64-char S3 secret from step 4
   - *(optional)* `R2_PUBLIC_BASE` — only if you set a custom domain; otherwise install scripts use `https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev`

If uploads fail with **`SignatureDoesNotMatch`** but lengths look right, the secret pair is wrong or stale — delete the three GitHub secrets, create a **new** R2 API token, paste fresh values (no trailing newline), re-run **Sync R2 mirror**.

That's it. Next tag push runs the `Upload assets to Cloudflare R2` step in `.github/workflows/release.yml`.

### Fix a stale `latest/` mirror

If GitHub has a release but R2 still serves an older version (e.g. `SignatureDoesNotMatch` during upload):

1. **Rotate R2 secrets** if needed — Cloudflare → R2 → Manage R2 API Tokens → create S3-compatible token (32-char access key, 64-char secret). Update repo secrets `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` (trim whitespace).
2. **Re-sync without rebuilding:** Actions → **Sync R2 mirror** → Run workflow → `tag: v0.7.0`.
3. Or locally: `./scripts/sync-r2-mirror.sh v0.7.0` with the same env vars.

The release workflow now **fails** after publish if R2 upload steps fail (GitHub assets still attach), so you get a clear signal to run **Sync R2 mirror**.

## What gets uploaded

Each tagged release uploads **two copies** of every artifact:

```
s3://artui-releases/
  ├─ v0.4.0/
  │   ├─ artui-0.4.0-linux-x86_64.tar.gz
  │   ├─ artui-0.4.0-macos-aarch64.tar.gz
  │   ├─ artui-0.4.0-windows-x86_64.zip
  │   └─ checksums.sha256
  └─ latest/                     ← overwritten on every release
      ├─ artui-0.4.0-linux-x86_64.tar.gz
      └─ checksums.sha256
```

The `latest/` prefix mirrors release assets for fast CDN downloads. **Tag resolution** uses GitHub Releases first (`releases/latest`), then R2 `latest/` only if GitHub is unreachable — so a stale R2 mirror cannot pin installs to an older version. The versioned prefix is immutable so `--version v0.7.0` (or any prior tag) keeps working forever.

## Install script flow

Every installer tries sources in this order:

1. **Cloudflare R2 mirror** (`$ARTUI_MIRROR_BASE` or `https://pub-artui-releases.r2.dev`) — public, no auth.
2. **GitHub API** with `$GITHUB_TOKEN` — only when the user is a collaborator and exported a PAT.
3. **GitHub Releases CDN** — public release assets, only useful when the source repo is public.

If R2 is configured (the only case after the steps above), step 1 wins for everyone, and steps 2-3 are dead code unless something is misconfigured.

Override the mirror to a private CDN of your own:

```bash
ARTUI_MIRROR_BASE=https://my-internal-cdn.example.com \
  curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh
```

## Cost

Cloudflare R2 free tier:

- 10 GB-month storage
- 10 M Class A operations / month (writes)
- 10 M Class B operations / month (reads)
- **Zero egress fees**

Each release adds ~80 MB across 6 archives. You can ship ~125 releases before paying anything for storage; downloads are free regardless of volume.

## Disabling the mirror

Don't want R2? Remove the four secrets — the upload step probes `R2_ACCOUNT_ID` and skips when empty (`HAS_R2=false` in the workflow). Installers also fall back to GitHub Releases when the mirror returns 404, so existing tag pushes continue to work.
