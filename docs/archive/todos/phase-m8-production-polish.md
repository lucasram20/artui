# Phase M8 — Production Polish (1.0 Release)

**Phase:** M8 (the 1.0 push)
**Spec:** none — operational quality work
**Depends:** all of M1–M7 (ideally) but most items can ship sooner
**Estimated PR size:** ~800 LoC + tons of docs

---

## Why

Tier-S projects (Claude Code, Codex, OpenCode, pi) aren't winning on
features alone — they win on the polish of telemetry, crash
reporting, accessibility, and documentation. This phase closes the
remaining "feels mature" items so artui can ship 1.0.

## Scope

### In scope

- **Telemetry (opt-in)**: anonymous error counts + tool-use frequency,
  written to a Cloudflare Workers endpoint. **Off by default**;
  `[telemetry] enabled = true` opts in. No PII, no payloads, never
  paths or keystrokes — just `{event_name, timestamp, build_hash}`.
- **Crash reporter**: panic handler writes a sanitized backtrace +
  active provider/model + tool registry to `~/.local/share/artui/
  crashes/<id>.json`. Optional auto-upload behind the same
  `[telemetry] crash_uploads = false` knob.
- **Accessibility**:
  - Screen-reader friendly mode (`ARTUI_NO_ANIMATIONS=1`,
    `[ui] flatten_for_a11y = true`) — drops the eye animation,
    serializes streamed text with newlines.
  - High-contrast theme with WCAG AA-compliant pairings.
  - Keyboard-only flows for every modal (no mouse-required action).
- **Docs site**: `docs/site/` mdBook → published to Cloudflare Pages
  on every release. Mirrors README + spec + per-tool reference + API.
- **i18n scaffolding**: `src/i18n/strings.rs` with English defaults;
  `Tr::t("…")` macro. Real translations later, but lock the API now.
- **Error catalogue**: every `Result::Err` carries a stable error
  code (`artui::err::E0123`) so docs can deep-link.
- **Quickstart benchmarks**: scripted profile that runs a fixed
  workload and prints latency / token / wallclock so we don't regress.

### Out of scope

- A11y audit by a third party (separate engagement).
- Public roadmap site / governance doc (community work).
- SBOM / supply-chain attestation (defer to v1.1).

## Acceptance criteria

- `[telemetry] enabled = false` results in zero outbound network
  calls anywhere in the binary (assert via wiremock test that no
  request reaches the telemetry endpoint).
- `panic!()` writes a crash JSON that does not contain absolute paths
  outside the workspace, env vars, or auth tokens.
- Screen-reader smoke test (manual): run `artui` under VoiceOver /
  Orca; navigate `/help`, send a message, exit.
- mdBook builds; deployed to Pages on tag push.
- `cargo test` covers error code uniqueness and i18n lookup.

## Files touched

| File | Change |
|---|---|
| `src/telemetry/mod.rs` (new) | Opt-in metrics emitter |
| `src/util/crash.rs` (new) | Sanitized panic hook |
| `src/i18n/mod.rs` (new) | String table + `Tr::t!` |
| `src/ui/themes.rs` | High-contrast theme |
| `src/ui/animation.rs` | A11y flatten mode |
| `src/util/errors.rs` (new) | Error code registry |
| `docs/site/book.toml` (new) | mdBook config |
| `docs/site/src/**` (new) | Generated reference + handcrafted guides |
| `.github/workflows/docs.yml` (new) | Build + deploy mdBook |
| `bench/scenarios/*.toml` (new) | Benchmark scripts |
| Tests | Error codes unique, telemetry off-by-default, i18n |

## Risks

- **Telemetry trust**: anything users perceive as "phoning home"
  damages adoption. Default OFF, document the exact bytes sent, sign
  the endpoint, document the shutdown procedure.
- **Crash reports leaking secrets**: scrub aggressively. Test by
  setting `OPENAI_API_KEY=test_secret` and asserting it's redacted.
- **mdBook drift**: per-tool docs must be auto-generated from
  `Tool::spec()` so they don't go stale. Build a `cargo run --bin
  gen-docs` step.
- **i18n cost**: don't translate everything yet — stub the API,
  populate strings as the project grows. Deferred translation work
  is fine.

## References

- pi telemetry doc
- Anthropic's claude-code crash reporter
- Vercel docs site (mdx-style structure that mdBook can mirror)
