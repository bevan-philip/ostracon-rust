# CLAUDE.md

Ostracon: a local, Todoist-style to-do list. `rust-port-design-doc.md` is the product brief; when a choice is
unclear, do what Todoist does. README.md covers usage.

## Workspace

- `ostracon-core`: everything without UI. Model (`model.rs`), repository (`repo.rs`, rusqlite), date grammar
  (`dates.rs`), quick-add draft and parsing (`quickadd.rs`), outline (`tree.rs`), notification schedule
  (`schedule.rs`), backups (`backup.rs`), settings, paths, and the tool layer (`tools.rs`) that the CLI and
  MCP server both call so they behave identically.
- `ostracon-cli` (binary `ostracon`), `ostracon-mcp`: thin layers over `ostracon_core::tools`.
- `ostracon-app`: `state.rs` is the UI state model with no GPUI, tested in `tests/state.rs`. `ui/` holds the
  GPUI views; `ui/workspace.rs` owns the `AppState` and `sync()` makes the views match it after every change.
  `platform/` is the only macOS-specific code (notifications); keep it that way, and look for a
  cross-platform route before adding to it.

## Rules

- Dates and times are jiff 1.0 everywhere. chrono appears only at the boundary with gpui-kit's calendar
  (`ui/pickers.rs`).
- Every write goes through `Repository::transaction` (savepoints nest), and in the app through
  `AppState::perform`, which reloads and reconciles. Change UI state only after a write succeeds.
- State that refers to tasks or projects is an explicit type in `state.rs` (`Selection`, `Composer`,
  `Dialog`, the overview); `AppState::reconcile` fixes them all up after every reload. Add new cases there.
- Quick-add text must never take over a title: a reverted block is literal text and is never re-read, and an
  existing task's name loads as literal.

## GPUI notes

- gpui comes only through `gpui-kit = "=0.9.0"`; never add gpui or gpui-pre crates directly. Import with
  `use gpui_kit::*;` and `gpui_kit::prelude::FluentBuilder` for `.when`.
- gpui-pre 0.3.11 needs a recent compiler; `rust-toolchain.toml` pins it.
- Bare identifiers in key-binding predicates match only the innermost context, but `!X` checks the whole
  stack: list shortcuts use `TaskList && !QuickAdd && !Input` so plain keys don't fire while typing.
- Render helpers that return `impl IntoElement` need `+ use<>` or they borrow `self` and `cx`.
- Use `gpui_kit::assets::AllAssets`; `Assets` only has gpui-component's default icons.
- The macOS notification centre aborts outside an app bundle; `Notifier::new` returns `None` there.
- Inside the app bundle the CLI is `ostracon-cli`: the disk is case-insensitive, so `ostracon` would
  overwrite `Ostracon`.

## Commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt
scripts/bundle-macos.sh --open
```

Commits are atomic and bisectable, straight to main.
