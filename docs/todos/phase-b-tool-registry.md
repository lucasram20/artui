# Phase B — Tool registry + first tool

**Status:** DONE (2026-05-21)
**Summary:** Created `src/tools/` module with `Tool` trait, `ToolContext`, `ToolResult`, `ToolRegistry`, and `read_file` tool. Registry wired into `App` so `ModelRequest.tools` is populated from registered tools. Path traversal rejection, binary detection, line-number formatting, truncation all tested. 44 tests pass.

**Phase:** B
**Spec:** `docs/spec/harness-architecture.md` §5.2, §6
**Blocks:** C
**Depends:** A
**Estimated PR size:** ~500 LoC

---

## Why

After Phase A, `ModelEvent` carries tool calls but nothing executes them. Phase B builds the tool dispatch path end-to-end with the smallest concrete tool: `read_file`.

## Scope

### In scope

- New `src/tools/` module with `Tool` trait, `ToolContext`, `ToolResult`.
- New `src/tools/registry.rs` — `ToolRegistry::new() -> Self`, `dispatch(call) -> Future<ToolResult>`.
- First tool: `src/tools/read_file.rs`.
- Wire `ToolRegistry` into `App` so `request.tools` is populated when sending to provider.
- Convert tool result into a `Message` of role=`tool` so the next prompt sees it (still happens in Phase C; Phase B can stub).

### Out of scope

- Permission engine (scaffold-only; full Phase D).
- Loop iteration (Phase C).
- glob/search/apply_patch/shell (Phases D, E, F).

## Acceptance criteria

- Sending a prompt that triggers a `read_file` tool call (e.g. "what does Cargo.toml look like?") to OpenAI-compat or Copilot returns the file content as a tool-result message visible in the TUI.
- `read_file` rejects paths outside workspace (`../../etc/passwd`).
- `read_file` caps output at `AgentConfig::max_read_file_chars`.
- Output includes line numbers (`  1: ...`).
- New unit tests:
  - `read_file` rejects path-traversal.
  - `read_file` truncates at cap and notes truncation.
  - `ToolRegistry::dispatch` round-trips a synthetic `ToolCall`.

## Files touched

| File | Change |
|---|---|
| `src/tools/mod.rs` (new) | `Tool` trait, `ToolContext`, `ToolResult`, `ToolError` |
| `src/tools/registry.rs` (new) | `ToolRegistry` |
| `src/tools/read_file.rs` (new) | First tool impl |
| `src/agent/mod.rs` | Re-export tool types |
| `src/app.rs` | Build a `ToolRegistry`; populate `ModelRequest.tools` from `registry.specs_for(agent)` |
| `src/providers/openai_compat.rs` | Already emits tool-call events from Phase A; wire into `App` event handling so a `ToolCallEnd` triggers `registry.dispatch` |
| `src/lib.rs` | `pub mod tools;` |
| Tests | Unit + integration |

## API shape

```rust
// src/tools/mod.rs

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn execute(&self, args: serde_json::Value, ctx: ToolContext) -> ToolResult;
}

pub struct ToolContext {
    pub session_id: SessionId,
    pub call_id: String,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<AppEvent>,
    // permissions: Arc<PermissionEngine>,  // Phase D
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub error: Option<String>,
    pub artifact_path: Option<PathBuf>,
}

impl ToolResult {
    pub fn ok(call_id: String, content: String) -> Self { /* … */ }
    pub fn error(call_id: String, error: String) -> Self { /* … */ }
}

// src/tools/read_file.rs

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a file from the workspace with optional line range. Output includes line numbers.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to workspace root" },
                    "start_line": { "type": "integer", "minimum": 1 },
                    "end_line":   { "type": "integer", "minimum": 1 }
                },
                "required": ["path"]
            }),
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: ToolContext) -> ToolResult {
        // 1. Parse args (path, start_line, end_line).
        // 2. Canonicalize path. Reject if not under workspace_root.
        // 3. Detect binary (first 8KB heuristic). Reject.
        // 4. Read; slice lines [start_line..=end_line] (1-indexed).
        // 5. Cap at max_read_file_chars; note truncation.
        // 6. Format with line numbers.
    }
}
```

## Risks

- **Path canonicalization on Windows**: `std::fs::canonicalize` returns `\\?\` extended paths. Use `dunce::canonicalize` if cross-platform compat matters; else document Linux/Mac focus.
- **Symlink escapes**: canonicalize must follow symlinks; any resolution outside `workspace_root` → reject.
- **Large files**: cap is char-based; for binary-detected files reject before allocating.

## References

- Spec: `docs/spec/artui_v1_agentic_spec.md` §8.4 (`read_file`)
- opencode `tool/read.ts`
- codex `core/src/tools/handlers/...` for ReadFileHandler shape
