// Cloudflare Worker — artui freemodel relay.
//
// The artui binary points its `FreemodelConfig.base_url` at this Worker
// instead of `api.freemodel.dev` directly. The Worker:
//
//   1. Validates the request shape (method + path).
//   2. Optionally enforces a soft User-Agent gate (set
//      `USER_AGENT_REQUIRED_SUBSTRING` in wrangler.toml [vars]).
//   3. Per-IP rate-limits via KV (`RATE_LIMIT` binding).
//   4. Forwards to `UPSTREAM_BASE_URL` with the `FREEMODEL_API_KEY` secret
//      injected as `Authorization: Bearer <key>`.
//   5. Streams the upstream response body back unchanged so SSE chat
//      streaming continues to work.
//
// The freemodel API key never leaves Cloudflare. End-user binaries don't
// ship the key.

interface Env {
  // Secret. Set with `wrangler secret put FREEMODEL_API_KEY`.
  FREEMODEL_API_KEY: string;
  // KV namespace for per-IP rate-limit counters.
  RATE_LIMIT: KVNamespace;
  // Vars (see wrangler.toml).
  UPSTREAM_BASE_URL: string;
  RATE_LIMIT_REQUESTS: string;
  RATE_LIMIT_WINDOW_SECONDS: string;
  USER_AGENT_REQUIRED_SUBSTRING: string;
}

// Allowlist: only the routes the binary actually uses.
const ALLOWED_ROUTES: ReadonlyArray<{ method: string; path: string }> = [
  { method: "POST", path: "/v1/chat/completions" },
  { method: "GET", path: "/v1/models" },
];

// Headers we strip from the inbound request before forwarding upstream.
// The hop-by-hop set per RFC 7230 plus our own auth header (we replace it).
const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "host",
  // We always overwrite Authorization with the secret. Drop whatever the
  // client sent so we don't leak its value upstream by accident.
  "authorization",
  // CF adds these on its own; the upstream may reject them.
  "cf-connecting-ip",
  "cf-ipcountry",
  "cf-ray",
  "cf-visitor",
  "x-forwarded-for",
  "x-forwarded-proto",
  "x-real-ip",
]);

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    try {
      return await handle(request, env, ctx);
    } catch (error) {
      return jsonError(500, "relay error", String(error));
    }
  },
} satisfies ExportedHandler<Env>;

