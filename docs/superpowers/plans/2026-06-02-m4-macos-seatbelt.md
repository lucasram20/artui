# M4 — macOS Seatbelt Implementation Plan

**Design:** `docs/superpowers/specs/2026-06-02-m4-macos-seatbelt-design.md`  
**Issue:** [#39](https://github.com/lucasram20/artui/issues/39)

## Tasks (completed)

1. Split `bwrap` into `src/sandbox/bwrap.rs`; add `seatbelt.rs` + `seatbelt_base.sb`.
2. Add `SandboxSettings` dispatch in `src/sandbox/mod.rs` (`auto` / per-OS).
3. Add `[sandbox]` to `AppConfig` (`mode`, `network`, `allow_home_read`).
4. Thread `SandboxSettings` through `App` → `ProviderRequest` → `AgentLoopConfig` → `ToolContext`.
5. Wrap Unix shell spawns in `src/tools/shell.rs` when sandbox active.
6. Startup warning in `lib.rs` when mode expects sandbox but backend missing.
7. `tests/sandbox_integration.rs` (OS-gated).
8. README + CHANGELOG.

## Verification

```bash
cargo fmt --all -- --check
cargo test --quiet
cargo clippy --all-targets -- -D warnings
```