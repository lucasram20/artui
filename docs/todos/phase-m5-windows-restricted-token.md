# Phase M5 — Windows Sandbox (Job Object + Restricted Token)

**Phase:** M5 (production polish, sandbox completeness)
**Spec:** `docs/spec/artui_v1_agentic_spec.md` §10 (Windows deferred)
**Depends:** F (shell tool), J (bwrap), M4 (mode = auto)
**Estimated PR size:** ~700 LoC

---

## Why

Last platform without isolation. Closing this means the sandbox
config knob `[sandbox] mode = "auto"` Just Works on every OS.

## Scope

### In scope

- `src/sandbox/win_jobobject.rs` using the `windows-rs` crate:
  - Create a Job Object with `JobObjectExtendedLimitInformation`
    (`KillOnJobClose`, `BreakawayOK = false`).
  - Spawn the child with `CreateProcessAsUserW` and a
    `CreateRestrictedToken` derived from the current user's token,
    deny-only SIDs covering `BUILTIN\Administrators`.
  - Assign process to the job; AssignProcessToJobObject.
- Filesystem access via redirected USERPROFILE pointing at a writable
  scratch dir; workspace bound through symlink.
- Network isolation via Windows Firewall rule scoped to the child's
  PID (best effort).
- Detection: `IsWindows10OrGreater()` check; fall back to
  unsandboxed with a warning on older Windows.
- `[sandbox] mode = "win_job"` overrides `auto`.

### Out of scope

- AppContainer (UWP-only, complex provisioning).
- Hyper-V isolated containers (overkill).
- Per-domain network rules (defer).

## Acceptance criteria

- On Windows 10+, `cmd.exe /C echo x > C:\Windows\foo` fails inside
  the sandbox.
- Workspace writes still work.
- `cargo test` runs a `#[cfg(target_os = "windows")]` integration
  test on a CI Windows runner.

## Files touched

| File | Change |
|---|---|
| `src/sandbox/mod.rs` | OS dispatch picks `win_jobobject` on Windows |
| `src/sandbox/win_jobobject.rs` (new) | Job Object + restricted token impl |
| `Cargo.toml` | Add `windows-rs` features needed for Job Object APIs |
| `src/config/schema.rs` | `mode = "win_job"` accepted |
| Tests | Windows-gated integration |

## Risks

- **Restricted-token complexity**: easy to make it unusable (cannot
  load DLLs). Start permissive (deny only Admins SID) and tighten in
  a follow-up.
- **AssignProcessToJobObject race**: the spawned child runs briefly
  before being assigned. Use `CREATE_SUSPENDED` + `ResumeThread`.
- **Firewall rule cleanup**: must remove the per-PID rule on exit
  even if artui crashes. Wrap in panic hook.
- **windows-rs binary size**: pulls in a lot of bindings. Use the
  smallest feature set needed.

## References

- Codex Windows isolation (planned)
- Bun's `bun-sandbox` Windows path
- MSDN Job Object docs
