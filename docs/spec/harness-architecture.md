# artui Harness Architecture

**Status:** v1 design (2026-05-21)
**Companion docs:** `codex-architecture.md`, `opencode-architecture.md`, `session-persistence.md`, `copilot-oauth.md`

This is the architectural blueprint for turning artui from a chat TUI with provider plumbing into a real coding-agent harness, comparable in capability (not scope) to OpenAI codex-cli and sst/opencode.

It is the synthesis of the codebase audit (`graphify-out/GRAPH_REPORT.md`), the v1 spec (`artui_v1_agentic_spec.md`), and concrete pattern extraction from codex-rs and opencode.

---

## 1. The diagnosis

What artui is today:

- A streaming chat TUI with multi-provider support (Ollama, OpenAI-compatible, OpenAI account stub, GitHub Copilot).
- Theme/popup/slash/statusline/animation polish.
- GitHub Copilot wire-API routing across `/chat/completions`, `/responses`, `/v1/messages`.
- A single god-object `App` (degree 86, betweenness 0.178) that owns every UI surface and every async result.

What artui is **not** today:

- An agent. There is no agent loop, no tool registry, no tool dispatch, no permission engine, no sandbox, no patch tool, no search/read tool, no session persistence, no prompt assembly, no compaction.
- The `LlmProvider::stream_turn` trait emits only `Token | Done | Error`. There is no `ToolCall` event arm. Without it, the model cannot drive tools, and no agent loop is possible regardless of how `App` is refactored.
- `PrimaryAgent::{Build, Plan}` exists as an enum but is never invoked, never alters the tool whitelist, never alters the system prompt at runtime.
- The "system prompt" lives in `docs/system-prompts/cli-prompt.md` as reference text. It is not loaded by the runtime.
- Context % was a transcript-char-vs-tool-output-cap ratio. The display landed; actual context budgeting did not.

The v1 spec describes the harness. The current code does not implement it.

---

## 2. Reference shapes

### 2.1 codex-rs (Rust, ~120 crates)

Three nested loops.

1. `RegularTask::run` (`codex-rs/core/src/tasks/regular.rs:39-96`) — outer task loop, `tokio::spawn`, owns a `CancellationToken`.
2. `run_turn` (`codex-rs/core/src/session/turn.rs:131`) — per-turn loop, decides whether to call the model again based on `needs_follow_up` (any tool call OR `end_turn:false`).
3. `try_run_sampling_request` (`codex-rs/core/src/session/turn.rs:1712`) — inner stream loop, walks `ResponseEvent`s. Tool calls are pushed onto a `FuturesOrdered<BoxFuture>` and awaited after `Completed`, enabling parallel tool calls.

Tool dispatch:

- `ToolRouter` → `ToolRegistry::dispatch_any` → `ToolOrchestrator::run`.
- Wraps every call with `PreToolUse`/`PostToolUse` hooks, OTel tracing, lifecycle accounting.
- Sandbox: Seatbelt (mac), Landlock+seccomp+Bubblewrap (linux), restricted-token (win), managed network proxy.
- Approval: `UnlessTrusted | OnFailure | OnRequest | Never | Granular`. Plus a Guardian model that can auto-approve.
- Edit primitive: V4A `apply_patch` only. Streaming parser renders the diff as the tool args stream in. `TurnDiffTracker` keeps a per-turn unified diff.

State:

- `Session` in-memory + JSONL `RolloutRecorder` under `~/.codex/sessions/`.
- SQLite index for fast listing across thousands of sessions.
- Resume reconstructs a `Session` from the rollout file.

Provider:

- `ModelProviderInfo` declarative registry from `~/.codex/config.toml`.
- Responses API only. `wire_api = "chat"` was removed.
- Transports: WebSocket (beta) with SSE fallback. Both feed a unified `ResponseEvent` enum.

### 2.2 opencode (Effect-TS / Bun)

One-and-a-half loops.

1. `runLoop` (`packages/opencode/src/session/prompt.ts:1240`) — outer `while(true)`; calls `streamText` once per iteration, terminates when `finish_reason` is anything but `tool-calls` and there are no unresolved tool calls.
2. `streamText` itself (Vercel AI SDK) handles a single turn including tool calls and tool results internally.

Result: there is **no hand-rolled mid-stream tool dispatch**. AI SDK owns one turn; opencode reruns it.

Tool dispatch:

