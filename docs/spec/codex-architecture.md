# OpenAI codex-cli — Harness / Agent-Loop Architecture Reference

**Source:** `https://github.com/openai/codex` cloned to `/tmp/codex` for this audit.
**Audit date:** 2026-05-21.
**Why this doc exists:** artui's harness should not be invented from scratch. codex is one of two production references (the other is opencode in `opencode-architecture.md`). This doc captures the concrete file:line patterns that artui can port.

---

## 0. Repo shape (the punchline)

Two superficial entry points, but only one real codebase:

- `codex-cli/` — a 100-line **Node shim** (`codex-cli/bin/codex.js:1-80`) that detects platform/arch and execs the prebuilt Rust binary from `@openai/codex-{linux,darwin,win32}-{x64,arm64}` packages.
- `sdk/typescript/` — a thin SDK that just shells out to the same Rust `codex` binary and pipes JSON events back (`sdk/typescript/src/codex.ts:1-39`). Same story for `sdk/python/`.
- `codex-rs/` — the **actual harness**, a Cargo workspace with ~120 crates. Crates of interest:
  - `core/` — agent loop, tool dispatch, sandbox glue, prompt assembly, session state
  - `tools/` — tool-name + spec types, dispatch traits, tool registry primitives
  - `protocol/` — wire types (ResponseItem, AskForApproval, RolloutLine, prompts/)
  - `codex-api/` — Responses API client + SSE parser + WebSocket transport
  - `codex-client/` — HTTP/SSE/WebSocket transport layer
  - `apply-patch/` — V4A patch parser/applier
  - `sandboxing/` — Seatbelt (macOS), Landlock+seccomp (Linux), restricted-token (Windows)
  - `rollout/` — JSONL session persistence + replay
  - `mcp-server/`, `codex-mcp/`, `rmcp-client/` — MCP plumbing
  - `tui/`, `exec/`, `app-server/` — three frontends (TUI, headless `codex exec`, JSON-RPC daemon)
  - `model-provider/`, `model-provider-info/` — provider registry/abstraction

Bottom line: **codex-cli is a Rust agent harness with thin TS/Python shells.** The TS SDK is a subprocess wrapper, not the agent.

---

## 1. Agent loop

Three layers, in order from outer to inner.

### 1a. Submission → Task spawn (`core/src/tasks/`)

The TUI/exec/app-server frontends submit "tasks" against a `Session`. A submission resolves to a `SessionTask` impl (`core/src/tasks/mod.rs`); the regular conversational task is `RegularTask` (`core/src/tasks/regular.rs:39-90`).

`RegularTask::run` is the outermost loop and it just delegates straight to `run_turn` in a loop with a prewarmed `ModelClientSession`:

```rust
// core/src/tasks/regular.rs:67-96
let mut next_input = input;
let mut prewarmed_client_session = prewarmed_client_session;
loop {
    let last_agent_message = run_turn(
        Arc::clone(&sess),
        Arc::clone(&ctx),
        Arc::clone(&turn_extension_data),
        std::mem::take(&mut next_input),
        prewarmed_client_session.take(),
        cancellation_token.clone(),
    ).await;
    // … drain pending input, decide whether to keep going …
}
```

A task is spawned via `tokio::spawn`, gets its own `CancellationToken`, and the parent holds the `JoinHandle`. Cancellation is cooperative — every async call inside the turn uses `or_cancel(&cancellation_token)` to propagate.

### 1b. The turn loop (`core/src/session/turn.rs`)

`pub(crate) async fn run_turn` is the heart (`core/src/session/turn.rs:131-451`). One **turn** = potentially many sampling requests, because each tool call needs a follow-up sampling request.

```rust
// turn.rs:239-448 (paraphrased)
loop {
    let pending_input = sess.input_queue.get_pending_input(...).await;
    let sampling_request_input: Vec<ResponseItem> = sess.clone_history()
        .await
        .for_prompt(&turn_context.model_info.input_modalities);

    match run_sampling_request(/* ... */).await {
        Ok(SamplingRequestResult { needs_follow_up, last_agent_message }) => {
            let token_status = auto_compact_token_status(...).await;
            if token_status.token_limit_reached && needs_follow_up {
                run_auto_compact(...).await?;     // self-summarize before continuing
                continue;
            }
            if !needs_follow_up { /* ... */ break; }
            continue;
        }
        Err(CodexErr::TurnAborted) => break,
        Err(CodexErr::InvalidImageRequest()) => { /* sanitize + retry */ continue; }
        Err(e) => { sess.send_event(EventMsg::Error(...)); break; }
    }
}
```

The signal that drives the loop is `needs_follow_up`. It is set whenever the assistant turn produced any tool call (because the model needs to see results), or whenever the model's `response.completed` carried `end_turn: false`, or whenever queued user input is waiting to be drained mid-turn.

