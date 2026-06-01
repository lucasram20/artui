# Phase A — Tool-call events on `ModelEvent`

**Status:** DONE (2026-05-21)
**Summary:** Extended `ModelEvent` with `TextDelta`, `ToolCallStart`, `ToolCallArgsDelta`, `ToolCallEnd`, `ReasoningDelta`, `Usage`, `Done { end_turn }`. Added `ToolSpec`, `ToolChoice`, `ToolCall` types. Created `tool_serialization.rs` with 4 provider serializers. Implemented full OpenAI-compat SSE streaming with tool-call parsing. All 36 tests pass, clippy clean.

**Phase:** A (must-have, first)
**Spec:** `docs/spec/harness-architecture.md` §5.1, §6
**Blocks:** B, C, D, E, F (every subsequent phase needs this)
**Estimated PR size:** ~400 LoC

---

## Why

`LlmProvider::stream_turn` currently emits only `Token | Done | Error`. The model has no way to drive tools. Until this changes, no agent loop is possible regardless of how `App` is refactored.

## Scope

Extend the provider trait surface so tool calls can flow from model → harness without changing existing chat behavior.

### In scope

- Extend `ModelEvent` with tool-call arms.
- Define `ToolSpec`, `ToolChoice`, `ToolCall` types in `src/providers/mod.rs` (or a new `src/providers/tool_protocol.rs`).
- Add `tools: Vec<ToolSpec>` and `tool_choice: ToolChoice` to `ModelRequest`.
- Add per-provider serializer for `ToolSpec → serde_json::Value` (OpenAI Chat shape, OpenAI Responses shape, Anthropic Messages shape, Ollama shape).
- Update one provider impl (OpenAI-compatible) to parse `tool_calls` from streaming chunks and emit tool-call events.

### Out of scope

- Tool execution (Phase B).
- Tool registry (Phase B).
- Permission engine (Phase B/D).
- Copilot Responses-API tool parsing (later in this phase or Phase B).

## Acceptance criteria

- `cargo build` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- Existing chat with Ollama/OpenAI-compat/Copilot still works (no tools registered = no tool calls = same behavior).
- New unit test: feed a synthetic `tool_calls` SSE chunk through the OpenAI-compat parser; assert `ModelEvent::ToolCallStart`, `ToolCallArgsDelta`, `ToolCallEnd` are produced in order with the right ids.
- New unit test: serialize a `ToolSpec { name: "read_file", description: "...", parameters: {...} }` to OpenAI Chat tool JSON; round-trip parse.

## Files touched

- `src/providers/mod.rs` (or new `tool_protocol.rs`):
  - Extend `ModelEvent` enum.
  - Add `ToolSpec`, `ToolChoice`, `ToolCall` structs.
  - Extend `ModelRequest`.
- `src/providers/openai_compat.rs`:
  - Parse `delta.tool_calls[]` from SSE chunks.
  - Emit `ToolCallStart`/`ArgsDelta`/`End`.
  - Serialize `request.tools` into the request body.
- `src/providers/ollama.rs`, `account.rs`, `copilot.rs`:
  - Compile-pass only — accept `tools` field, ignore for now (will be wired in Phase B).
- New: `src/providers/tool_serialization.rs` with per-provider `to_openai_chat`, `to_openai_responses`, `to_anthropic_messages`, `to_ollama` functions.
- Tests under `src/providers/openai_compat.rs` and `src/providers/tool_serialization.rs`.

## API shape

```rust
// src/providers/mod.rs (target)

#[derive(Debug, Clone)]
pub enum ModelEvent {
    TextDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallArgsDelta { id: String, json_chunk: String },
    ToolCallEnd { id: String, arguments: serde_json::Value },
    ReasoningDelta(String),
    Usage { input_tokens: u32, output_tokens: u32 },
    Done { end_turn: bool },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

#[derive(Debug, Clone, Default, Serialize)]
pub enum ToolChoice {
    #[default] Auto,
    Required,
    None,
    Specific(String),
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: String,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub reasoning_effort: ReasoningEffort,
    pub max_output_tokens: Option<u32>,
}
```

## Migration of existing variants

`ModelEvent::Token(s)` → `ModelEvent::TextDelta(s)`. Single rename, sed-able.
`ModelEvent::Done` → `ModelEvent::Done { end_turn: true }`. Existing call sites get `end_turn: true` constant.

## Risks

- **OpenAI-compat tool-call parsing has a wrinkle**: argument JSON is streamed across multiple chunks. Implementation must accumulate `delta.tool_calls[i].function.arguments` per index until the chunk with `finish_reason: "tool_calls"` arrives.
- **Copilot Claude-family models** use Anthropic Messages shape (`tool_use` content blocks). Defer to Phase B; only OpenAI-compat lands here.
- **Existing UI** consumes `ModelEvent::Token` — rename is mechanical but every call site must compile.

## References

- Codex `ResponseEvent` enum: `codex-rs/codex-api/src/common.rs:72`
- OpenAI-compat tool_calls spec: https://platform.openai.com/docs/api-reference/chat/streaming
- Ollama 0.4 tool_calls: https://ollama.com/blog/tool-support
