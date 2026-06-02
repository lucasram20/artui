# M4 — macOS Seatbelt Sandbox (Design)

**Issue:** [#39](https://github.com/lucasram20/artui/issues/39)  
**Depends:** Phase F (shell), Phase J (bwrap module — wiring completed in M4)

## Problem

Phase J added `src/sandbox/mod.rs` with bubblewrap argument building, but the shell tool never invoked it and `[sandbox]` was not in config. macOS has no isolation today.

## Approach

1. **Platform backends**
   - Linux: `bwrap` (existing logic, moved to `bwrap.rs`)
   - macOS: `sandbox-exec` with generated Seatbelt profile (`seatbelt.rs` + `seatbelt_base.sb`)

2. **Config** — `[sandbox]` on `AppConfig`:
   - `mode`: `off` | `auto` | `bubblewrap` | `seatbelt` (`auto` → bwrap on Linux, seatbelt on macOS)
   - `network`: default `false`
   - `allow_home_read`: default `false` (when true, read-only `$HOME` for toolchain caches)

3. **Wiring** — `SandboxSettings` on `ToolContext`; shell spawns wrapped argv when active.

4. **Fallback** — If mode requests sandbox but binary missing, log once at startup and run unsandboxed (parity with bwrap spec).

## Out of scope (M4)

- Windows Job Object sandbox (M5)
- Escalate-on-failure re-approval without sandbox (permissions layer)
- Full Codex-style proxy/network policy

## Acceptance

Matches issue #39: auto on macOS wraps shell; system read + workspace write; network gated; `#[cfg(target_os = "macos")]` tests.