### 1c. The sampling-request loop (`run_sampling_request` → `try_run_sampling_request`)

`run_sampling_request` (`turn.rs:945-1046`) wraps **stream retries** around a single Responses-API call. It catches `CodexErr::ContextWindowExceeded` and `CodexErr::UsageLimitReached` as terminal, otherwise checks `err.is_retryable()` and re-calls `try_run_sampling_request` with backoff until `provider.info().stream_max_retries()` is exhausted (default 5). It can also flip transport from WebSocket → HTTP SSE on the way: `client_session.try_switch_fallback_transport(...)`.

`try_run_sampling_request` (`turn.rs:1712-2189`) then opens a stream and walks `ResponseEvent`s one at a time:

```rust
let mut stream = client_session.stream(prompt, &model_info, ...)
    .or_cancel(&cancellation_token).await??;

let outcome: CodexResult<SamplingRequestResult> = loop {
    let event = match stream.next().or_cancel(&cancellation_token).await? {
        Some(Ok(ev)) => ev,
        Some(Err(e)) => break Err(e),
        None => break Err(CodexErr::Stream("stream closed before response.completed", None)),
    };
    match event {
        ResponseEvent::Created => {}
        ResponseEvent::OutputItemAdded(item) => { /* stream-start */ }
        ResponseEvent::OutputItemDone(item) => {
            let output_result = handle_output_item_done(&mut ctx, item, prev).await?;
            if let Some(tool_future) = output_result.tool_future {
                in_flight.push_back(tool_future);   // dispatched, but result waited on later
            }
            needs_follow_up |= output_result.needs_follow_up;
        }
        ResponseEvent::OutputTextDelta(delta) => { /* stream tokens to UI */ }
        ResponseEvent::ToolCallInputDelta { .. } => { /* live tool-arg diff to UI */ }
        ResponseEvent::Completed { response_id, token_usage, end_turn } => {
            if let Some(false) = end_turn { needs_follow_up = true; }
            break Ok(SamplingRequestResult { needs_follow_up, last_agent_message });
        }
    }
};
drain_in_flight(&mut in_flight, sess.clone(), turn_context.clone()).await?;
```

Three things matter:

1. **Tool calls are not awaited inline.** `handle_output_item_done` returns a boxed future that's pushed onto a `FuturesOrdered<BoxFuture<…>>` named `in_flight`. The streaming loop continues consuming model events. After `Completed`, `drain_in_flight` runs, which is what enables `parallel_tool_calls`.
2. **Reasoning, text, and tool-arg deltas are routed to the UI live** — there is no buffer-then-flush.
3. **Iteration cap is implicit.** Termination conditions: model says `end_turn:true` and emits no tool call, or `auto_compact` triggers context-limit recovery, or cancellation, or unrecoverable error.

### Streaming → `ResponseEvent` enum

```rust
// codex-rs/codex-api/src/common.rs:72-75
pub enum ResponseEvent {
    Created,
    OutputItemDone(ResponseItem),
    OutputItemAdded(ResponseItem),
    Completed { response_id: String, token_usage: Option<TokenUsage>, end_turn: Option<bool> },
    OutputTextDelta(String),
    ToolCallInputDelta { item_id: String, call_id: String, delta: String },
    ReasoningSummaryDelta { /* ... */ },
    ReasoningContentDelta { /* ... */ },
    RateLimits(/* ... */),
    ServerModel(/* ... */),
}
```

Both SSE and WebSocket transports produce the same enum.

---

## 2. Tool calling protocol

Codex speaks the **OpenAI Responses API** exclusively. `WireApi::Chat` was **removed**. Tool calls come over the wire as native Responses API `function_call` items.

The model-facing tool definition is built by `create_tools_json_for_responses_api(&prompt.tools)` (`core/src/client.rs:728`). Specs are typed (`codex_tools::ToolSpec`) and the registry exposes them via `ToolRouter::model_visible_specs()`.

The router converts a `ResponseItem` into a `ToolCall`:

```rust
// core/src/tools/router.rs:90-137
pub fn build_tool_call(item: ResponseItem) -> Result<Option<ToolCall>, FunctionCallError> {
    match item {
        ResponseItem::FunctionCall { name, namespace, arguments, call_id, .. } => {
            Ok(Some(ToolCall {
                tool_name: ToolName::new(namespace, name),
                call_id,
                payload: ToolPayload::Function { arguments },
            }))
        }
        ResponseItem::CustomToolCall { name, input, call_id, .. } => {
            Ok(Some(ToolCall { tool_name: ToolName::plain(name), call_id, payload: ToolPayload::Custom { input } }))
        }
        _ => Ok(None),
    }
}
```