- AI SDK `tool()` adapter (`session/tools.ts`).
- Permissions decoupled via `ctx.ask(...)` — every write tool calls the same `Permission.ask` Effect, which blocks on a `Deferred`. Reply via `POST /permission/:id/reply`.
- `experimental_repairToolCall` self-corrects malformed tool calls.

Built-in tools: `shell`, `read`, `write`, `edit`, `glob`, `grep`, `task`, `todo`, `lsp`, `skill`, `webfetch`, `websearch`, `apply_patch`, `patch`, `question`, `plan`, `repo_clone`, `repo_overview`.

State:

- Drizzle SQLite at `~/.local/share/opencode/opencode.db` plus per-workspace files.
- `MessageV2` rows + parts. `MessageV2.toModelMessagesEffect` rebuilds `ModelMessage[]` for `streamText`.
- Snapshot system tracks workspace as content tree at every `step-start`; emits a `patch` part with diffs at `step-finish`.

Server / TUI:

- Bun HTTP server + Go TUI. Single `/event` SSE channel fans out everything. REST routes for sessions, messages, permissions, TUI RPCs.
- Same surface that VSCode/web/Slack integrations use.

Sub-agent (`tool/task.ts`): a fresh session running the same loop with derived permissions. Background mode + `task_status` polling tool.

### 2.3 What artui should keep, drop, and copy

| Pattern | Source | Take? | Why |
|---|---|---|---|
| Three-level nested loop | codex-rs | yes | Maps cleanly to spec §7 pseudocode |
| `ResponseEvent` unified enum | codex-rs | yes | Already partly there as `ModelEvent`; needs tool-call arms |
| Tool dispatch with `PreToolUse`/`PostToolUse` hooks | codex-rs | partial | Hooks are nice-to-have; ship without them in v1 |
| V4A `apply_patch` parser | codex-rs (Apache-2.0) | yes | Reuse code or port idiom |
| Sandbox via Seatbelt/Landlock/restricted-token | codex-rs | partial | Linux-first via `bwrap`; mac/win can wait |
| Effect-TS Layers | opencode | no | Wrong language, but the *idea* of `Permission.ask` Effect → port as `permissions::ask(call) -> Future<ApprovalResult>` |
| AI SDK `streamText` ownership of one turn | opencode | partial | Rust has no AI SDK; we own all stream parsing. But the loop shape (`while finish_reason == tool-calls`) is the right one |
| `experimental_repairToolCall` | opencode | yes | Cheap self-correct for malformed tool calls |
| SQLite session DB | opencode | yes | See `session-persistence.md` |
| Snapshot at every step-start | opencode | yes (deferred) | Free revert/diff stream |
| Sub-agent = fresh session | opencode | yes (deferred) | Defer until basic loop works |
| Per-tool permission patterns with wildcards | opencode | yes | Maps to spec §9 |
| Single SSE/`AppEvent` fan-out bus | opencode | partial | Rust mpsc is enough for in-process; defer SSE until artui-server split |

---

## 3. Target architecture

```
                         ┌──────────────┐
        keystroke ──►    │  TUI (App)   │ ◄── render frames
                         └──────┬───────┘
                                │ AppEvent
                         ┌──────▼───────┐
                         │   Session    │  transcript, tool_log,
                         │              │  agent_id, cancellation_token
                         └──────┬───────┘
                                │ run_turn
                         ┌──────▼───────┐
                         │  AgentRunner │  while !done:
                         │              │    provider.stream_turn() →
                         │              │    ToolDispatcher →
                         │              │    feed result back
                         └────┬───┬─────┘
                              │   │
              ┌───────────────┘   └────────────────┐
              ▼                                    ▼
       ┌──────────────┐                    ┌────────────────┐
       │ LlmProvider  │                    │ ToolDispatcher │
       │              │                    │ ┌────────────┐ │
       │ Ollama       │                    │ │ Permission │ │
       │ OpenAICompat │                    │ │  Engine    │ │
       │ Account      │                    │ └─────┬──────┘ │
       │ Copilot      │                    │       ▼        │
       └──────────────┘                    │ ┌────────────┐ │
                                           │ │  Sandbox   │ │
                                           │ │  (bwrap)   │ │
                                           │ └─────┬──────┘ │
                                           │       ▼        │
                                           │ ┌────────────┐ │
                                           │ │ ToolRegistry│ │
                                           │ │  read       │ │
                                           │ │  glob       │ │
                                           │ │  search     │ │
                                           │ │  apply_patch│ │
                                           │ │  shell      │ │
                                           │ │  todo       │ │
                                           │ └────────────┘ │
                                           └────────┬───────┘
                                                    │ writes
                                                    ▼
                                           ┌────────────────┐
                                           │ Workspace + FS │
                                           │ (cwd-bounded)  │
                                           └────────────────┘
                              │
                              ▼
                       ┌──────────────┐
                       │ SqliteRollout│  ~/.local/share/artui/artui.db
                       │ (sessions,   │  - sessions table
                       │  messages,   │  - messages table (rebuildable transcript)
                       │  tool_calls, │  - tool_calls table
                       │  patches)    │  - patches table
                       └──────────────┘
```

