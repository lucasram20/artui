# artui hosted API relay (Cloudflare Worker)

A tiny stateless proxy that lets the artui binary call an OpenAI-compatible
upstream without shipping an API key to end users. The Worker holds the
maintainer’s upstream API key as a
[Workers secret](https://developers.cloudflare.com/workers/configuration/secrets/),
forwards a tight allowlist of OpenAI-format routes
(`POST /v1/chat/completions`, `GET /v1/models`), enforces per-IP rate
limits via Workers KV, and streams responses through unchanged so SSE
chat streaming keeps working.

This is the freebuff/Codebuff distribution model — server-side credentials,
client-side anonymity — adapted for artui's free-tier needs.

## What it gives you

- **Zero credentials in user binaries.** The artui binary downloaded by
  end users never contains the upstream API key. It's only in Cloudflare.
- **Free to run.** Workers free tier covers 100,000 requests/day, no CPU
  charge for time spent waiting on `fetch()`. KV free tier covers
  rate-limit counters with room to spare.
- **No code changes for new upstream models.** The Worker forwards the
  request body verbatim, so any model the upstream supports works.
- **Soft abuse mitigation.** User-Agent gate + per-IP rate limit. Easily
  bypassed by determined abusers, but keeps casual scraping out and gives
  you a Cloudflare dashboard view of unusual traffic patterns.

## Files

| File             | Role                                                              |
|------------------|-------------------------------------------------------------------|
| `worker.ts`      | The Worker entry point — handler, rate limiter, upstream forward. |
| `wrangler.toml`  | Deploy config (name, KV binding, vars).                           |
| `package.json`   | Wrangler + Workers types as devDependencies.                      |
| `tsconfig.json`  | Strict-mode TypeScript for the Worker source.                     |

The KV namespace **id** in `wrangler.toml` and the `FREEMODEL_API_KEY`
**secret** are both filled in interactively during the deploy steps below.

## Deploy

You need a Cloudflare account (any plan, the Workers free tier is fine).

```sh
cd cloudflare/
npm install                              # pulls wrangler + types

# 1. One-time: log in to Cloudflare in your browser.
npx wrangler login

# 2. Create the KV namespace used for rate limiting.
npx wrangler kv namespace create FREEMODEL_RATE_LIMIT
# → copy the printed `id = "..."` into wrangler.toml under [[kv_namespaces]]

# 3. Set the upstream API key as a Worker secret. You'll be prompted to
#    paste the key (it never appears in shell history or in the repo).
npx wrangler secret put FREEMODEL_API_KEY

# 4. Deploy.
npx wrangler deploy
```

After deploy Wrangler prints the public URL, e.g.:

```
Published artui-freemodel-relay (1.23 sec)
  https://artui-freemodel-relay.<your-subdomain>.workers.dev
```

Update artui's default base URL to that URL + `/v1`. Either:

- Edit `src/config/schema.rs::FreemodelConfig::default().base_url` and
  rebuild, or
- Tell users to set `providers.freemodel.base_url` in
  `~/.config/artui/config.toml`.

## Verify

A direct call should now work without any Authorization header:

```sh
curl -i -H "User-Agent: artui-cli/test" \
  https://artui-freemodel-relay.<your-subdomain>.workers.dev/v1/models
```

You should see a `200 OK` and a JSON body with the model list. A call
without `User-Agent: artui*` should return `403 forbidden` (the soft gate),
and any path other than the two allowed routes should return `404 not
found` from the relay (not the upstream).

## Tuning

All knobs are environment variables in `[vars]` of `wrangler.toml` — change
the file and re-deploy:

| Var                              | Default | Purpose                                                     |
|----------------------------------|---------|-------------------------------------------------------------|
| `UPSTREAM_BASE_URL`              | `https://api.freemodel.dev` | Where the relay forwards. Change if upstream URL changes.   |
| `RATE_LIMIT_REQUESTS`            | `60`    | Max requests per IP per window.                             |
| `RATE_LIMIT_WINDOW_SECONDS`      | `60`    | Window size. 60/60 = sustained 1 req/s with bursts.         |
| `USER_AGENT_REQUIRED_SUBSTRING`  | `artui` | UA must contain this. Set to `""` to disable the soft gate. |

Secrets (rotated separately):

```sh
npx wrangler secret put FREEMODEL_API_KEY     # rotate the upstream key
npx wrangler secret delete FREEMODEL_API_KEY  # revoke
```

## Observability

`[observability] enabled = true` in `wrangler.toml` opts the Worker into
[Workers Logs](https://developers.cloudflare.com/workers/observability/logs/)
on the free tier. The dashboard shows:

- Requests per minute, broken down by status code
- p50 / p95 / p99 latency
- CPU time per request (well under 1ms for a streaming proxy)
- Live error tail via `npx wrangler tail`

If you'd rather not send analytics, drop the `[observability]` block.

## Security posture

- **The relay URL is the new free key.** Anyone with the URL can `curl`
  it. The User-Agent gate and rate limiter make casual abuse expensive in
  effort, not in money. Determined abusers will burn through your daily
  100k budget before they hit your wallet.
- **No CORS lockdown.** The relay accepts any `Origin`. If you ever ship
  a browser-hosted artui this is what you want; if you're paranoid, edit
  `corsHeaders()` to allowlist a specific origin.
- **No request validation beyond shape.** The relay forwards the body
  unchanged, including the `model` parameter. If freemodel later adds
  premium model tiers you don't want artui users hitting, validate the
  `model` field in the request body before forwarding.
- **No logging of request bodies.** The Worker doesn't log prompts or
  responses. Only request metadata (status, IP, latency) lands in
  Workers Logs.

## Rotating the freemodel key

If the upstream key leaks or you just want a fresh one:

```sh
# Get a new key from freemodel.dev
npx wrangler secret put FREEMODEL_API_KEY    # paste the new key
# No redeploy needed — Workers picks up the new secret value on next
# request without a restart.
```

Existing artui binaries continue to work without any user-side action.