Tool calls are parsed at **`OutputItemDone`** time (i.e. when the SSE provider emits a fully-assembled item), not chunk-by-chunk. Argument *deltas* are still emitted by the API for UX (`ResponseEvent::ToolCallInputDelta`), and the registry can attach a `ToolArgumentDiffConsumer` that interprets partial JSON to render in-progress tool-arg previews — most prominently `apply_patch` shows the diff as it streams in.

---

## 3. Tool dispatch

Layered like an onion: **handler → registry → orchestrator → sandbox manager → exec**.

### Built-in tools

| Tool | Source / location |
|---|---|
| `shell` (and `local_shell`) | `tools/tool_family/shell.rs`, runtime `tools/runtimes/shell.rs` |
| `apply_patch` | `tools/handlers/ApplyPatchHandler` |
| `update_plan` | `PlanHandler` |
| `view_image` | `ViewImageHandler` |
| `request_user_input` | `RequestUserInputHandler` |
| `tool_search` | when namespaced tools enabled |
| **MCP tools** | dynamically attached from any connected MCP server |
| Multi-agent v1/v2 (`spawn_agent`, `wait_agent`, `close_agent`) | `tools/handlers/multi_agents*` |
| Hosted: `web_search`, `image_generation` | declared as specs only; the *server* runs them |
| `code_mode` | `tools/code_mode.rs` |

### ToolRegistry

```rust
// core/src/tools/registry.rs:249-269
pub struct ToolRegistry {
    tools: HashMap<ToolName, Arc<dyn CoreToolRuntime>>,
}
```

`dispatch_any_with_terminal_outcome` is the main entry. Every tool call goes through it and gets:

1. Active-turn accounting
2. **PreToolUse hooks** — can `Block(message)`, `Continue { updated_input }`, or no-op
3. OTel-traced execution
4. **PostToolUse hooks** — can replace the response text or stop the turn
5. Lifecycle notifications
6. Goal-runtime accounting

### ToolOrchestrator — approval + sandbox + retry

For shell-like tools the handler delegates to `ToolOrchestrator::run`. Sequence:

1. Compute approval requirement from policy + sandbox.
2. Approval phase: `Skip`, `Forbidden`, or `NeedsApproval`. Approval can be answered by a hook, by the **Guardian** (automated reviewer model), or punted to user.
3. Initial sandboxed attempt: pick `SandboxType` via `SandboxManager::select_initial(...)`.
4. On `SandboxErr::Denied`: if policy permits, ask user/guardian to re-approve **without sandbox**, retry once with `SandboxType::None`.
5. Network approvals run concurrently.

### Approval modes

```rust
// codex-rs/protocol/src/protocol.rs:764-794
pub enum AskForApproval {
    UnlessTrusted,
    OnFailure,
    OnRequest,
    Never,
    Granular(GranularConfig),
}
```

### Sandbox

`codex-sandboxing` implements:

- macOS Seatbelt via `sandbox-exec` with a generated `.sb` policy file.
- Linux Landlock + seccomp via a `codex-linux-sandbox` helper exec; Bubblewrap fallback.
- Windows restricted-token with optional private desktop and elevated levels.
- A managed network proxy (`codex-network-proxy`) that exec'd commands route through.

---

## 4. Provider abstraction

- **`codex-model-provider-info`** — declarative `ModelProviderInfo` struct from built-in defaults and `~/.codex/config.toml [model_providers]`. Holds `base_url`, `env_key`, `auth`, `aws`, `wire_api: Responses`, `http_headers`, `request_max_retries`, `stream_max_retries`, `stream_idle_timeout_ms`, `supports_websockets`.
- **`codex-model-provider`** — concrete impls: `openai`, `chatgpt` (login-based), `azure`, `amazon-bedrock` (SigV4), `ollama`, `lmstudio`. **`wire_api = "chat"` is rejected** — every provider must speak Responses API.

`ModelClient` (`core/src/client.rs:163-2245`) is session-scoped (auth, provider, conversation id, transport fallback state). `ModelClientSession` is turn-scoped and holds the WebSocket connection cache.

```rust
pub async fn stream(...) -> Result<ResponseStream> {
    let wire_api = self.client.state.provider.info().wire_api;
    match wire_api {
        WireApi::Responses => /* try websocket if supports_websockets, else SSE */
    }
}
```

The SSE parser in `codex-api/src/sse/responses.rs` reads `data: {…}` lines, dispatches by `type` field.

---

## 5. Session / state

### In-memory

`Session` owns: `services`, `state: Mutex<SessionState>`, `active_turn`, `input_queue`, `features`, `conversation_id`. `SessionState` holds the conversation history. `TurnContext` is per-turn config.

### Persisted: rollouts (`codex-rs/rollout/`)