Process layout v1: single binary, single process, no client/server split. Phase F+ may split out an `artui-server` to match opencode's HTTP+SSE shape, but not required for the harness to be correct.

---

## 4. Module decomposition

Split the current god-object `App` along the dotted lines below. The graph audit shows `App` bridges 9 communities; most of those communities map cleanly to the new modules.

```
src/
├── main.rs
├── lib.rs
├── app.rs                 (legacy god-object during migration)
├── ui/                    (was: most of C1, C4, C12, C15, C17, C21, C25, C28, C31, C36, C39)
│   ├── mod.rs
│   ├── layout.rs
│   ├── chat.rs
│   ├── tools.rs           ← tool timeline (new)
│   ├── diff.rs            ← diff preview popup (new)
│   └── popups.rs
├── session/               (new)
│   ├── mod.rs             pub struct Session, SessionId, transcript, tool_log
│   ├── persistence.rs     SQLite read/write (see session-persistence.md)
│   └── compaction.rs      compact when tokens >= 0.835 * window
├── agent/                 (was: C24/C38 enum + new loop)
│   ├── mod.rs             re-exports
│   ├── primary.rs         PrimaryAgent { Build, Plan } enum + system_prompt()
│   ├── loop.rs            run_turn() per spec §7
│   ├── prompts.rs         build_system_prompt() — env block + AGENTS.md walk + tool spec injection
│   └── parser.rs          tool-call extraction from provider events
├── tools/                 (new)
│   ├── mod.rs             pub trait Tool { fn spec() -> ToolSpec; async fn execute(args, ctx) -> ToolResult; }
│   ├── registry.rs        HashMap<&'static str, Arc<dyn Tool>>; dispatch
│   ├── read_file.rs
│   ├── glob.rs
│   ├── search.rs          ripgrep wrapper, fallback to ignore-crate walk
│   ├── apply_patch.rs     V4A parser/applier (cribbed from codex-rs)
│   ├── shell.rs           classifier-gated execution
│   ├── todo.rs            session-scoped todo list (model self-managed)
│   └── git_status.rs
├── permissions/           (was: missing)
│   ├── mod.rs             pub enum PermissionDecision { Allow, Ask(Prompt), Deny(String) }
│   ├── classifier.rs      argv parse + chain detection
│   └── policy.rs          default policy + project trust
├── sandbox/               (new, Linux-first)
│   ├── mod.rs
│   └── bwrap.rs           ro-bind /usr/bin/lib + bind workspace + unshare-net + die-with-parent
├── providers/             (largely as-is; extend ModelEvent)
├── auth/                  (as-is; minor changes from copilot-oauth.md)
├── config/                (extend with [permissions], [sandbox], [agent])
└── util/
```

---

## 5. The contract changes (v1)

### 5.1 `LlmProvider` trait

Today (`src/providers/mod.rs`):

```rust
pub enum ModelEvent {
    Token(String),
    Done,
    Error(String),
}
```

Required:

```rust
pub enum ModelEvent {
    /// Plain text delta from the assistant message.
    TextDelta(String),
    /// Model started a tool call. Emitted once per call before any args.
    ToolCallStart { id: String, name: String },
    /// Streaming JSON args for an in-progress tool call.
    ToolCallArgsDelta { id: String, json_chunk: String },
    /// Tool call complete. `arguments` is the assembled JSON.
    ToolCallEnd { id: String, arguments: serde_json::Value },
    /// Model reasoning trace (Anthropic / Copilot Responses API).
    ReasoningDelta(String),
    /// Token usage for the turn.
    Usage { input_tokens: u32, output_tokens: u32 },
    /// Stream finished. `end_turn` mirrors codex semantics.
    Done { end_turn: bool },
    Error(String),
}

pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system: String,
    pub tools: Vec<ToolSpec>,           // NEW
    pub tool_choice: ToolChoice,        // NEW: Auto | Required | None
    pub reasoning_effort: ReasoningEffort,
    pub max_output_tokens: Option<u32>,
}
```

