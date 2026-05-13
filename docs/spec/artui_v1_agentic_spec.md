# artui — Coding Agent TUI Spec

**Version:** v0.2 — v1 milestone cut  
**Date:** 2026-05-13  
**Target:** Rust + ratatui terminal coding agent  
**Primary platform for v1:** Linux/Fedora first, Windows/macOS compatible where practical  
**Core idea:** artui is not a chat window. It is a controlled agentic loop around deterministic tools: search, read, patch, shell, test, recover.

---

## 1. Product Summary

artui is a terminal-based coding agent built with Rust and ratatui. It lets a model work inside a repository by using explicit tools instead of relying on all code being pasted into the prompt.

The v1 product should feel closer to Claude Code, Codex CLI, OpenCode, and Gemini CLI than a normal chatbot:

1. The model searches the repo with `rg`/grep-like tools.
2. The model reads only relevant file slices.
3. The model proposes patches instead of dumping replacement files.
4. artui previews edits before applying them.
5. artui runs tests/builds only through a permission gate.
6. artui feeds command results back into the model so it can iterate.
7. The user stays in control through visible approvals, diffs, and logs.

The most important v1 principle: **the LLM decides what to inspect next, but deterministic infrastructure enforces what it is allowed to do.**

---

## 2. v1 Scope

### v1 Goals

| Goal | Description |
|---|---|
| Agentic repo exploration | Model can use `search`, `glob`, `read_file`, and `list_files` tools to understand a codebase incrementally. |
| Patch-based editing | Model edits through an `apply_patch` tool with preview, approval, rollback metadata, and result feedback. |
| Permission-gated shell | Shell commands are classified as `allow`, `ask`, or `deny`; sensitive commands require approval. |
| Verification loop | Agent can run approved tests/checks, inspect failures, patch again, and stop with a concise summary. |
| Streaming TUI | Token streaming, tool-call timeline, search results, diff preview, approval prompt, status bar. |
| Provider abstraction | Minimal provider trait supporting Ollama/local and OpenAI-compatible HTTP streaming. |
| Project trust | artui must ask before trusting project-level config files. |
| Linux/Fedora readiness | Detect `rg`, optionally detect `bwrap`, work cleanly in Fedora terminal environments. |

### v1 Non-goals

These should be deferred until v1 is stable:

- Multi-agent background workers.
- Native Anthropic/Gemini provider support.
- GitHub Copilot internal API support.
- Full MCP integration.
- LSP-driven semantic code intelligence.
- Vector database/RAG indexing.
- IDE plugins.
- Cloud execution.
- Autonomous no-approval mode.
- Complex plugin marketplace.
- Long-running background dev server management.

---

## 3. Research-Informed Design Decisions

| Decision | v1 Action |
|---|---|
| Use agentic search before RAG | Start with `rg`, glob, file reads, and tool iteration. Do not build embeddings for v1. |
| Treat shell as a dangerous capability | Wrap every command in a classifier and approval flow. Do not let the model run arbitrary command strings silently. |
| Prefer patch tools over raw shell writes | Use structured patches for create/update/delete operations. Avoid `cat > file`, `sed -i`, shell redirects, and opaque generated scripts for edits. |
| Keep the loop simple | A single agent loop is enough: model response → parse tool calls → permission check → execute → feed result back. |
| Put engineering effort into deterministic harness | Context caps, output caps, retry limits, permission rules, cwd control, patch validation, and recovery logic matter more than elaborate prompt chains. |
| Use repo instructions as soft guidance only | Files like `AGENTS.md`, `CLAUDE.md`, or `.artui/rules.md` can guide the model, but they must not override safety policy. |
| Make tool activity visible | The TUI should show every search, read, patch, shell command, success/failure, and approval decision. |

---

## 4. v1 Architecture

```text
User prompt
   │
   ▼
TUI event loop ──────► Agent loop
   │                    │
   │                    ▼
   │              Model provider
   │                    │
   │                    ▼
   │              Tool call request
   │                    │
   │                    ▼
   │              Tool router
   │                    │
   │          ┌─────────┴─────────┐
   │          ▼                   ▼
   │    Permission engine     Context manager
   │          │                   │
   │          ▼                   ▼
   │      Tool executor ◄── Tool output caps
   │          │
   │          ▼
   │   Tool result returned to model
   │
   ▼
Visible transcript, diff previews, approvals, logs
```

