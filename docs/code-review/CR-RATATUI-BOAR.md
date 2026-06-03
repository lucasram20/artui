# Code Review Refactor Instruction: CR-RATATUI-BOAR

| Field | Value |
|-------|-------|
| Author | GPT-5.5 Codex |
| Date | 2026-06-03 |
| Scope | Ratatui-first UI refactor instructions for a future coding agent |
| Primary goal | Reduce custom TUI code by adopting Ratatui built-ins and vetted Ratatui ecosystem widgets |
| Non-goal | Flue/Ink rewrite of artui core |

## Quick Verdict

Do **not** rewrite artui around Flue + Ink right now.

Do refactor the existing Rust/Ratatui UI into smaller, reusable components and use Ratatui prebuilt widgets where they fit.

Reason:

- artui already uses Rust + `ratatui = "0.30"` + `crossterm = "0.28"`.
- Flue is TypeScript/Node agent harness, not Rust-native.
- Ink is React/TypeScript terminal UI, which means a second full UI/runtime rewrite.
- Current artui value lives in Rust providers, tools, permissions, LSP, snapshots, MCP, sandboxing, and session store.

## Source Inputs

### Local docs

- `docs/spec/flue-framework-evaluation.md`

Relevant decision from that spec:

- Keep Rust TUI/provider/tool/session stack.
- Use Flue only as optional TypeScript sidecar for bounded workflows.
- Do not migrate core harness to Flue unless Node.js runtime dependency and a large rewrite are intentionally accepted.

### Ratatui docs fetched 2026-06-03

- `https://ratatui.rs/`
- `https://ratatui.rs/concepts/widgets/`
- `https://ratatui.rs/concepts/layout/`
- `https://ratatui.rs/recipes/apps/`
- `https://ratatui.rs/showcase/third-party-widgets/`

Docs takeaways:

- Widgets are Ratatui's core UI building blocks.
- Built-ins to prefer before custom drawing:
  - `Block`
  - `Paragraph`
  - `List`
  - `Table`
  - `Tabs`
  - `Scrollbar`
  - `Clear`
  - `Gauge`
  - `Canvas` only when real drawing is needed
- Use `StatefulWidget` / `render_stateful_widget` for selected/scrolling UI.
- Use `Layout`/`Constraint`/nested areas instead of hand-rolled coordinate math where possible.
- Consider ecosystem widgets only after checking maintenance and API fit:
  - `tui-widget-list` for heterogeneous widget lists
  - textarea/input crates for multiline prompt editing, if compatible with `ratatui 0.30`

### Graphify inputs

Existing Graphify output was used:

- `graphify-out/GRAPH_REPORT.md`
- `graphify-out/graph.json`

Graphify-relevant architecture:

- `src/app.rs` is a UI/state god object: ~3210 lines.
- `src/lib.rs` owns terminal setup/event loop/input dispatch: ~810 lines.
- `src/ui/layout.rs` owns broad frame layout/footer/header rendering: ~1061 lines.
- `src/ui/chat.rs` owns transcript rendering: ~666 lines.
- `src/ui/popups.rs` owns modal/popup rendering: ~730 lines.
- `handle_key()` is a major hub in `src/lib.rs`.
- `run_turn()` in `src/agent/loop.rs` should stay outside this UI refactor.
- `ToolRegistry`, `LspClient`, provider/session/auth clusters are non-UI core and should not be rewritten for this work.

## Refactor Objective

Create a Ratatui-first component layer that shrinks custom UI drawing without changing artui behavior.

Target end state:

```text
src/ui/
  components/
    chrome.rs        # app frame, title/header/footer wrappers
    transcript.rs    # chat transcript viewport
    prompt.rs        # input/prompt/editor surface
    selectors.rs     # provider/model/theme/statusline pickers
    approvals.rs     # tool approval dialog/widgets
    statusline.rs    # footer/status item composition
    scroll.rs        # shared scroll state/helpers if needed
  layout.rs          # high-level composition only
  chat.rs            # markdown/message rendering helpers only
  popups.rs          # compatibility wrapper during migration
```

Do not force this exact structure if current code has a better nearby pattern, but keep files focused.

