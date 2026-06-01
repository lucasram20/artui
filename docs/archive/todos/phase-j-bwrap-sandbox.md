# Phase J — Linux bubblewrap sandbox

**Status:** DONE (2026-05-21)
**Summary:** Implemented `src/sandbox/mod.rs` with `wrap_command` (builds bwrap args: ro-bind system, writable workspace, optional network isolation, die-with-parent). `SandboxConfig` with `is_active()` check. Graceful fallback when bwrap not installed. 86 tests pass.

**Phase:** J
**Spec:** `docs/spec/harness-architecture.md` §6; `artui_v1_agentic_spec.md` §10
**Depends:** F (shell tool)
**Estimated PR size:** ~400 LoC

---

## Why

Phase F shell is permission-gated but unsandboxed. A command with `Allow` decision runs as the user with full filesystem and network access. Spec §10 calls for an opt-in Linux `bwrap` sandbox: bind workspace as RW, system as RO, block network by default, die-with-parent.

## Scope

### In scope

- Detect `bwrap` via `which`. If missing, log info and run unsandboxed (fall back to current Phase F behavior).
- New `[sandbox]` config section: `mode = "off" | "bubblewrap"`, `workspace_write = true`, `network = false`, `allow_home_read = false`.
- New `src/sandbox/bwrap.rs` — wrap a `Command` so it execs `bwrap --ro-bind /usr ... -- <command>`.
- When sandbox mode is `bubblewrap`, `tools::shell` execs the bwrap-wrapped command instead.
- Sandbox flags:
  - `--ro-bind /usr /usr`, `/lib`, `/lib64`, `/bin`, `/etc/resolv.conf` (for DNS)
  - `--bind <workspace> <workspace>` (RW)
  - `--tmpfs /tmp`
  - `--chdir <workspace>`
  - `--unshare-net` if `[sandbox] network = false`
  - `--die-with-parent`
  - `--proc /proc`
  - `--dev /dev`
- Sandbox failures: emit `SandboxErr::Denied` → escalate path: ask user to re-approve without sandbox, retry once.

### Out of scope

- macOS Seatbelt (deferred).
- Windows restricted-token (deferred).
- Network proxy with per-host allowlists (deferred to its own phase).
- Landlock/seccomp (deferred; bwrap is sufficient for v1).

## Acceptance criteria

- On Fedora/Ubuntu with `bwrap` installed, enabling `[sandbox] mode = "bubblewrap"` makes shell tool execs run inside bwrap.
- Inside the sandbox, `cat /etc/passwd` succeeds (read-only); `echo x > /etc/foo` fails; `curl example.com` fails (when `network = false`).
- Inside the sandbox, the workspace is writable; building/testing the project works.
- On systems without `bwrap`, artui starts cleanly with a startup log line "bwrap not found; running unsandboxed".
- Escalate-on-failure: a denied tool can be re-tried once after explicit user approval, without sandbox.
- `cargo test` includes a Linux-gated integration test (`#[cfg(target_os = "linux")]`).

## Files touched

| File | Change |
|---|---|
| `src/sandbox/mod.rs` (new) | `SandboxKind`, `SandboxManager` trait |
| `src/sandbox/bwrap.rs` (new) | Bubblewrap impl |
| `src/sandbox/none.rs` (new) | No-op impl |
| `src/tools/shell.rs` | Wrap command in `sandbox.transform(...)` before exec |
| `src/config/schema.rs` | `[sandbox]` section |
| `src/app.rs` | At startup: detect `bwrap`, log status, instantiate `SandboxManager` |
| `src/permissions/mod.rs` | `escalate_on_failure` flow |
| Tests | `#[cfg(target_os = "linux")]` integration tests with synthetic commands |

## Bwrap command shape

```rust
// src/sandbox/bwrap.rs (sketch)
pub fn wrap(cmd: &str, cwd: &Path, network: bool, workspace: &Path) -> Vec<String> {
    let mut args = vec![
        "--ro-bind", "/usr", "/usr",
        "--ro-bind", "/lib", "/lib",
        "--ro-bind", "/lib64", "/lib64",
        "--ro-bind", "/bin", "/bin",
        "--ro-bind", "/etc/resolv.conf", "/etc/resolv.conf",
        "--proc", "/proc",
        "--dev", "/dev",
        "--tmpfs", "/tmp",
        "--bind", workspace.to_str().unwrap(), workspace.to_str().unwrap(),
        "--chdir", cwd.to_str().unwrap(),
        "--die-with-parent",
        "--new-session",
    ];
    if !network {
        args.push("--unshare-net");
    }
    args.extend(["--", "/bin/sh", "-c", cmd]);
    args.iter().map(|s| s.to_string()).collect()
}
```

## Risks

- **bwrap CAP_SYS_ADMIN**: on some systems user-namespaces are disabled (`/proc/sys/kernel/unprivileged_userns_clone = 0`). Detect and fall back to unsandboxed with a clear warning.
- **DNS in sandbox**: must bind `/etc/resolv.conf` and `/etc/nsswitch.conf`. Without them, DNS lookups fail even if `--share-net` is set.
- **Build tools needing /var/cache, ~/.cargo, ~/.npm**: cargo test wants `~/.cargo/registry/cache`. Add per-toolchain bind mounts as defaults (`~/.cargo` RO, project's `target/` RW).
- **`die-with-parent` on PR_SET_PDEATHSIG**: sandbox must propagate signals correctly. Test SIGINT handling.

## References

- Spec: `docs/spec/artui_v1_agentic_spec.md` §10
- bwrap: https://github.com/containers/bubblewrap
- codex landlock impl: `codex-rs/sandboxing/src/landlock.rs`
- codex bwrap impl: `codex-rs/sandboxing/src/bwrap.rs`