### Core Runtime Rule

The model never directly touches the filesystem or shell. It only emits tool calls. artui owns:

- path normalization,
- cwd boundaries,
- ignore rules,
- output truncation,
- patch application,
- approval prompts,
- command execution,
- retries,
- and final summaries.

---

## 5. Project Structure

```text
artui/
├── Cargo.toml
├── README.md
├── docs/
│   ├── v1-agent-loop.md
│   ├── permissions.md
│   └── tool-contract.md
└── src/
    ├── main.rs
    ├── app.rs
    ├── ui/
    │   ├── mod.rs
    │   ├── layout.rs
    │   ├── chat.rs
    │   ├── tools.rs
    │   ├── diff.rs
    │   └── popups.rs
    ├── agent/
    │   ├── mod.rs
    │   ├── loop.rs
    │   ├── prompts.rs
    │   ├── context.rs
    │   └── parser.rs
    ├── tools/
    │   ├── mod.rs
    │   ├── search.rs
    │   ├── glob.rs
    │   ├── read_file.rs
    │   ├── apply_patch.rs
    │   ├── shell.rs
    │   └── git.rs
    ├── permissions/
    │   ├── mod.rs
    │   ├── classifier.rs
    │   └── policy.rs
    ├── providers/
    │   ├── mod.rs
    │   ├── ollama.rs
    │   └── openai_compat.rs
    ├── config/
    │   ├── mod.rs
    │   └── schema.rs
    └── util/
        ├── paths.rs
        ├── tokens.rs
        └── output.rs
```

---

## 6. Recommended Crates

Exact versions can be pinned during implementation. Use the latest mutually compatible releases at build time.

| Crate | Purpose |
|---|---|
| `ratatui` | TUI rendering |
| `crossterm` | Cross-platform terminal input/output |
| `tokio` | Async runtime |
| `reqwest` | HTTP client and streaming |
| `serde`, `serde_json` | JSON schemas and provider payloads |
| `toml` | Config parsing |
| `directories` | Cross-platform config path discovery |
| `anyhow`, `thiserror` | Error handling |
| `async-trait` | Provider/tool traits |
| `syntect` | Syntax highlighting |
| `arboard` | Clipboard support |
| `ignore` | Gitignore-aware fallback file walking |
| `which` | Detect `rg`, `git`, `cargo`, `npm`, `bwrap` |
| `similar` | Diff rendering |
| `tempfile` | Patch/test temp files |
| `shlex` or equivalent | Shell tokenization/parsing support |
| `tracing`, `tracing-subscriber` | Debug logs |

### External Tools

| Tool | v1 Requirement | Notes |
|---|---|---|
| `rg` / ripgrep | Strongly recommended | Primary search backend. |
| `git` | Recommended | Used for file listing, status, and rollback metadata. |
| `bwrap` | Optional Linux sandbox | Fedora users can install with `sudo dnf install bubblewrap`. |

If `rg` is missing, artui should fall back to a Rust implementation using `ignore`, but show a degraded-search warning.

---

## 7. Agent Loop

### Loop Pseudocode

```rust
async fn run_agent_turn(user_input: String, app: &mut App) -> Result<()> {
    app.transcript.push(Message::user(user_input));

    for step in 0..app.config.agent.max_steps_per_turn {
        let context = context::build_model_context(app)?;
        let response = app.provider.next(context).await?;

        match response.kind {
            ModelResponseKind::Text(text) => {
                app.transcript.push(Message::assistant(text));
                return Ok(());
            }
            ModelResponseKind::ToolCall(call) => {
                let decision = permissions::classify(&call, app)?;
                let result = match decision {
                    PermissionDecision::Allow => tools::execute(call, app).await?,
                    PermissionDecision::Ask(prompt) => {
                        let approved = ui::approval_prompt(prompt).await?;
                        if approved { tools::execute(call, app).await? }
                        else { ToolResult::denied_by_user(call.id) }
                    }
                    PermissionDecision::Deny(reason) => ToolResult::denied(call.id, reason),
                };

                app.tool_log.push(result.clone());
                app.transcript.push(Message::tool_result(result.into_model_text()));
            }
        }
    }

    app.transcript.push(Message::assistant(
        "I stopped because I reached the step limit. Here is what I completed...".into(),
    ));
    Ok(())
}
```

