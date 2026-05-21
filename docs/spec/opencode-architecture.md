# OpenCode Architecture Reference
> A breakdown of how OpenCode works — for reference while building artui.

---

## Core Architecture

### 1. Client/Server Split
OpenCode uses a **client/server architecture**. The TUI frontend is just one possible client — the server can run independently and be driven remotely (e.g. from a mobile app). This decoupling makes the tool flexible and extensible without rewriting the core.

### 2. Event-Driven Core
At the heart is a **strongly-typed event bus**. Every action — file changes, permission requests, tool results — flows through it.

```ts
// When a session updates, everyone who cares knows about it
Bus.publish(Event.SessionUpdated, sessionData)

// Tools can react to file changes
Bus.subscribe(Event.FileChanged, (data) => {
  // Update diagnostics, refresh LSP, etc.
})
```

This is what enables tight feedback loops. Example: the LLM edits a file → LSP client fires `textDocument/didChange` → diagnostics come back → fed back into the LLM's context — all automatically via the event bus.

### 3. Provider-Agnostic Model Abstraction
Models are abstracted behind a provider interface — automatic cost calculation, token limits, and auth are all handled at that layer. Switching between Anthropic, OpenAI, Gemini, etc. requires no core changes.

---

## The Tool Layer

Tools are what turn an LLM into an **actor** in your system. The LLM is the brain; tools are the arms.

Every tool is defined with `Tool.define()`, which wraps the execute function with:
- **Zod argument validation**
- **Automatic output truncation** (2000 lines / 50KB cap)
- When output is truncated, the full content goes to a temp file and the model is told to use `grep`/`read` or delegate to an Explore subagent

### Built-in Tool Categories

| Tool | Description |
|---|---|
| `read` | Reads files; warms up LSP in background |
| `edit` | String-replacement edits with file locking to prevent race conditions |
| `write` | Full file writes; collects LSP diagnostics after |
| `bash` | Shell execution; uses tree-sitter to parse commands for granular permissions |
| `grep` | Pattern search across codebase (backed by ripgrep) |
| `glob` | File pattern matching (also backed by ripgrep) |
| `list` | Directory listing |
| `task` | Spawns a child subagent session for parallelized work |
| `lsp` | Symbol-level code intelligence (definitions, references, call hierarchy) |
| `todo` | Session-scoped task list the LLM manages itself |
| `skill` | Loads a specialized instruction set into the agent's context |
| `apply_patch` | Applies unified diffs to files (used by GPT models instead of `edit`) |

---

## Search: grep is Load-Bearing Infrastructure

The industry has converged on a **layered retrieval** pattern — not a single search mechanism.

### Layer 1: ripgrep (grep/glob tools)
- **Always available** — unconditional, no setup required
- **Broad-coverage, low-cost** exploratory search
- Respects `.gitignore` by default
- OpenCode bundles ripgrep as a **product-level dependency**, not an optional one
- The Anthropic team experimented with vector DBs and recursive model-based indexing for Claude Code — **plain glob + grep won**

### Layer 2: LSP (Language Server Protocol)
- **Conditionally available** — only if an LSP server is configured for that file type
- **High-precision, symbol-level** lookups: `goToDefinition`, `findReferences`, `hover`, `documentSymbol`, `incomingCalls`, `outgoingCalls`
- The model is explicitly told: *"grep is your default; LSP is available when conditions are met"*
- In practice, trigger rate is low — agents find what they need through grep most of the time

> **Pattern:** grep handles exploration. LSP handles confirmation.

---

## Multi-Agent Layer

OpenCode has two tiers of agents.

### Primary Agents (user-facing, switchable with Tab)

| Agent | Access | Purpose |
|---|---|---|
| **Build** | All tools | Default for active development |
| **Plan** | Read-only (asks before anything) | Analysis and planning without making changes |

### Subagents (invoked by primary agents or via `@mention`)

| Subagent | Access | Purpose |
|---|---|---|
| **General** | Full (except todo) | Parallel multi-step tasks |
| **Explore** | Read-only | Fast codebase navigation (grep/glob/read) |
| **Scout** | Read-only | External docs and dependency research |
| **Compaction** *(hidden)* | Internal | Auto-summarizes long contexts to manage token budget |
| **Title** *(hidden)* | Internal | Generates short session titles |