Every session is recorded to a JSONL file under `~/.codex/sessions/rollout-<rfc3339>-<thread-id>.jsonl`. The `RolloutRecorder` is a clonable handle backed by a Tokio writer task. `RolloutCmd` is `AddItems(Vec<RolloutItem>) | Persist | Flush | Shutdown`.

The first line is a `SessionMeta` line (id, source, base_instructions, dynamic_tools, git info from `collect_git_info`). Subsequent `RolloutLine`s carry every model item, every tool call, every approval, every diff.

`RolloutRecorderParams::Resume { path }` reads to **resume** a session. There is also a `state_db` SQLite index for fast listing across thousands of sessions.

---

## 6. Prompt construction

Codex carries multiple prompt variants compiled in via `include_str!`:

- `protocol/src/prompts/base_instructions/default.md`
- `core/prompt_with_apply_patch_instructions.md`
- `core/gpt_5_codex_prompt.md`, `gpt-5.2-codex_prompt.md`, `gpt-5.1-codex-max_prompt.md`, `gpt_5_1_prompt.md`, `gpt_5_2_prompt.md`
- `core/review_prompt.md`
- `core/templates/personalities/*.md`, `core/templates/compact/prompt.md`

The selection ends up in `BaseInstructions { text, source, … }` and is fetched via `sess.get_base_instructions().await`.

```rust
// core/src/session/turn.rs:886-901
pub(crate) fn build_prompt(
    input: Vec<ResponseItem>,
    router: &ToolRouter,
    turn_context: &TurnContext,
    base_instructions: BaseInstructions,
) -> Prompt {
    Prompt {
        input,
        tools: router.model_visible_specs(),
        parallel_tool_calls: turn_context.model_info.supports_parallel_tool_calls,
        base_instructions,
        personality: turn_context.personality,
        output_schema: turn_context.final_output_json_schema(),
        output_schema_strict: turn_context.final_output_json_schema_strict(),
    }
}
```

Context injection adds: skills/plugins from `@plugin:`/`/skill` mentions, AGENTS.md walk from cwd up, MCP/connector tools, pre-sampling auto-compact summary if needed, `PreInputHooks`.

---

## 7. Diff / file-edit primitives

V4A patch format only:

```
*** Begin Patch
*** Update File: path/to/file.py
@@ def example():
- pass
+ return 123
*** End Patch
```

Implementation:

- Parser: `apply-patch/src/parser.rs` (954 lines). Produces `Vec<Hunk>`, where `Hunk` is `Add | Delete | Update`.
- Streaming parser: `apply-patch/src/streaming_parser.rs` (851 lines). Lets the UI render the diff while args are still streaming.
- Fuzzy locate: `apply-patch/src/seek_sequence.rs` — finds where a `@@ context` block matches in the existing file even with drift.
- Application: produces `AppliedPatchDelta` and emits a unified diff via the `similar` crate.
- Patch safety: `core/src/safety.rs::assess_patch_safety` checks every changed path against the sandbox's writable roots.
- Per-turn diff tracking: `core/src/turn_diff_tracker.rs`.

There is **no full-file rewrite mode and no AST-aware editor** — V4A is the only format.

---

## 8. Files for artui to study while porting

- Agent loop: `codex-rs/core/src/session/turn.rs:131` (run_turn), `:945` (run_sampling_request), `:1712` (try_run_sampling_request), `:886` (build_prompt)
- Task spawn: `codex-rs/core/src/tasks/regular.rs:39-96`
- Tool dispatch: `codex-rs/core/src/tools/router.rs:90-217`, `codex-rs/core/src/tools/registry.rs:249-619`, `codex-rs/core/src/tools/orchestrator.rs:128-489`
- Stream events: `codex-rs/codex-api/src/common.rs:72-90`, `codex-rs/codex-api/src/sse/responses.rs:1-500`
- Provider: `codex-rs/model-provider-info/src/lib.rs:45-200`, `codex-rs/core/src/client.rs:1224-1582`
- Approvals + safety: `codex-rs/protocol/src/protocol.rs:764` (AskForApproval), `codex-rs/core/src/safety.rs:1-197`
- Sandbox: `codex-rs/sandboxing/src/manager.rs:139-200`, `seatbelt.rs`, `landlock.rs`
- Apply-patch: `codex-rs/apply-patch/src/lib.rs:1-200`, `parser.rs`, `streaming_parser.rs`
- Rollout: `codex-rs/rollout/src/recorder.rs:73-200`, `state_db.rs`
- System prompts: `codex-rs/core/prompt_with_apply_patch_instructions.md`, `gpt_5_codex_prompt.md`

---

## 9. License

codex-rs is Apache-2.0. The V4A `apply-patch` parser, the `RolloutRecorder` JSONL design, and the `ResponseEvent` enum can all be ported with attribution. See `harness-architecture.md` §6 for sequencing.