### Step Limits

```toml
[agent]
max_steps_per_turn = 12
max_patch_retries = 2
max_shell_retries = 2
max_tool_output_chars = 30000
max_search_output_chars = 20000
max_read_file_chars = 16000
```

The agent must not enter infinite tool loops. Failed patches and failed shell commands should have small retry budgets.

---

## 8. Tool Contract

The model should receive schemas for these tools in v1.

### 8.1 `list_files`

Use for quick repo inventory.

```json
{
  "name": "list_files",
  "arguments": {
    "path": ".",
    "max_results": 500
  }
}
```

Implementation order:

1. Prefer `git ls-files` when inside a Git repo.
2. Fallback to `ignore` crate walk.
3. Respect `.gitignore`, `.ignore`, and `.artuiignore`.
4. Exclude `.git`, `node_modules`, `target`, `dist`, `build`, `.next`, `coverage`, binary files.

### 8.2 `search`

Primary retrieval tool. Wraps ripgrep.

```json
{
  "name": "search",
  "arguments": {
    "pattern": "fn build_messages|struct App|Provider",
    "path": ".",
    "case_sensitive": false,
    "file_glob": "*.rs",
    "context_lines": 2,
    "max_matches": 80
  }
}
```

Default command shape:

```bash
rg --json --line-number --column --smart-case --max-count 80 \
  --glob '!{.git,node_modules,target,dist,build,.next,coverage}' \
  '<pattern>' '<path>'
```

Safety and quality rules:

- Do not search hidden files by default.
- Do not search ignored files by default.
- Do not search binary files.
- Cap output by matches and characters.
- Convert ripgrep JSON into concise model-facing results:

```text
src/agent/context.rs:42:13
  fn build_model_context(app: &App) -> Result<ModelContext> {
```

Recommended search strategy for the model:

1. Search filenames and obvious symbols first.
2. Search exact function/type names before reading whole files.
3. Search call sites before changing a public API.
4. Use context lines for local understanding.
5. Read file slices only after search identifies relevant lines.

### 8.3 `glob`

Use for pattern-based file discovery.

```json
{
  "name": "glob",
  "arguments": {
    "pattern": "src/**/*.rs",
    "max_results": 200
  }
}
```

Rules:

- Respect ignore files.
- Never return paths outside workspace.
- Sort deterministically.

### 8.4 `read_file`

Read bounded slices of files.

```json
{
  "name": "read_file",
  "arguments": {
    "path": "src/agent/context.rs",
    "start_line": 1,
    "end_line": 160
  }
}
```

Rules:

- Must reject paths outside workspace.
- Must reject binary files.
- Must include line numbers in output.
- Must cap by lines and characters.
- If file is large, return a file outline plus a hint to search within it.

### 8.5 `apply_patch`

Use for all v1 file edits.

```json
{
  "name": "apply_patch",
  "arguments": {
    "patch": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
  }
}
```

Rules:

- Preview diff before applying.
- Require approval by default.
- Reject patches outside workspace.
- Reject patches touching ignored sensitive paths unless explicitly approved.
- Save rollback metadata:

```text
.artui/session/<session-id>/patches/<patch-id>.diff
.artui/session/<session-id>/patches/<patch-id>.before.json
```

Patch failure recovery:

1. Return exact error to model.
2. Include surrounding file lines if safe.
3. Allow at most two retry patches.
4. If still failing, ask the model to summarize the needed manual change.

### 8.6 `shell`

Run project commands through permission gates.

```json
{
  "name": "shell",
  "arguments": {
    "command": "cargo test",
    "cwd": ".",
    "timeout_ms": 120000,
    "reason": "Run the test suite after patching the parser"
  }
}
```