The `task` tool creates a child session with inherited (but restricted) permissions — no todo tools, no recursive task spawning unless explicitly allowed.

---

## Prompt Assembly Pipeline

Each agent loop iteration assembles the system prompt from four collaborating modules:

1. **`prompt.ts`** — orchestrator; runs the agentic loop
2. **`system.ts`** — injects environment block (model name, working directory, platform, date) and selects a provider-specific prompt file
3. **`instruction.ts`** — walks the filesystem for `AGENTS.md` / `CLAUDE.md` files and fetches URL-based instructions from config
4. **`llm.ts`** — assembles the final system message array and calls `streamText()`

Provider-specific prompts are matched by model ID string:
- Claude → `anthropic.txt`
- GPT/o1/o3 → `beast.txt`
- Gemini → `gemini.txt`

---

## Context Window Management

Every coding agent has to solve the same fundamental problem: **the context window is finite, but sessions aren't.** Here's how the production agents handle it.

### The Token Budget Model

Claude Code reserves a fixed **autocompact buffer** — roughly 16.5% of the context window — that's never used for conversation history. For a 200K context model, compaction fires at ~167K tokens, leaving 33K reserved for the summarization process itself.

```
claude-opus-4-5 · 76k/200k tokens (38%)
  System prompt:   2.7k tokens  (1.3%)
  System tools:   16.8k tokens  (8.4%)
  Custom agents:   1.3k tokens  (0.7%)
  Memory files:    7.4k tokens  (3.7%)
  Skills:          1.0k tokens  (0.5%)
  Messages:        9.6k tokens  (4.8%)
  Free space:      118k         (58.9%)
  Autocompact buf: 33.0k        (16.5%)  ← never touched by history
```

**Key insight:** Set your internal limit *below* the API's hard ceiling. Leave a buffer for compaction to run before the context overflows. The threshold for Claude Code is ~83.5% by default (`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` lets you tune it).

### How Compaction Works

Compaction is a **lossy compression** — it replaces the full conversation history with a model-generated summary. Claude Code triggers it automatically; OpenCode runs it through a hidden `compaction` agent with all normal tools denied.

The process:
1. Monitor running token count from API response metadata each turn
2. When `free_space <= autocompact_buffer`, halt the normal agent loop
3. Ask the model (or a dedicated compaction agent) to generate a summary of the conversation so far — key decisions, current state, open tasks, constraints
4. Replace `messages[]` with: `[{ role: "user", content: summary }]`
5. Resume the agent loop with the fresh (compacted) context

**What to preserve in a summary:**
- The original task/goal (verbatim if possible — or keep it in a non-compactable system prompt block)
- File paths modified and current state of those files
- Decisions made and why
- Open items / what's next
- Any constraints or requirements stated by the user

**What you lose:**
- Exact variable names from early turns
- Specific error messages from prior debugging
- Nuanced intermediate reasoning steps

**Mitigation strategies:**
- Pin the task specification to the system prompt, not the message history — system prompts survive compaction
- Persist critical state to disk (files, a session notes file) before compaction fires
- For sessions that can't afford lossy summaries (audit trails, legal review), use chunked processing instead of compaction
- Proactively compact before hitting the limit for cleaner summaries — don't wait for auto-trigger

### Prompt Caching

Both Claude Code and Codex aggressively use **prompt caching** to avoid re-paying for the system prompt on every turn.

Claude Code restructures the system message array specifically for Anthropic's cache: if the first element survived plugin transforms unchanged, the rest is joined into a single string to maintain a cacheable 2-part structure. This means the ~20K token system prompt (tools + instructions + environment) is only billed once per session after the first turn.

For artui: mark your system prompt and tool definitions with cache-control headers if your provider supports it. Input tokens are billed on every request regardless of how much new content you're adding — caching the static parts is free money.

### Subagent Context Isolation

One of the biggest wins in OpenCode's architecture: **subagents run in isolated context windows**. When the `task` tool spawns a subagent, it creates a child session with its own fresh context — the parent's bloated history doesn't contaminate it.

This is why you'd use a Scout agent to research external docs rather than doing it inline: the research context stays in the subagent's window and only the result (a summary) comes back to the parent. This keeps the primary agent's context lean.

