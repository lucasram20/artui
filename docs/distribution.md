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
4. **Mint an API token** at R2 → Manage API tokens → Create API Token. Permission: **Object Read & Write**. Scope: this bucket only. Save the access key id + secret.
5. **Add four secrets to the GitHub repo** (`Settings → Secrets and variables → Actions`):
   - `R2_ACCOUNT_ID` — the 32-char hash in your Cloudflare dashboard URL
   - `R2_ACCESS_KEY_ID`
   - `R2_SECRET_ACCESS_KEY`
   - *(optional)* `R2_PUBLIC_BASE` — only if you set a custom domain; otherwise the install scripts default to the `pub-…r2.dev` form

That's it. Next tag push runs the `Upload assets to Cloudflare R2` step in `.github/workflows/release.yml`.

## What gets uploaded

Each tagged release uploads **two copies** of every artifact:

```
s3://artui-releases/
  ├─ v0.4.0/
  │   ├─ artui-0.4.0-x86_64-unknown-linux-gnu.tar.gz
  │   ├─ artui-0.4.0-aarch64-apple-darwin.tar.gz
  │   ├─ artui-0.4.0-x86_64-pc-windows-msvc.zip
  │   └─ checksums.sha256
  └─ latest/                     ← overwritten on every release
      ├─ artui-0.4.0-x86_64-unknown-linux-gnu.tar.gz
      └─ checksums.sha256
```

The `latest/` prefix is what `install.sh` and `install.ps1` hit when the user requests the default version. The versioned prefix is immutable so `--version v0.4.0` keeps working forever.

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