Rules:

- All shell commands must be visible in the TUI.
- Commands run with controlled cwd inside workspace.
- Environment variables do not persist between commands.
- Timeout defaults to 2 minutes.
- Output is capped; full output is saved in session logs.
- Commands that write files, install packages, access network, delete files, change permissions, or run elevated privileges require approval or are denied.

### 8.7 `git_status`

Dedicated safe Git status tool.

```json
{
  "name": "git_status",
  "arguments": {}
}
```

Use this instead of raw `git status` where possible. Output should include:

- current branch,
- dirty files,
- staged files,
- untracked files,
- and whether artui has applied patches in this session.

---

## 9. Command and Permission Model

### Permission Decisions

```rust
pub enum PermissionDecision {
    Allow,
    Ask(ApprovalPrompt),
    Deny(String),
}
```

### Default v1 Policy

| Action | Default | Reason |
|---|---:|---|
| `list_files`, `glob`, `search`, `read_file` inside workspace | Allow | Read-only retrieval. |
| `git status`, `git diff`, `git log`, `git show` | Allow | Read-only Git inspection. |
| `apply_patch` | Ask | Changes files. |
| `cargo check`, `cargo test`, `npm test`, `pnpm test` | Ask unless sandboxed | Can write build/cache output and execute project code. |
| `npm install`, `pnpm install`, `pip install`, `cargo install` | Ask | Network and dependency changes. |
| `curl`, `wget`, `ssh`, `scp`, `rsync` | Ask or deny by config | Network/data exfiltration risk. |
| `rm`, `mv`, `cp`, `chmod`, `chown` | Ask or deny depending target | Can destroy or alter files. |
| `sudo`, `su`, `doas` | Deny by default | Privilege escalation. |
| Commands outside workspace | Deny by default | Boundary violation. |
| Shell chains with `;`, `&&`, `||`, pipes, backticks, `$()` | Ask | Harder to classify safely. |

### Read-only Allowlist

Read-only commands can be auto-allowed only when the parsed command is simple and points inside the workspace:

```text
pwd
ls
cat
head
tail
wc
stat
du
diff
grep
rg
find   # read-only forms only; no -exec, no -delete
git status
git diff
git log
git show
git branch --show-current
```

Important classifier rule: **do not approve by string prefix alone.** Parse argv and detect wrappers/chains. For example, `npx`, `devbox run`, `docker exec`, `bash -c`, and `sh -c` must not inherit the approval of the inner command unless the full command is specifically allowed.

### Dangerous Pattern Denylist

Deny by default:

```text
rm -rf /
rm -rf .
sudo *
su *
doas *
chmod -R 777 *
chown -R *
dd if=* of=*
mkfs*
mount*
umount*
:(){ :|:& };:
curl * | sh
wget * | sh
bash -c "$(curl *)"
```

Do not rely only on string matching. Use this denylist as a final guard after tokenization.

---

## 10. Optional Linux Sandbox

v1 should implement a simple Linux sandbox path first because the user target includes Fedora.

### Sandbox Modes

```toml
[sandbox]
mode = "off"              # off | bubblewrap
workspace_write = true
network = false
allow_home_read = false
```

### Bubblewrap Behavior

When `bwrap` is available and sandbox mode is enabled:

- bind workspace as read-write,
- bind system paths read-only as needed,
- block network by default,
- block writes outside workspace,
- use temp dirs for `/tmp`,
- pass a minimal environment.

Example implementation target, not literal final command:

```bash
bwrap \
  --ro-bind /usr /usr \
  --ro-bind /bin /bin \
  --ro-bind /lib /lib \
  --ro-bind /lib64 /lib64 \
  --bind "$WORKSPACE" "$WORKSPACE" \
  --tmpfs /tmp \
  --chdir "$WORKSPACE" \
  --unshare-net \
  --die-with-parent \
  -- "$SHELL" -lc "$COMMAND"
```

If sandboxing is off, artui must ask more often. No-approval autonomous mode is not a v1 feature.

---

## 11. Context Management

### Context Assembly Order