```
Parent Agent  →  task("research X") →  Subagent (clean context)
                                            ↓ runs its own agentic loop
                                            ↓ searches, reads, explores
Parent Agent  ←  summary result    ←  Subagent (done, context discarded)
```

---

## The Universal System Prompt

Claude Code doesn't have a single static system prompt string — it **dynamically assembles** 110+ prompt fragments based on model, agent, session state, and plugins. But across Claude Code, Codex, and OpenCode, there's a shared set of *universal principles* that every coding agent system prompt covers.

### The Core Sections (in assembly order)

#### 1. Identity and Role
```
You are an interactive CLI coding agent. You help users with software
engineering tasks by reading codebases, making changes, running commands,
and iterating on failures.
```
Establishes the agent as an *actor*, not a chat assistant. Critical framing.

#### 2. Environment Block (dynamic, injected each turn)
```
Model: claude-opus-4-5
Working directory: /home/user/myproject
Platform: linux
Date: 2026-05-15
Shell: bash
```
OpenCode's `system.ts` generates this block fresh on every iteration. The agent always knows where it is.

#### 3. Output and Formatting Rules
```
All text output outside of tool use is displayed to the user.
Use GitHub-flavored Markdown. Output is rendered in a monospace font.
Be concise — don't repeat yourself. Don't confirm actions with filler phrases.
Don't add unsolicited explanations after completing a task.
```
Claude Code explicitly forbids output verbosity. Codex's prompt bakes in the same principle.

#### 4. Coding Philosophy (the anti-over-engineering rules)
This is the most important section for agent quality. Both Claude Code and Codex share these principles almost verbatim:

```
- Read before editing. Understand before changing.
- Make minimal changes. Don't refactor surrounding code unless asked.
- Don't add features beyond what was asked.
- Don't introduce security vulnerabilities.
- A bug fix doesn't need surrounding code cleaned up.
- A simple feature doesn't need extra configurability.
- Prefer existing patterns in the codebase over new ones.
```

#### 5. Tool Use Rules
```
- When searching for files, prefer rg/ripgrep since it's faster than grep.
- Use the read tool (not cat/head via bash) for reading files.
- When bash output is truncated, use grep/read to get specific sections.
- Don't run interactive commands that require user input mid-task.
- Prefer non-destructive operations first; confirm before destructive ones.
```
Codex bakes ripgrep preference *directly into the system prompt*. Claude Code does it via tool descriptions.

#### 6. Safety and Permission Rules
```
- Permission gates: allow / ask / deny per tool
- External directory access requires explicit permission
- Never read .env files without permission
- No doom loops (recursive self-invocation without limit)
- Destructive bash commands (rm -rf, git reset --hard) require confirmation
```

#### 7. Autonomy and Persistence (critical for agentic loops)
```
- Do not give up when encountering errors. Retry with a different approach.
- If a command fails, read the error, understand it, and adjust.
- Complete the task fully before reporting back to the user.
- Only ask the user a question if you are genuinely blocked.
```
This is what separates an *agent* from a chatbot. The system prompt explicitly tells the model to keep going.

#### 8. Memory / Instruction Files
```
- Read AGENTS.md / CLAUDE.md / CONTEXT.md at the start of every session.
- These files contain project-specific context, coding standards, and known pitfalls.
- Sub-directory instruction files are loaded when files in that directory are touched.
```
OpenCode's `instruction.ts` walks the filesystem at session start *and* during tool execution. This means instruction files can be scoped to specific modules.

#### 9. Context / Compaction Rules (injected near context limit)
When the context approaches the limit, a system reminder is injected:
```
Your context window is getting long. Wrap up what you're doing,
summarize progress, and stop — don't start new work.
```
This prevents the agent from starting a new subtask right before it runs out of context.

### Claude Code vs. Codex: Key Differences in System Prompt

| Aspect | Claude Code | Codex |
|---|---|---|
| **Prompt format** | 110+ dynamic fragments assembled per turn | Separate `instructions` field via Responses API |
| **Tool routing** | Tool descriptions tell the model how/when to use each tool | Core prompt section covers tool use philosophy |
| **ripgrep preference** | In tool descriptions | Baked into core prompt: *"prefer rg since rg is faster than grep"* |
| **Mode fragments** | `plan.txt`, `build-switch.txt` injected into message array | Equivalent via separate session state |
| **Instruction files** | `AGENTS.md` / `CLAUDE.md`, scoped per directory | `AGENTS.md`, project-wide |
| **Compaction** | Hidden compaction agent with its own prompt | Automatic context window management via Responses API |
| **Provider prompt** | Per-provider `.txt` files (anthropic.txt, gemini.txt, beast.txt) | Single standard Codex-Max prompt as the base |