## Hard Constraints

- Preserve current behavior before visual redesign.
- No Flue dependency for normal `artui` startup.
- No Ink/React/TypeScript rewrite.
- No provider/tool/permission/LSP/session/snapshot/sandbox rewrite.
- No broad formatting churn.
- No unrelated docs/todos files. Backlog lives in GitHub Project/issues.
- Keep changes reviewable as small commits.
- Add tests or snapshots before changing rendering behavior.
- Prefer borrowing over cloning in Rust code.
- No `unwrap()`/`expect()` in production paths.
- Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and targeted tests before claiming done.

## Phase Plan

### Phase 0 — Baseline and Safety Net

**Status: done** ([#49](https://github.com/lucasram20/artui/pull/49))

1. Record current behavior with screenshots/snapshots if available.
2. Add or update rendering tests around:
   - footer/statusline composition
   - popup centering/clearing
   - transcript viewport/scroll behavior
   - prompt wrapping
   - provider/model/theme/statusline selector lists
3. Do not refactor until tests capture current visible behavior.

Expected commit:

```bash
git commit -m "test(ui): capture ratatui rendering baseline"
```

### Phase 1 — Component Boundaries

**Status: done** ([#50](https://github.com/lucasram20/artui/pull/50))

Extract pure rendering helpers first. No dependency changes.

Candidates:

- `src/ui/statusline.rs`
  - Move footer item selection/truncation out of `layout.rs`.
  - Use `unicode-width` if not already wired for terminal-cell width.
- `src/ui/components/chrome.rs`
  - App shell: header/body/footer areas.
  - Use `Block`, `Paragraph`, `Layout`, and `Constraint`.
- `src/ui/components/approvals.rs`
  - Approval modal structure.
  - Use `Clear` before modal rendering.
- `src/ui/components/selectors.rs`
  - Provider/model/theme/statusline pickers.
  - Prefer `List` + state over custom line math.

Expected commit:

```bash
git commit -m "refactor(ui): extract ratatui component boundaries"
```

### Phase 2 — Replace Custom Lists With Ratatui Lists

**Status: done** ([#51](https://github.com/lucasram20/artui/pull/51))

Use built-in `List`, `ListItem`, and `ListState` where items are homogeneous:

- model picker
- provider picker
- theme picker
- statusline item picker
- slash command picker, if present

Use `render_stateful_widget` for selected rows and viewport preservation.

Only consider `tui-widget-list` if built-in `List` cannot represent heterogeneous rows cleanly.

Expected commit:

```bash
git commit -m "refactor(ui): use ratatui list widgets for selectors"
```

### Phase 3 — Prompt/Input Surface

**Status: done** — `components/prompt.rs`; no textarea crate (criteria unmet / unnecessary).

Investigate a textarea/input crate compatible with current Ratatui.

Acceptance criteria before adding a dependency:

- Compatible with `ratatui 0.30`.
- Handles multiline input.
- Supports cursor movement and scrolling.
- Does not break paste handling.
- Does not force crossterm version conflicts.
- Maintained enough for production use.

If dependency fit is poor, keep internal prompt code but wrap it as `PromptWidget`.

Expected commit:

```bash
git commit -m "refactor(ui): isolate prompt editor widget"
```

### Phase 4 — Transcript Viewport

**Status: done** — content-hash cache + viewport overscan in `chat.rs`.

Main risk from prior reviews: transcript rendering reparses/reallocates too much.

Do:

- Keep provider/model transcript data separate from UI-render cache.
- Cache parsed/rendered message lines by message id/content hash.
- Render visible viewport plus small overscan.
- Use `Paragraph` with controlled wrapping where sufficient.
- Keep markdown parsing behavior unchanged.

Do not:

- Change message persistence format.
- Change model context construction.
- Remove existing transcript content.

Expected commit:

```bash
git commit -m "perf(ui): cache transcript rendering"
```

### Phase 5 — Layout Cleanup

**Status: done** — `Layout::vertical`, transcript scrollbar.

Reduce manual coordinate math in `src/ui/layout.rs`.

Use:

- `Layout::vertical` / `Layout::horizontal`
- `Constraint::Length`, `Constraint::Min`, `Constraint::Fill`, `Constraint::Percentage`
- nested `Rect`s
- `Clear` for overlays
- `Scrollbar` for long scrollable views where useful

Keep any custom cell-level drawing only where Ratatui widgets cannot express the UI clearly.

Expected commit:

```bash
git commit -m "refactor(ui): simplify layout composition"
```

## Implementation status

Tracked in [#48](https://github.com/lucasram20/artui/issues/48).

| Phase | Status | PR |
|-------|--------|-----|
| 0 — Baseline tests | Done | [#49](https://github.com/lucasram20/artui/pull/49) |
| 1 — Component boundaries | Done | [#50](https://github.com/lucasram20/artui/pull/50) |
| 2 — ListState selectors | Done | [#51](https://github.com/lucasram20/artui/pull/51) |
| 3 — Prompt widget | Done | [#52](https://github.com/lucasram20/artui/pull/52) |
| 4 — Transcript viewport | Done | [#53](https://github.com/lucasram20/artui/pull/53) |
| 5 — Layout cleanup | Done | [#54](https://github.com/lucasram20/artui/pull/54) |

Phase notes:

- **0**: `geometry`, `composer`, `render_tests` baseline.
- **1**: `statusline`, `components/{chrome,selectors,approvals}`, thin `popups`.
- **2**: `ui/list.rs` + `render_stateful_widget` for pickers and slash suggestions.
- **3**: `components/prompt.rs` — no new textarea crate (existing `composer` retained).
- **4**: transcript line cache + viewport overscan window in `chat.rs`.
- **5**: `Layout::vertical`, transcript `Scrollbar`, minor clippy fixes.

## Dependency Policy

Allowed without extra approval:

- Ratatui built-in widgets.
- Small helper modules under `src/ui`.

Needs explicit review before adding:

- `tui-widget-list`
- textarea/input crates
- `unicode-width` if not already present
- any animation/effects crate

Reject for this refactor:

- Flue as core runtime.
- Ink.
- React/Node UI runtime.
- new app framework layer.

## Rust Quality Rules

Follow Rust best practices:

- Prefer `&str`/`&[T]` params over owned `String`/`Vec<T>`.
- Avoid clones in render loops.
- Avoid intermediate `collect()` in hot paths unless needed.
- Use small structs with clear ownership.
- Use `Result` for fallible helpers; no production panics.
- Prefer explicit state structs for widgets over hidden globals.
- Keep public APIs documented when exposed outside `src/ui`.

## Review Checklist for Coding Agent

Before opening PR / marking complete:

- [x] No Flue/Ink/Node dependency added for normal startup.
- [x] `src/agent/loop.rs` behavior unchanged except imports if unavoidable.
- [x] provider/tool/permission/LSP/session/snapshot/sandbox modules untouched unless required by tests (sandbox: clippy-only one-liner).
- [x] UI changes are split into focused commits.
- [x] Ratatui built-ins used before ecosystem widgets.
- [x] New dependency, if any, is justified in PR body (none added).
- [x] `cargo fmt --check` passes.
- [x] `cargo clippy --all-targets -- -D warnings` passes.
- [x] `cargo test` or targeted UI/render tests pass (`cargo test ui::` — 27 tests).
- [ ] Manual smoke test in terminal:
  - open app
  - type/paste multiline prompt
  - use slash commands
  - open provider/model/theme/statusline pickers
  - trigger a tool approval
  - scroll transcript
  - resize terminal

## Suggested GitHub Issue

Create or link a GitHub issue before implementation:

```bash
gh issue create \
  --repo lucasram20/artui \
  --title "Refactor TUI toward reusable Ratatui components" \
  --body-file docs/code-review/CR-RATATUI-BOAR.md
```

Then add it to the project board:

```bash
gh project item-add 2 --owner lucasram20 --url <ISSUE_URL>
```

## Final Instruction

This is a **refactor**, not a rewrite.

Keep artui Rust-first. Harvest Ratatui's reusable components. Preserve current product behavior. Reduce custom UI code gradually.