1. System safety and tool instructions.
2. Current task from user.
3. Project guidance files, if trusted:
   - `AGENTS.md`
   - `CLAUDE.md`
   - `.artui/rules.md`
4. Recent conversation summary.
5. Tool results from current turn.
6. Relevant file slices from `read_file`.
7. Optional compact repo map summary if already generated.

### Hard Rules

- Do not inject entire repositories into context.
- Do not keep stale full files in context after edits.
- Prefer line-numbered slices.
- After each patch, invalidate cached reads for changed files.
- Summarize old conversation when token budget is near limit.
- Keep the model focused on current task, not general chat history.

### Output Caps

| Output type | v1 cap |
|---|---:|
| Search results | 20,000 chars |
| Shell output preview | 30,000 chars |
| Single file read | 16,000 chars |
| Tool log kept in model context | Last 8 results or summarized |
| Full raw logs | Stored under `.artui/session/` |

---

## 12. Search and Retrieval Best Practices

### Default Agent Retrieval Policy

The model should follow this workflow before editing:

```text
1. list_files or glob to understand structure.
2. search for exact symbols, filenames, route names, config keys, or error messages.
3. read_file around the highest-value matches.
4. search call sites before changing function signatures or exports.
5. patch the smallest safe set of files.
6. run targeted verification.
7. inspect failures through search/read, then retry.
```

### Recommended `rg` Patterns

| Need | Pattern |
|---|---|
| Find a Rust function | `rg "fn function_name|pub fn function_name" src --type rust` |
| Find struct/enum | `rg "struct App|enum Provider" src --type rust` |
| Find call sites | `rg "function_name\\(" src` |
| Find routes | `rg "route|Router|GET|POST|handler" src` |
| Find config keys | `rg "default_provider|sandbox_mode|approval_policy" .` |
| Find TODOs | `rg "TODO|FIXME|HACK" .` |
| Find errors from compiler output | `rg "ExactErrorType|missing_field|trait bound" src tests` |

### What artui Should Avoid

- Avoid reading whole files when search slices are enough.
- Avoid hidden/ignored files unless user asks.
- Avoid `rg -uuu` by default.
- Avoid dumping `node_modules`, `target`, lockfiles, generated files, or minified files into context.
- Avoid semantic/vector search until the basic loop is stable.

---

## 13. TUI Layout

Three-column layout remains, but v1 adds a tool timeline and approval popup.

```text
┌───────────────┬────────────────────────────────────┬─────────────────┐
│ File / Search │ Chat + Tool Timeline               │ Session Info    │
│               │                                    │                 │
│ src/          │ User: fix parser error             │ Provider        │
│ ├ main.rs     │                                    │ Model           │
│ ├ agent.rs    │ Tool: search "ParserError" ✓       │ Context budget  │
│ └ tools.rs    │ Tool: read_file src/parser.rs ✓     │ Step 4/12       │
│               │ Tool: apply_patch pending approval │ Sandbox: off    │
│ Search hits   │                                    │ Git: dirty      │
│ parser.rs:42  │ ┌──── Diff Preview ──────────────┐ │                 │
│ parser.rs:88  │ │ - old line                     │ │ Permissions    │
│               │ │ + new line                     │ │ next: ask      │
│               │ └────────────────────────────────┘ │                 │
├───────────────┴────────────────────────────────────┴─────────────────┤
│ Input: > _                                                            │
│ [Tab] Focus [Enter] Send [/] Command [a] Approve [d] Deny [q] Quit     │
└───────────────────────────────────────────────────────────────────────┘
```

### TUI States

```rust
pub enum UiMode {
    Normal,
    Input,
    Streaming,
    ToolRunning,
    ApprovalPending,
    DiffPreview,
    ModelPicker,
    ProviderPicker,
    Help,
}
```

### Keybindings

| Key | Action |
|---|---|
| `Tab` | Cycle focus |
| `Enter` | Send input / confirm focused action |
| `/` | Open command palette |
| `Esc` | Cancel streaming or close popup |
| `a` | Approve pending tool action |
| `d` | Deny pending tool action |
| `v` | View full diff/tool output |
| `c` | Copy last assistant response |
| `Ctrl+L` | Clear visible transcript after confirmation |
| `q` | Quit |

