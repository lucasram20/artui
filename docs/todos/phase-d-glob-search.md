# Phase D — `glob` + `search` tools, permission scaffold

**Status:** DONE (2026-05-21)
**Summary:** Added `glob` tool (using `ignore::WalkBuilder` + `glob::Pattern`), `search` tool (ripgrep wrapper with `tokio::process::Command`), and `PermissionEngine` scaffold (read-only tools auto-allowed, others → Ask). All registered in `ToolRegistry`. 52 tests pass, clippy clean. Added `ignore`, `glob`, `which` deps; added `process` feature to tokio.

**Phase:** D
**Spec:** `docs/spec/harness-architecture.md` §5.4; `artui_v1_agentic_spec.md` §8.2, §8.3, §9
**Blocks:** E
**Depends:** A, B, C
**Estimated PR size:** ~600 LoC

---

## Why

Phase B has `read_file`. The model can read but can't find. `glob` and `search` (ripgrep wrapper) make the agent useful for repo navigation. Phase D also lands the permission engine scaffold so Phase E (write tools) can hook into it without re-architecture.

## Scope

### In scope

- `src/tools/glob.rs` — pattern-based file discovery, gitignore-aware via `ignore` crate (already in Cargo.toml per spec §6).
- `src/tools/search.rs` — `rg --json` wrapper; fall back to `ignore`-based grep if `rg` is missing.
- `src/permissions/mod.rs` — `PermissionDecision`, `PermissionEngine::classify(call, agent, cfg)`.
- `src/permissions/policy.rs` — default policy from `[permissions]` config table (read-only-allowlist for these tools = `Allow`).
- `src/permissions/classifier.rs` — argv parser + chain detector (used by Phase F shell, but lands here as scaffolding).
- `[permissions]` section in `AppConfig`.
- TUI surface: tool timeline shows search hits with file:line:col previews.

### Out of scope

- `Ask` modal (Phase E).
- `Deny` enforcement for write tools (Phase E).
- Shell tool (Phase F).

## Acceptance criteria

- "find all uses of `LlmProvider`" → model issues `search`, gets paginated hits, summarizes.
- "list all .rs files under src/" → model issues `glob` and gets a sorted list.
- `rg` is detected via `which`. If absent, search falls back with a degraded-warning logged.
- Search output is capped at `AgentConfig::max_search_output_chars`; truncation noted.
- glob never returns paths outside workspace.
- `PermissionEngine::classify(read_only_tool_call, ...)` returns `Allow`.
- `cargo test` passes.

## Files touched

| File | Change |
|---|---|
| `src/tools/glob.rs` (new) | Glob tool impl using `ignore::WalkBuilder` |
| `src/tools/search.rs` (new) | rg wrapper, parses JSON, fallback path |
| `src/permissions/mod.rs` (new) | Engine + decisions |
| `src/permissions/policy.rs` (new) | Defaults + agent overlay hooks |
| `src/permissions/classifier.rs` (new) | argv parsing for shell command classification (used in Phase F) |
| `src/config/schema.rs` | Add `[permissions]` section |
| `src/tools/registry.rs` | Register `glob`, `search` |
| `src/tools/mod.rs` | Add `permissions: Arc<PermissionEngine>` to `ToolContext` |
| `src/agent/loop.rs` | Call `permissions.classify` before `tools.dispatch`; return `ToolResult::denied(...)` for Deny |
| Tests | Unit per tool + permission classifier table tests |

## Tool specs

```rust
// glob
ToolSpec {
    name: "glob",
    description: "List files matching a glob pattern within the workspace. Respects .gitignore.",
    parameters: { pattern: string (required), max_results: integer (default 200) }
}

// search
ToolSpec {
    name: "search",
    description: "Search file contents using ripgrep. Returns matches with file:line:col context.",
    parameters: {
        pattern: string (required),
        path: string (default "."),
        case_sensitive: bool (default false),
        file_glob: string (optional),
        context_lines: integer (default 2),
        max_matches: integer (default 80)
    }
}
```

## Risks

- **rg not bundled**: spec §6 says strongly recommended. README must document `dnf install ripgrep` / `apt install ripgrep` / `brew install ripgrep`. Add a soft warning at startup if `rg` is missing.
- **rg JSON output is large**: 80 matches with 2 context lines each ≈ 30k chars. Cap *after* summarizing, not before.
- **gitignore semantics**: `ignore::WalkBuilder` defaults are correct (respects `.gitignore`, `.ignore`, hidden), but don't accidentally re-implement.
- **`ripgrep::SearchBuilder` Rust crate** is an option to avoid shelling out. Defer; shelling out is simpler and matches spec §8.2.

## References

- Spec: `docs/spec/artui_v1_agentic_spec.md` §8.2 (search), §8.3 (glob), §9 (permissions)
- opencode `tool/grep.ts`, `tool/glob.ts`
- codex `core/src/tools/handlers/...` for ripgrep tool shape
- `ignore` crate: https://docs.rs/ignore