### Minimal Viable System Prompt Template for artui

Based on everything above, here's the skeleton of what artui's agent system prompt should cover:

```
You are artui, an interactive AI coding agent for the terminal.
You help users with software engineering tasks by reading codebases,
making changes across files, running commands, and iterating on failures.

## Environment
Model: {model}
Working directory: {cwd}
Platform: {platform}
Date: {date}
Shell: {shell}

## Output Style
- All output outside tool calls is shown directly to the user
- Use Markdown for formatting; output renders in a monospace terminal font
- Be concise. Don't repeat yourself. Don't add filler confirmations.
- Don't explain what you just did unless asked.

## Coding Philosophy
- Read before editing. Understand the codebase before making changes.
- Make the minimal change required. Don't refactor surrounding code.
- Don't add features beyond what was asked.
- Prefer existing patterns over introducing new ones.
- Don't introduce security vulnerabilities.

## Tool Use
- Use grep/ripgrep for file search — prefer rg over grep
- Use the read tool for reading files, not cat via bash
- When bash output is truncated, use grep or read to get specific sections
- Don't run interactive commands that block waiting for input

## Safety
- Confirm before destructive operations (rm, git reset --hard, etc.)
- Never read .env or secrets files without explicit permission
- Ask when you are genuinely blocked — not as a default response

## Autonomy
- Don't give up when you hit errors. Read the error and try a different approach.
- Complete tasks fully before reporting back.
- Only ask the user a question if you are genuinely blocked and cannot proceed.

## Project Instructions
{contents of AGENTS.md / artui.md if present}
```

---

## Key Design Principles to Steal for artui

| Principle | Why it matters |
|---|---|
| **Event bus for all state changes** | Keeps tool results, UI updates, and LSP diagnostics decoupled |
| **ripgrep as a first-class dependency** | Bundle it; don't assume it exists. Treat it like stdlib. |
| **Tool output truncation with fallback** | Prevents context blowout; redirect to file + tell the model how to get the rest |
| **Permission granularity per tool** | `allow` / `ask` / `deny` per tool, with glob patterns for fine-grained control |
| **Subagent delegation via `task`** | Parallelism without shared state headaches |
| **Context compaction as a hidden agent** | Automatic; runs when token budget is near limit — use a dedicated compaction agent, not inline logic |
| **Reserve an autocompact buffer** | Never let history fill 100% of the window — reserve ~15-20% for the compaction pass itself |
| **Prompt caching for static content** | System prompt + tool definitions are re-sent every turn; cache them to avoid re-billing 20K tokens each time |
| **Subagent context isolation** | Research/exploration tasks go in subagents — only the summary returns to the parent context |
| **System prompt as stable anchor** | Pin the task spec and constraints to the system prompt, not message history — it survives compaction |
| **Provider abstraction from day one** | Makes swapping models or adding Ollama/local endpoints trivial later |

---

## Concrete file:line audit (2026-05-21)

`git clone --depth 1 https://github.com/sst/opencode /tmp/opencode`. The "harness" lives in `packages/opencode/src/`. Effect-TS / Bun service driving the Vercel AI SDK, exposed over HTTP API to a separate Go TUI binary.

Headline architectural decisions:

1. **No hand-rolled agent loop around `streamText`.** AI SDK owns one model turn (text + tool-calls + tool-results within a step). The opencode "loop" is an outer `while(true)` that re-invokes `streamText` whenever the previous turn finished with tool calls outstanding.
2. **Tools are AI SDK `tool()` objects.** Vercel AI SDK schema: `description`, `inputSchema` JSON-Schema, `execute(args, opts)`. Permissions and audit are layered around `execute` via Effect.
3. **Effect-TS Layers + Context.Services replace dependency injection.** Almost every subsystem (Bus, Permission, ToolRegistry, Provider, Session, Plugin, Storage) is an `Effect.Service`.
4. **State is durable in SQLite via Drizzle** (`storage/db.ts`).
5. **Server↔TUI is one HTTP API + one SSE stream** (`/event`).

