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