---

## 14. Provider Abstraction

### v1 Providers

| Provider | v1 Status | Notes |
|---|---|---|
| Ollama | Required | Local-first default. |
| OpenAI-compatible HTTP | Required | Covers OpenAI-compatible APIs with SSE-style streaming. |
| NVIDIA NIM | Optional profile | Can use OpenAI-compatible implementation if configured. |
| Anthropic Claude | Backlog | Native schema differs; defer. |
| Google Gemini | Backlog | Native schema differs; defer. |
| GitHub Copilot | Backlog | Avoid relying on internal/unstable APIs in v1. |

### Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;

    async fn stream_turn(
        &self,
        request: ModelRequest,
        tx: tokio::sync::mpsc::Sender<ModelEvent>,
    ) -> Result<()>;
}
```

### Model Events

```rust
pub enum ModelEvent {
    Token(String),
    ToolCall(ToolCall),
    Done,
    Error(String),
}
```

---

## 15. Config

### Global Config

Path:

```text
~/.config/artui/config.toml
```

Example:

```toml
default_provider = "ollama"
trusted_projects = []

[agent]
max_steps_per_turn = 12
max_patch_retries = 2
max_shell_retries = 2
max_tool_output_chars = 30000
max_search_output_chars = 20000
max_read_file_chars = 16000

[providers.ollama]
host = "http://localhost:11434"
default_model = "qwen2.5-coder:7b"

[providers.openai_compat]
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
default_model = "gpt-4o-mini"

[permissions]
default = "ask"
read_only = "allow"
apply_patch = "ask"
shell = "ask"
network = "ask"
privilege_escalation = "deny"
outside_workspace = "deny"

[sandbox]
mode = "off"
workspace_write = true
network = false
allow_home_read = false
```

### Project Config

Path:

```text
.artui/config.toml
```

Project config is ignored until the user trusts the project.

Allowed project-level overrides in v1:

- default model,
- safe test commands,
- ignored directories,
- repo guidance file path,
- sandbox preference.

Project config must not be allowed to weaken global safety by default. For example, it cannot silently set `outside_workspace = "allow"`.

---

## 16. Recovery Logic

### Failed Search

If search returns no results:

1. Try alternate case/symbol spelling.
2. Search filenames with `glob`.
3. Search broader terms.
4. Ask user only after reasonable attempts.

### Failed Patch

If patch fails:

1. Read the affected file slice.
2. Recalculate hunk context.
3. Retry once or twice.
4. If still failing, stop and summarize.

### Failed Test/Build

If command fails:

1. Show concise error preview.
2. Search exact error names or file paths.
3. Read related files.
4. Patch only if the cause is clear.
5. Stop after retry budget.

### Large Output

When output exceeds cap:

```text
Tool output exceeded 30000 chars.
Preview shown below.
Full output saved to:
.artui/session/<session-id>/tool-output/<tool-id>.txt
```

The model can then search/read the saved output through tools if needed.

---

## 17. v1 Milestones

### Milestone 0 — TUI Skeleton and Config

Deliverables:

- ratatui layout,
- input box,
- streamed assistant text,
- config loading,
- provider picker,
- Ollama provider,
- OpenAI-compatible provider skeleton.

Acceptance:

- User can chat with a local Ollama model.
- UI remains responsive during streaming.
- Config loads from `~/.config/artui/config.toml`.

### Milestone 1 — Repo Read/Search Tools

Deliverables:

- `list_files`,
- `glob`,
- `search`,
- `read_file`,
- ignore handling,
- output caps,
- visible tool timeline.

Acceptance:

- Given a repo question, model can search and read files without manual file loading.
- `rg` is used when available.
- Search does not enter ignored/generated directories by default.
- Tool outputs are line-numbered and capped.

### Milestone 2 — Agent Loop and Tool Router

Deliverables:

- model tool-call parser,
- single agent loop,
- max-step limit,
- tool result feedback,
- transcript model context builder.

Acceptance:

- Model can search → read → answer in one turn.
- Tool failures are returned to the model clearly.
- Agent stops gracefully at step limit.

### Milestone 3 — Patch Editing and Diff Approval

Deliverables:

- `apply_patch`,
- diff preview popup,
- approve/deny flow,
- patch logs,
- cache invalidation for modified files.

Acceptance:

- Model can propose a multi-file patch.
- User sees a diff before applying.
- Denied patches do not modify files.
- Failed patches can be retried with bounded recovery.

### Milestone 4 — Shell Verification and Permissions

Deliverables:

- `shell` tool,
- permission classifier,
- read-only allowlist,
- ask/deny rules,
- command timeout,
- shell output caps,
- optional Linux `bwrap` sandbox detection.

Acceptance:

- Read-only commands can run with no approval when safe.
- `cargo test`/`npm test` asks unless sandbox policy allows it.
- Dangerous commands are denied.
- User can see the exact command and reason before approving.

### Milestone 5 — v1 Hardening and Release

Deliverables:

- session logs,
- crash-safe terminal restore,
- integration tests,
- permission classifier tests,
- README quickstart,
- Fedora/Linux setup docs,
- example `AGENTS.md`,
- v1 demo workflow.

Acceptance:

- artui can perform a small bugfix workflow: inspect repo, patch file, run test, fix failure, summarize changes.
- Terminal restores correctly after panic or Ctrl+C.
- Permission tests cover wrappers/chains/dangerous commands.
- v1 docs explain what artui can and cannot do.

---

## 18. Example v1 Workflow

User:

```text
Fix the parser panic when config.toml is missing the providers table.
```

Expected agent behavior:

```text
1. search "providers" in src/**/*.rs
2. read_file src/config/schema.rs around matching lines
3. read_file tests/config_tests.rs if found
4. propose apply_patch for graceful default/diagnostic
5. user approves diff
6. shell "cargo test config" asks approval
7. user approves
8. if test fails, search exact error and patch again
9. final summary:
   - files changed
   - tests run
   - any remaining risks