### Outer loop

```ts
// session/prompt.ts:1240
const runLoop = Effect.fn("SessionPrompt.run")(function* (sessionID) {
  let step = 0
  const session = yield* sessions.get(sessionID).pipe(Effect.orDie)
  while (true) {
    yield* status.set(sessionID, { type: "busy" })
    let msgs = yield* MessageV2.filterCompactedEffect(sessionID)
    const { user: lastUser, assistant: lastAssistant, finished: lastFinished, tasks } = MessageV2.latest(msgs)

    const hasToolCalls = lastAssistantMsg?.parts.some(
      p => p.type === "tool" && !p.metadata?.providerExecuted) ?? false

    if (lastAssistant?.finish && !["tool-calls"].includes(lastAssistant.finish)
        && !hasToolCalls && lastUser.id < lastAssistant.id) break

    step++
    const agent = yield* agents.get(lastUser.agent)
    const maxSteps = agent.steps ?? Infinity
    const isLastStep = step >= maxSteps
    const result = yield* handle.process({
      user: lastUser, agent, sessionID, system,
      messages: [...modelMsgs, ...(isLastStep ? [{ role: "assistant", content: MAX_STEPS }] : [])],
      tools, model, ...
    })
    if (result === "stop") return "break"
    if (result === "compact") yield* compaction.create({...})
  }
})
```

Properties:

- One iteration ≈ one provider request. Loop exits when finish is anything but `tool-calls`/`unknown` and there are no unhandled tool calls.
- Cancellation: `SessionRunState.layer` (`session/run-state.ts:38`) holds `Map<SessionID, Runner>`; `cancel(sessionID)` aborts the runner's fiber.
- Doom-loop detection in `session/processor.ts:425-449`. If last `DOOM_LOOP_THRESHOLD` parts are identical tool calls, raises `permission.ask("doom_loop", [toolName])`.

Single-turn execution delegated to `streamText` in `session/llm.ts:272`:

```ts
result: streamText({
  async experimental_repairToolCall(failed) { /* lowercase tool name fix, else fall back to "invalid" tool */ },
  temperature, topP, topK, maxOutputTokens,
  providerOptions: ProviderTransform.providerOptions(input.model, prepared.params.options),
  activeTools: Object.keys(prepared.tools).filter(x => x !== "invalid"),
  tools: prepared.tools,
  toolChoice: input.toolChoice,
  abortSignal: input.abort,
  maxRetries: input.retries ?? 0,
  messages: prepared.messages,
  model: wrapLanguageModel({ model: language, middleware: [{ /* prompt transforms */ }] }),
})
```

### Tool definition contract

```ts
// tool/tool.ts
export type Context<M extends Metadata = Metadata> = {
  sessionID: SessionID
  messageID: MessageID
  agent: string
  abort: AbortSignal
  callID?: string
  extra?: { [key: string]: unknown }
  messages: MessageV2.WithParts[]
  metadata(input: { title?: string; metadata?: M }): Effect.Effect<void>
  ask(input: Omit<Permission.AskInput, "sessionID"|"id"|"ruleset"|"tool">): Effect.Effect<void, Error>
}
```

`Tool.define(id, init)` returns a factory. `wrap()` (`tool/tool.ts:155`) compiles a Schema decoder once per tool init so every LLM tool call goes through `decodeUnknownEffect(args)` and raises `InvalidArgumentsError` on mismatch — that error has a model-facing `.message` getter.

### Tool registry

```ts
// tool/registry.ts ~290
const tool = yield* Effect.all({
  invalid: Tool.init(invalid), shell: Tool.init(shell), read: Tool.init(read),
  glob: Tool.init(globtool), grep: Tool.init(greptool), edit: Tool.init(edit),
  write: Tool.init(writetool), task: Tool.init(task), task_status: Tool.init(taskStatus),
  fetch: Tool.init(webfetch), todo: Tool.init(todo), search: Tool.init(websearch),
  repo_clone: Tool.init(repoClone), repo_overview: Tool.init(repoOverview),
  skill: Tool.init(skilltool), patch: Tool.init(patchtool),
  question: Tool.init(question), lsp: Tool.init(lsptool), plan: Tool.init(plan),
})
```