`ToolSpec` is a typed Rust struct. Per-provider serializers convert it:

- OpenAI Chat Completions → `tools: [{type: "function", function: { name, description, parameters }}]`, `tool_choice`.
- OpenAI Responses API (Copilot newer models) → `tools: [{type: "function", name, description, parameters}]` (flat).
- Anthropic Messages API (Copilot Claude models, Anthropic native later) → `tools: [{name, description, input_schema}]`.
- Ollama → `tools: [{type: "function", function: {…}}]` (Ollama 0.4+).

### 5.2 `Tool` trait

```rust
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
    pub permissions: Arc<PermissionEngine>,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<AppEvent>,
}

pub struct ToolResult {
    pub call_id: String,
    pub content: String,         // model-facing text (capped per AgentConfig)
    pub error: Option<String>,
    pub artifact_path: Option<PathBuf>,  // if output was capped, full output here
}
```

### 5.3 Agent loop (per spec §7, reified)

```rust
pub async fn run_turn(
    user_input: Vec<MessagePart>,
    session: &mut Session,
    agent: PrimaryAgent,
    cfg: &AppConfig,
    provider: &dyn LlmProvider,
    tools: &ToolRegistry,
    permissions: &PermissionEngine,
    cancel: CancellationToken,
    tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    session.push(Message::user(user_input));

    for step in 0..cfg.agent.max_steps_per_turn {
        let request = build_model_request(session, agent, &cfg, tools.specs_for(agent))?;
        let mut stream = provider.stream_turn(request, cancel.child_token()).await?;

        let mut pending_calls: Vec<ToolCall> = Vec::new();
        let mut assistant_text = String::new();
        let mut end_turn = false;

        while let Some(ev) = stream.next().await {
            match ev? {
                ModelEvent::TextDelta(t) => {
                    tx.send(AppEvent::AssistantToken(t.clone())).await?;
                    assistant_text.push_str(&t);
                }
                ModelEvent::ToolCallEnd { id, name, arguments } => {
                    pending_calls.push(ToolCall { id, name, arguments });
                }
                ModelEvent::Done { end_turn: et } => { end_turn = et; break; }
                ModelEvent::Error(e) => return Err(anyhow!(e)),
                _ => {}
            }
        }

        if !assistant_text.is_empty() {
            session.push(Message::assistant_text(assistant_text));
        }

        if pending_calls.is_empty() {
            // No tool calls. End turn whether end_turn flag is set or not.
            return Ok(());
        }

        // Dispatch all pending calls (parallelize where possible).
        for call in pending_calls {
            let decision = permissions.classify(&call, agent, &cfg).await;
            let result = match decision {
                PermissionDecision::Allow => tools.dispatch(call, &ctx).await,
                PermissionDecision::Ask(prompt) => {
                    tx.send(AppEvent::ApprovalRequested(prompt)).await?;
                    let approved = wait_for_approval(&mut session, cancel.child_token()).await?;
                    if approved { tools.dispatch(call, &ctx).await }
                    else { ToolResult::denied_by_user(call.id) }
                }
                PermissionDecision::Deny(reason) => ToolResult::denied(call.id, reason),
            };
            session.push(Message::tool_result(result));
        }
    }

    session.push(Message::assistant_text(MAX_STEPS_REACHED.into()));
    Ok(())
}
```

Note: Phase A keeps it sequential. Parallel tool calls (codex `FuturesOrdered`) come later — most providers' Rust SDKs already support it natively when the request sets `parallel_tool_calls: true`.

### 5.4 Permission engine (per spec §9)

```rust
pub enum PermissionDecision {
    Allow,
    Ask(ApprovalPrompt),
    Deny(String),
}

pub struct PermissionEngine {
    policy: PermissionPolicy,                  // from config
    session_approvals: HashMap<String, Approved>,  // "always" approvals
    project_trusted: bool,
}

impl PermissionEngine {
    pub fn classify(&self, call: &ToolCall, agent: PrimaryAgent, cfg: &AppConfig) -> PermissionDecision { … }
}
```

Default policy from `[permissions]` config:

```toml
[permissions]
default = "ask"
read_only = "allow"
apply_patch = "ask"
shell = "ask"
network = "ask"
privilege_escalation = "deny"
outside_workspace = "deny"
```

Plan agent overlay forces `apply_patch = "deny"`, `shell = "deny"`, makes the agent read-only.

