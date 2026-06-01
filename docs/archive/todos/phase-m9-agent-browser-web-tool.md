# Phase M9 — Web Browsing Tool (Vercel agent-browser)

**Phase:** M9 (production polish, capability expansion)
**Spec:** new — closes the web-browsing gap vs Claude Code / Codex
**Depends:** F (shell tool pattern), B (tool registry)
**Estimated PR size:** ~600 LoC

---

## Why

artui can read local files and the file system but cannot fetch live
web content. Claude Code, Codex, and OpenCode all bundle a web tool;
ours is missing.

[Vercel's `agent-browser`](https://github.com/vercel-labs/agent-browser)
is the right partner: native Rust CLI, real headless Chrome, **93 %
context savings vs Playwright MCP** (returns accessibility-tree
snapshots, not raw HTML/screenshots). Same external-binary pattern
artui already uses for `ripgrep` (`search` tool) and `gh` (auth).

## Scope

### In scope

- `src/tools/web.rs` — three tool variants:
  - `web_open` — `agent-browser open <url>` → returns short OK + URL
  - `web_snapshot` — `agent-browser snapshot` → returns accessibility
    tree (the cheap, model-friendly format)
  - `web_get_text <ref>` — fetch text under an `@eN` ref returned by
    snapshot
- Detection: `which agent-browser` at registry init; tool spec only
  registered when present. Otherwise hidden so the model never tries
  it.
- Output cap reuses `[agent].max_tool_output_chars`. Cap on snapshot
  content too (truncate with hint).
- Permission classifier: `web_*` defaults to `Ask` in Build, `Deny`
  in Plan. Users opt to `allow` via `[permissions.tools]`.
- Process lifecycle: artui never starts the daemon explicitly. The
  agent-browser CLI manages its own daemon; first command spawns it,
  subsequent commands reuse it. artui just shells out.

### Out of scope

- Bundling agent-browser binary inside artui's release archive.
  Friends install it themselves via `npm install -g agent-browser`
  or `brew install agent-browser`. Document in README.
- Workers Browser Rendering / browserless.io alternatives. Pluggable
  via `[providers.browser]` later if needed.
- Visual screenshots passed back as multimodal images. Defer until we
  measure that the snapshot-only path is enough.

## Acceptance criteria

- "Search docs.rust-lang.org for tokio::sync::Mutex" → model issues
  `web_open` then `web_snapshot`, gets the accessibility tree, finds
  the relevant section, returns summary.
- When `agent-browser` is missing, the tool registry omits the
  `web_*` specs entirely. Model never sees them, never tries them.
- Plan-mode agent denies `web_*` outright (consistent with shell).
- Output cap respected on huge sites.
- `cargo test` covers the spec-omission path and the args
  serialization for each variant.

## Files touched

| File | Change |
|---|---|
| `src/tools/web.rs` (new) | The three tool impls + which-detection |
| `src/tools/registry.rs` | Conditionally register `WebTool` variants |
| `src/permissions/mod.rs` | Add `web_*` to WRITE_TOOLS classification |
| `src/agent/loop.rs` | `render_approval_body` arm for `web_*` (URL preview) |
| `Cargo.toml` | No new deps — shells out via existing `tokio::process` |
| README + docs/installation hint | One-liner to install agent-browser |
| Tests | which-detection mock, args parse |

## Risks

- **External binary drift**: agent-browser is an active project; CLI
  flags may change. Pin a tested version in docs and warn when the
  detected version is older.
- **Stale daemon**: agent-browser keeps a long-lived daemon. Long
  sessions may end up with multiple daemons across machines. Document
  `agent-browser close` for cleanup.
- **Snapshot size still grows**: 93 % savings vs Playwright MCP, not
  vs an agent that decides to scroll forever. Cap at 30 k chars and
  hint to scroll/click instead.
- **Auth-walled sites**: Cookies live in agent-browser's profile.
  Login flow is explicit — `agent-browser open <login url>`, user
  signs in once, subsequent runs reuse the cookie.

## References

- [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser)
  README — full command list and `--ref` accessibility tree format
- [Apiyi guide](https://help.apiyi.com/en/agent-browser-ai-browser-automation-cli-guide-en.html)
  — context-savings benchmark vs Playwright MCP
- skills.sh entry: `npx skills add vercel-labs/agent-browser`
