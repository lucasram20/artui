# Phase N4 — Mutating LSP Operations (rename + code actions)

**Phase:** N4 (LSP support, mutating ops with approval)
**Spec:** [docs/spec/lsp.md](../../spec/lsp.md)
**Depends:** N1 (skeleton), N2 (read-only ops), N3 (writethrough),
            M2 (permission ask UI)
**Estimated PR size:** ~600 LoC
**Target release:** v0.7.0

---

## Why

Rename and code actions are where LSP earns its keep beyond search-and-edit.
A rename touches re-exports, barrel files, and aliased imports correctly —
something `apply_patch` plus grep cannot do reliably. Code actions surface
"add missing import", "destructure parameter", "extract function" — common
cleanups the agent should be able to invoke.

These are the *mutating* ops, so they go through artui's approval engine.
Until M2 ships the in-flight Ask UI, these phases compose poorly: we'd
auto-allow which silently mutates the workspace. So N4 is gated on M2.

## Scope

### In scope

- `LspAction::Rename`:
  - Send `textDocument/prepareRename` first (capability gate; some
    servers reject in-flight when the cursor is on a non-renameable
    symbol).
  - Send `textDocument/rename` with `new_name`. Receives a
    `WorkspaceEdit`.
  - Render the `WorkspaceEdit` as a unified diff across all touched
    files. Show file count + line count summary.
  - Route through the same approval flow as `apply_patch`: pre-apply
    diff popup (M1), Ask modal if `permissions.tools.lsp_rename = ask`
    (M2). On approve, apply each `TextEdit` as a `apply_patch` call so
    the writethrough loop (N3) fires per file.
- `LspAction::CodeActions`:
  - Listing variant: `{action: code_actions, path, line, column}` —
    returns the menu of available actions. Read-only.
  - Apply variant: `{action: code_actions, apply: <id>}` — runs the
    selected action's `WorkspaceEdit`. Same approval path as rename.
- `workspace/applyEdit` server-to-client request handling: when a
  language server proactively requests an edit (e.g. as part of a code
  action), route through approval engine instead of silently applying.
  The response back to the server (`{applied: true|false}`) reflects
  the user's decision.
- New permission knobs in `[permissions.tools]`:
  - `lsp_rename = "ask"` (default in Build, "deny" in Plan)
  - `lsp_code_actions_apply = "ask"` (default in Build, "deny" in Plan)
  - Listing code actions remains classified as read-only.

### Out of scope

- Quick-fix-on-diagnostic auto-trigger. The agent must explicitly
  request `code_actions` after seeing a diagnostic.
- Multi-file undo across rename. We rely on the existing
  apply_patch + snapshot rollback (M3) — a rename is just a fan-out of
  patches.
- LSP `executeCommand` server commands. Spec is too freeform; the
  surface area for arbitrary side effects is too large for v1.

## Acceptance criteria

- [ ] Rename a Rust function name across 4 files; approval popup
      shows the consolidated diff with file/line counts; on approve,
      all 4 files are rewritten and N3 writethrough confirms diagnostics
      are clean.
- [ ] Rename on a non-renameable symbol (a literal, a comment) returns
      a useful error from `prepareRename` rather than producing a bad
      WorkspaceEdit.
- [ ] `code_actions` listing on a Rust file with a missing-import error
      shows "add `use crate::Foo`" as an available action.
- [ ] Applying that action triggers the approval popup, then on
      approve adds the import and the writethrough confirms it.
- [ ] When >5 files are touched by a rename, the approval popup
      includes a `WARNING: 12 files affected` banner.
- [ ] `permissions.tools.lsp_rename = "deny"` blocks rename without
      asking; the tool result is a clear "rename denied by policy"
      string.
- [ ] `workspace/applyEdit` from the language server (e.g. an "organize
      imports" code action that touches multiple files) routes
      through approval.
- [ ] +8 tests covering: WorkspaceEdit → diff rendering, prepareRename
      gating, applyEdit approval flow, deny-policy.

## Files touched

```
src/lsp/types.rs                     +Rename, CodeActions variants
src/lsp/client.rs                    +workspace/applyEdit handler
src/lsp/render.rs                    +WorkspaceEdit → diff renderer
src/lsp/edits.rs                     NEW ~150 LoC — apply WorkspaceEdit
                                       through apply_patch fanout
src/tools/lsp.rs                     +rename + code_actions arms
src/permissions/mod.rs               +lsp_rename + lsp_code_actions_apply
                                       classifiers
src/config/schema.rs                 +tool permission defaults
docs/changelogs/CHANGELOG.md         +0.7.0 entry
```

## Test plan

| Layer        | Tests                                                          |
| ------------ | -------------------------------------------------------------- |
| Render       | WorkspaceEdit with 3 files renders as 3 diff hunks; counts correct |
| Mock client  | prepareRename rejects → tool returns clean error;              |
|              | applyEdit request routed to approval; approve → applied;       |
|              | reject → not applied                                            |
| Permissions  | deny-policy short-circuits before LSP call;                    |
|              | ask-policy waits on approval; allow-policy bypasses prompt     |
| Integration  | `cfg(integration)`: real rust-analyzer rename across 2 files;  |
|              | assert diff matches expected and writethrough confirms clean   |

## Risks

- **Approval-modal information overload**: a rename touching 30 files
  is a wall of diff. Mitigation: collapsed view by default, expand-all
  keybind, hard cap at 100 files (anything bigger surfaces a "split
  this rename" suggestion).
- **`workspace/applyEdit` race**: server expects a response within a
  short window. If the user is slow on the approval prompt, the server
  may time out and surface a confusing error. Mitigation: respond
  `{applied: false}` if the approval prompt is still open after 30 s
  and let the user retry by re-invoking the code action.
- **Partial application**: a multi-file WorkspaceEdit that fails on
  file 3 of 5 leaves files 1-2 modified. Mitigation: rely on M3
  snapshots for rollback; the tool result clearly reports
  `applied 2/5 files; abort on file 3`.
- **rust-analyzer rename quirks**: ra is sometimes overconfident and
  renames inside string literals/comments. Acceptable; the diff in the
  approval popup catches it before disk hits.
