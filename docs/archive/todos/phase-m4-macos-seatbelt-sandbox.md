# Phase M4 — macOS Seatbelt Sandbox

**Phase:** M4 (production polish, sandbox completeness)
**Spec:** `docs/spec/artui_v1_agentic_spec.md` §10 (macOS deferred)
**Depends:** F (shell tool), J (bwrap pattern)
**Estimated PR size:** ~500 LoC

---

## Why

Linux already has `bwrap` (Phase J). macOS users currently run
`apply_patch` and `shell` with the full filesystem accessible. Codex
and Claude Code use `sandbox-exec` (Seatbelt) on macOS for the same
isolation. Without it, a misbehaving model can `rm -rf ~/Documents`.

## Scope

### In scope

- `src/sandbox/seatbelt.rs` — `wrap_command(cmd, cwd, network,
  workspace) -> Vec<String>` returning a `sandbox-exec -p <profile>`
  invocation.
- `.sb` profile generator with these rules:
  - `(deny default)`
  - `(allow process-fork)` `(allow process-exec)`
  - `(allow file-read*)` for `/usr`, `/System`, `/Library`, `/private/etc`
  - `(allow file-read* file-write*)` rooted at `<workspace>` and `/tmp`
  - `(allow network*)` only if `network = true`
  - `(allow mach-lookup …)` for required system services
- Detection: `which sandbox-exec` at startup; fall back to unsandboxed
  with a warning when missing (parity with bwrap fallback).
- Same `[sandbox] mode = "off" | "auto" | "seatbelt" | "bubblewrap"`
  config knob; `auto` picks per-OS.
- Escalate-on-failure flow shared with bwrap.

### Out of scope

- macOS Endpoint Security Framework integration.
- Per-process resource limits (CPU, memory) — unrelated.
- App Sandbox entitlements (only relevant for App Store builds).

## Acceptance criteria

- On macOS 13+, `[sandbox] mode = "auto"` wraps shell commands.
- `cat /etc/passwd` works (read-only system); `echo x > /etc/foo`
  fails; `curl …` fails when `network = false`.
- Workspace writes still succeed.
- `cargo test` includes a `#[cfg(target_os = "macos")]` integration
  test exercising the profile.

## Files touched

| File | Change |
|---|---|
| `src/sandbox/mod.rs` | OS dispatch + `auto` resolution |
| `src/sandbox/seatbelt.rs` (new) | Seatbelt impl + profile builder |
| `src/sandbox/profile.sb.tmpl` (new) | Profile template |
| `src/tools/shell.rs` | Already calls `sandbox.transform()` — no change |
| `src/config/schema.rs` | `mode = "seatbelt"` accepted |
| Tests | macOS-gated integration |

## Risks

- **macOS deprecation**: Apple keeps marking `sandbox-exec` as
  "deprecated" but never removed it. If they pull it, fall back to
  the bwrap-on-macOS via brew, or no sandbox.
- **Profile escapes**: tight profiles break things like `cargo build`
  (needs `~/.cargo`). Add per-toolchain bind allowances mirroring
  bwrap's config.
- **mach-lookup permissions**: subtle; the profile needs DNS,
  keychain prompts, etc. Iterate from a permissive base.

## References

- codex `sandboxing/src/seatbelt.rs`
- Apple's `sandbox-exec` man page
- artui spec §10 (deferred macOS path)