```

---

## 19. Testing Plan

### Unit Tests

- path normalization rejects `../` escapes,
- ignore rules skip generated directories,
- command classifier catches chains/wrappers,
- dangerous denylist works,
- read-only allowlist does not allow write-capable flags,
- patch parser rejects outside-workspace paths,
- output cap logic stores full logs.

### Integration Tests

- small Rust fixture repo,
- small Node fixture repo,
- no-`rg` fallback mode,
- patch approval accepted,
- patch approval denied,
- shell command timeout,
- terminal restore on panic.

### Manual Demo Test

```text
cargo run -- ~/dev/sample-rust-project
> Find why the config parser fails on empty config and fix it.
```

Expected result:

- visible search/read/patch/test timeline,
- no hidden shell execution,
- approved diff only,
- final concise summary.

---

## 20. Implementation Priorities

Build in this order:

1. TUI streaming chat.
2. Provider abstraction.
3. `search` and `read_file` tools.
4. Agent loop with tool result feedback.
5. Permission engine.
6. `apply_patch` with diff approval.
7. `shell` with approval and output caps.
8. Recovery logic.
9. Optional `bwrap` sandbox.
10. Packaging/docs.

Do not start with providers, MCP, or subagents. The product becomes useful when the local agent loop works.

---

## 21. Reference Notes

The following public references informed this v1 cut:

- Claude Code permissions document read-only command behavior and warn that command-pattern permissions are fragile around wrappers and argument variation.
- Claude Code tool docs describe Bash cwd behavior, output caps, read-before-edit checks, and exact edit matching.
- OpenAI Codex docs describe sandbox modes, approval policies, workspace-write defaults, and config layering.
- OpenAI apply_patch docs describe structured patch operations and returning patch results back to the model.
- OpenCode docs expose built-in tools like bash/edit/write/read/grep/glob/apply_patch and permission values allow/ask/deny.
- Gemini CLI docs describe a ReAct loop with tool requests, user confirmation for sensitive operations, and sandboxing.
- ripgrep documentation confirms code-search defaults: recursive search, gitignore awareness, and skipping hidden/binary files by default.
- Aider's repo map is useful inspiration, but it is deferred because v1 should first prove the simpler search/read loop.