async function handle(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    if (!env.FREEMODEL_API_KEY) {
      return jsonError(503, "relay misconfigured", "FREEMODEL_API_KEY secret not set");
    }

    const url = new URL(request.url);

    // CORS preflight: keep the door open for browser-based callers (the
    // artui TUI doesn't need this, but it's harmless and useful for tests).
    if (request.method === "OPTIONS") {
      return new Response(null, {
        status: 204,
        headers: corsHeaders(request),
      });
    }

    // 1. Method + path allowlist.
    const allowed = ALLOWED_ROUTES.find(
      (route) => route.method === request.method && route.path === url.pathname,
    );
    if (!allowed) {
      return jsonError(404, "not found", `unsupported route: ${request.method} ${url.pathname}`);
    }

    // 2. User-Agent gate (soft).
    const requiredUa = (env.USER_AGENT_REQUIRED_SUBSTRING ?? "").trim();
    if (requiredUa) {
      const ua = request.headers.get("user-agent") ?? "";
      if (!ua.toLowerCase().includes(requiredUa.toLowerCase())) {
        return jsonError(403, "forbidden", "client identifier missing");
      }
    }

    // 3. Rate limit per IP.
    const ip = clientIp(request);
    const rateLimitResult = await checkRateLimit(env, ctx, ip);
    if (!rateLimitResult.allowed) {
      return jsonError(429, "rate limited", `try again in ${rateLimitResult.retryAfter}s`, {
        "Retry-After": String(rateLimitResult.retryAfter),
      });
    }

    // 4. Build the upstream request.
    const upstreamUrl = new URL(url.pathname + url.search, env.UPSTREAM_BASE_URL).toString();

    const upstreamHeaders = new Headers();
    for (const [key, value] of request.headers) {
      if (!HOP_BY_HOP.has(key.toLowerCase())) {
        upstreamHeaders.set(key, value);
      }
    }
    upstreamHeaders.set("Authorization", `Bearer ${env.FREEMODEL_API_KEY}`);
    // Identify the relay in upstream logs for support traceability.
    upstreamHeaders.set("X-Relay", "artui-freemodel-relay");

    // 5. Forward and stream the response back. `fetch()` returns a
    // streaming Response — assigning `response.body` straight to the new
    // Response keeps SSE chunks flowing without buffering the whole thing.
    const upstreamResponse = await fetch(upstreamUrl, {
      method: request.method,
      headers: upstreamHeaders,
      body: request.body,
    });

    const responseHeaders = new Headers(upstreamResponse.headers);
    // Drop hop-by-hop headers from the response too. Workers strips most
    // automatically; this is belt-and-braces.
    for (const name of HOP_BY_HOP) {
      responseHeaders.delete(name);
    }
    // Add CORS headers so a browser-hosted artui (future) can talk to us.
    for (const [k, v] of Object.entries(corsHeaders(request))) {
      responseHeaders.set(k, v);
    }

    return new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      statusText: upstreamResponse.statusText,
      headers: responseHeaders,
    });
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function clientIp(request: Request): string {
  return (
    request.headers.get("cf-connecting-ip") ??
    request.headers.get("x-real-ip") ??
    request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() ??
    "unknown"
  );
}

interface RateLimitResult {
  allowed: boolean;
  retryAfter: number;
}

/**
 * Fixed-window rate limit keyed by IP. Cheap on KV (one read + one write
 * per request worst case). The window resets every
 * `RATE_LIMIT_WINDOW_SECONDS`. KV's eventual consistency means a burst
 * client in two regions could briefly exceed the quota — acceptable for
 * an abuse-mitigation layer in front of a free upstream.
 */
async function checkRateLimit(
  env: Env,
  ctx: ExecutionContext,
  ip: string,
): Promise<RateLimitResult> {
  const limit = parsePositiveInt(env.RATE_LIMIT_REQUESTS, 60);
  const windowSeconds = parsePositiveInt(env.RATE_LIMIT_WINDOW_SECONDS, 60);
  const now = Math.floor(Date.now() / 1000);
  const bucket = Math.floor(now / windowSeconds);
  const key = `rl:${ip}:${bucket}`;

  const raw = await env.RATE_LIMIT.get(key);
  const count = raw ? Number.parseInt(raw, 10) || 0 : 0;
  if (count >= limit) {
    const resetAt = (bucket + 1) * windowSeconds;
    return { allowed: false, retryAfter: Math.max(1, resetAt - now) };
  }

  // Best-effort increment. expirationTtl drops the key shortly after the
  // window ends so storage stays bounded. We use ctx.waitUntil so the
  // write doesn't block the response, which keeps p50 latency low.
  ctx.waitUntil(
    env.RATE_LIMIT.put(key, String(count + 1), {
      expirationTtl: Math.max(60, windowSeconds * 2),
    }),
  );
  return { allowed: true, retryAfter: 0 };
}

function parsePositiveInt(value: string | undefined, fallback: number): number {
  if (!value) return fallback;
  const n = Number.parseInt(value, 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

function corsHeaders(request: Request): Record<string, string> {
  return {
    "Access-Control-Allow-Origin": request.headers.get("origin") ?? "*",
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, Authorization",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
}

function jsonError(
  status: number,
  error: string,
  detail: string,
  extra: Record<string, string> = {},
): Response {
  return new Response(JSON.stringify({ error, detail }), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      ...extra,
    },
  });
}