---

## 6. Build order (smallest correct sequence)

| Phase | Changes | Outcome |
|---|---|---|
| **A** | Extend `ModelEvent` with tool-call arms; add `ToolSpec`, `ToolChoice`; per-provider serializer | Provider can carry tool calls |
| **B** | Add `Tool` trait + `ToolRegistry`; `read_file` tool; wire into one provider (OpenAI-compat first) | Smallest end-to-end agent step |
| **C** | Add `Session` extracted from `App`; `agent::loop::run_turn`; cancellation token threaded through | Real loop, replaces current single-shot chat |
| **D** | Add `glob`, `search` tools (read-only); permission engine scaffold | Useful read-only agent |
| **E** | Add `apply_patch` (V4A) + diff preview popup + permission `Ask` flow | First write-class tool |
| **F** | Add `shell` tool with classifier; output caps to disk | Verification loop |
| **G** | SQLite persistence: `Session` write-through; resume; see `session-persistence.md` | Crash-safe, resumable |
| **H** | Compaction trigger at 0.835 × context window via hidden compaction sub-prompt | Long sessions don't blow up |
| **I** | Build/Plan agent switch alters tool whitelist + system prompt; Tab cycles agent | Spec §11 modes work |
| **J** | bwrap sandbox (Linux first) | Safer shell |
| **K** | Sub-agent `task` tool (fresh Session, derived permissions) | Parallelism + context isolation |
| **L** | MCP, LSP, web search, vector RAG | Defer |

Phases A-G are the must-haves. H-K are quality. L is research-grade.

---

## 7. The first PR shape

Single PR, ~600 lines, no behavior change for existing chat:

1. `src/providers/mod.rs` — add tool-call arms to `ModelEvent`, add `ToolCall` struct, add `tools: Vec<ToolSpec>` to `ModelRequest`. All existing providers compile because they only emit `TextDelta`/`Done` initially.
2. `src/tools/mod.rs` — `Tool` trait, `ToolSpec`, `ToolResult`, `ToolContext`.
3. `src/tools/registry.rs` — `ToolRegistry::new() -> Self`, `dispatch(call) -> ToolResult`.
4. `src/tools/read_file.rs` — first concrete `Tool`.
5. `src/agent/loop.rs` — empty `run_turn` skeleton, falls through to existing chat path until tool registry is non-empty.
6. `src/permissions/mod.rs` — scaffold; default policy reads from `AppConfig`.
7. One provider impl: OpenAI-compat parsing `tool_calls` from `delta.tool_calls[]`. (Smallest diff; Copilot Responses-API parsing comes second.)

After this PR merges, every subsequent PR can ship one tool or one capability without re-architecture.

---

## 8. Risks and open questions

- **`App` god-object migration.** Pulling `Session` and `AgentRunner` out of `App` is mechanical but touches every file in `ui/`. Suggest doing it in Phase C with a compat alias (`pub type App = Session;` during transition) so PRs stay reviewable.
- **Provider tool-call surface differences.** Anthropic's `tool_use` content blocks vs OpenAI Chat's `tool_calls` array vs Responses API's `function_call` items vs Ollama's chat format — the `ModelEvent::ToolCallEnd` arm collapses all of them, but the per-provider parser is non-trivial. Start with OpenAI-compat (simplest) and reuse for Copilot's chat-completions path.
- **V4A patch format licensing.** codex-rs is Apache-2.0, so the parser can be lifted. But it's ~2000 lines; a from-scratch port is less risky.
- **Sandbox on Fedora.** `bwrap` is in default repos but not pre-installed; document `dnf install bubblewrap` in README.
- **Compaction trigger tokenizer.** Without a tokenizer dep, char-based estimates undercount Asian languages and code. Either add `tiktoken-rs` (OpenAI) and `anthropic-tokenizer-rs` (Anthropic) or document the approximation clearly.

---

## 9. References

- `docs/spec/codex-architecture.md` — full codex-rs harness reference
- `docs/spec/opencode-architecture.md` — full opencode harness reference (was `docs/system-prompts/opencode-architecture.md`)
- `docs/spec/session-persistence.md` — SQLite session DB design
- `docs/spec/copilot-oauth.md` — Copilot device-flow + token-exchange spec
- `docs/spec/artui_v1_agentic_spec.md` — original v1 product spec
- `graphify-out/GRAPH_REPORT.md` — current-state codebase audit
- `docs/todos/` — implementation tickets per phase