### Provider abstraction

`packages/opencode/src/provider/provider.ts` (1846 lines) lazily imports per provider:

```ts
"@ai-sdk/anthropic": () => import("@ai-sdk/anthropic").then(m => m.createAnthropic),
"@ai-sdk/google":    () => import("@ai-sdk/google").then(m => m.createGoogleGenerativeAI),
"@ai-sdk/openai":    () => import("@ai-sdk/openai").then(m => m.createOpenAI),
"@ai-sdk/openai-compatible": () => import("@ai-sdk/openai-compatible").then(m => m.createOpenAICompatible),
"@openrouter/ai-sdk-provider": () => import("@openrouter/ai-sdk-provider").then(m => m.createOpenRouter),
"@ai-sdk/github-copilot": () => import("@opencode-ai/core/github-copilot/copilot-provider")
                                   .then(m => m.createOpenaiCompatible),
```

Per-provider JSON-Schema munging in `provider/transform.ts` (1376 lines) — strips `$defs`, removes `exclusiveMaximum: bool`, etc.

### Session/state model

State durable in SQLite:

- `packages/opencode/src/storage/db.ts:33` — DB at `~/.local/share/opencode/opencode.db` (or per-workspace `opencode-<hash>.db`).
- `db.bun.ts` / `db.node.ts` — runtime-specific drivers.
- Sessions: `session/session.sql.ts` (`PermissionTable`, etc.); messages live as `MessageV2` rows + parts, paged via `MessageV2.page` (`session/session.ts:768`).

Replay rebuilds via `MessageV2.toModelMessagesEffect(msgs, model)` (`session/prompt.ts:1424`) before next `streamText`.

A `Snapshot` system tracks workspace as content tree at each `step-start`, emits a `patch` part with file diffs at `step-finish` (`session/processor.ts:580-602`).

### Prompt construction

```ts
// session/system.ts
export function provider(model) {
  if (id.includes("gpt-4") || id.includes("o1") || id.includes("o3")) return [PROMPT_BEAST]
  if (id.includes("gpt"))    return id.includes("codex") ? [PROMPT_CODEX] : [PROMPT_GPT]
  if (id.includes("gemini-")) return [PROMPT_GEMINI]
  if (id.includes("claude"))  return [PROMPT_ANTHROPIC]
  if (id.toLowerCase().includes("trinity")) return [PROMPT_TRINITY]
  if (id.toLowerCase().includes("kimi"))    return [PROMPT_KIMI]
  return [PROMPT_DEFAULT]
}
```

`environment(model)` injects working directory, worktree root, git-vcs flag, platform, date.

Final assembly (`session/prompt.ts:1420-1428`):

```ts
const [skills, env, instructions, modelMsgs] = yield* Effect.all([
  sys.skills(agent), sys.environment(model),
  instruction.system().pipe(Effect.orDie),
  MessageV2.toModelMessagesEffect(msgs, model),
])
const system = [...env, ...instructions, ...(skills ? [skills] : [])]
```

### Permissions

`packages/opencode/src/permission/index.ts` is the single point of approval. A `Rule = { permission, pattern, action: "allow"|"ask"|"deny" }` is wildcard-matched against tool name + a tool-specified pattern:

```ts
for (const pattern of request.patterns) {
  const rule = evaluate(request.permission, pattern, ruleset, approved)
  if (rule.action === "deny") return yield* new DeniedError({ ruleset: ... })
  if (rule.action === "allow") continue
  needsAsk = true
}
if (!needsAsk) return
const id = PermissionID.ascending()
pending.set(id, { info, deferred })
yield* bus.publish(Event.Asked, info)
return yield* Deferred.await(deferred)
```

Agent-level permissions in `agent/agent.ts:120-156`:

```ts
const defaults = Permission.fromConfig({
  "*": "allow", doom_loop: "ask",
  external_directory: { "*": "ask", ...whitelistedDirs.map(d => [d, "allow"]) },
  question: "deny", plan_enter: "deny", plan_exit: "deny",
  repo_clone: "deny", repo_overview: "deny",
  read: { "*": "allow", "*.env": "ask", "*.env.*": "ask", "*.env.example": "allow" },
})
```

### Diff / patch primitives

`tool/edit.ts` is canonical. Per-file semaphore (`Semaphore.makeUnsafe(1)`) and multi-stage replace:

```ts
export function replace(content, oldString, newString, replaceAll = false) {
  if (oldString === newString) throw new Error("No changes...")
  for (const replacer of [
    /* exact, normalize-line-endings, normalize-whitespace, escaped-chars,
       single-line-trim, words-as-regex, ... */
  ]) {
    for (const search of replacer(content, oldString)) {
      if (replaceAll) return content.replaceAll(search, newString)
      // single occurrence: assert uniqueness, replace
    }
  }
  throw new Error("could not match")
}
```

Diff output uses `diff`'s `createTwoFilesPatch`. Origin: `tool/edit.ts:1-4` cites `cline/cline` and Google `gemini-cli` editCorrector.

### Sub-agents / Task tool

`tool/task.ts` (345 lines):

```ts
export const Parameters = Schema.Struct({
  description: "A short (3-5 words) description",
  prompt: "The task for the agent to perform",
  subagent_type: "The type of specialized agent to use",
  task_id: "optional, resume previous task",
  command: "optional, the command that triggered this task",
  background: "When true, launch async and return immediately",
})
```

A subagent is just a fresh session running the same loop. Background mode uses `BackgroundJob.Service` and `task_status` polling.

### Modes & agents

Built-ins (`agent/agent.ts:129-280`):

- `build` (default, all-allow except `doom_loop: ask`)
- `plan` (denies edits, allows `plan_enter`/`plan_exit`/writes only into `.opencode/plans/*.md`)
- `general` (subagent, no `todowrite`)
- `explore` (subagent, deny `*` allow read/grep/glob)
- `scout`, plus internal helpers `compaction`, `title`, `summary`

Each agent alters: system prompt, tool whitelist, max steps, temperature/topP, permission ruleset.

### Concrete file map (for follow-up reads)

| Concern | File |
|---|---|
| Outer agent loop | `packages/opencode/src/session/prompt.ts:1240-1483` |
| `streamText` invocation | `packages/opencode/src/session/llm.ts:81-340` |
| Stream → events | `packages/opencode/src/session/processor.ts:300-619` |
| Tool definition contract | `packages/opencode/src/tool/tool.ts` |
| Tool registry | `packages/opencode/src/tool/registry.ts:~290` |
| AI-SDK tool adapter | `packages/opencode/src/session/tools.ts:~30-90` |
| Edit tool | `packages/opencode/src/tool/edit.ts` |
| Subagent dispatch | `packages/opencode/src/tool/task.ts` |
| Permissions | `packages/opencode/src/permission/index.ts` |
| Agent definitions | `packages/opencode/src/agent/agent.ts` |
| Provider abstraction | `packages/opencode/src/provider/provider.ts:94-820` |
| Bus | `packages/opencode/src/bus/index.ts` |
| SSE handler | `packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts` |
| Run-state / cancellation | `packages/opencode/src/session/run-state.ts` |
| Storage (sqlite/drizzle) | `packages/opencode/src/storage/db.ts` |
| System prompts | `packages/opencode/src/session/system.ts` + `session/prompt/*.txt` |
| Snapshots | `packages/opencode/src/snapshot/` |

### Patterns worth borrowing

1. **No bespoke loop around `streamText`.** Vercel AI SDK already streams tool calls + results in one request; harness reruns it only when `finish_reason` is `tool-calls` or any tool part went through.
2. **`experimental_repairToolCall`** — graceful fallback for case-insensitive tool-name typos and a hard-coded `"invalid"` tool, so malformed tool calls surface as a normal tool result.
3. **InvalidArgumentsError with model-facing `.message`** — schema decode failures are converted to a tool-result-shaped error string the model can parse.
4. **One Bus, one SSE.** All UI surfaces share the same `text/event-stream` feed.
5. **Permissions decoupled from tools via `ctx.ask`** — every write tool calls the same `Permission.ask` Effect; unanswered requests block on a `Deferred` that lives in `InstanceState`.
6. **Subagent = a fresh Session running the same loop** — no separate execution path.
7. **Snapshot at every step-start, diff at step-finish** — gives a free, persistent revert/diff stream.
