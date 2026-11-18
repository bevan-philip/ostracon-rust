# Ostracon (Rust) — Design Doc

A greenfield Rust rewrite of Ostracon. This doc records what the app is for, what it must do, and what has mattered most while building the Swift version. It describes behaviour, not implementation; technology choices are left to the new repo.

## Why it exists

Ostracon is a **local, Todoist-style to-do list for a work machine where Todoist can't be used.** It isn't meant to compete with Todoist. It copies Todoist's UX as closely as is sensible and keeps all data on the machine.

That leads to three standing constraints:

- **Local-first, no accounts, no cloud service.** All data lives in one local SQLite file.
- **Minimal scope.** The original brief was "Inbox, projects, `#` filing, good keyboard flow, nested tasks, Todoist's look. We don't need a lot more." New features should earn their place against that.
- **UX is the priority.** In the user's words, "the UX is key." When a choice is unclear, do what Todoist does.

## What the user has cared about

These are the themes that kept coming up across the history and past sessions, roughly in order of weight.

1. **Todoist fidelity in the workflow.** Quick-add, `#Project`, natural-language dates and inline blocks all copy Todoist. When something behaved slightly differently from Todoist, the user called it out and it was fixed (for example, the block/backspace behaviour).
2. **A fast keyboard flow.** Pressing Return saves the task and immediately opens a fresh composer for the next one. Everything in the list can be done from the keyboard.
3. **Quick-add text must never take over the title.** Recognised tokens turn into blocks, and backspacing over a block makes it plain text again for good, so you can still write "Plan Monday's standup". This was stated explicitly as important.
4. **Visual polish in Todoist's style.** The user has asked for "more aesthetic", then a flat Todoist list with hairline dividers, larger type, bigger sidebar targets, and a full task overview.
5. **Nested tasks done properly.** Nested tasks have to look right and show their state. A parent should make it obvious that it has completed subtasks, even when completed tasks are hidden.
6. **Direct manipulation.** Drag tasks onto projects, select several tasks and act on them together, double-click to edit, click to open an overview.
7. **Automation and agent access.** An MCP server, a CLI with the same capabilities, and an agent skill. Each one works on the same data whether or not the app is open.
8. **Data safety on a work machine.** Backups go to a synced folder such as OneDrive without ever exposing a half-written file. Backups are kept small (two days, not two weeks). Restoring first saves the current state.
9. **Correctness.** A whole pass went into making implicit state machines explicit: atomic multi-step changes, and UI state reconciled after every reload. The rewrite should model these as explicit states from the start.
10. **Easy installs on other machines.** Getting the app running on a second, managed laptop was a real pain point because of macOS Gatekeeper and quarantine. The user asked whether building locally, for example through Homebrew, would avoid it.

## Feature requirements

### Projects and tasks
- An **Inbox** that can't be deleted, plus user-created **projects**, each with a name and a colour from a fixed palette.
- A task has a name, an optional description, a project, an optional parent task, a position, a completed flag, an optional due date and any number of reminders.
- **Nesting**: indent, outdent, move up and down, and collapse or expand. Moving a task moves its subtasks with it.
- **Show/hide completed** tasks. With completed tasks hidden, a parent still shows its subtask progress ("1 of 2 done", "1 completed hidden").
- **Undo completion** from a toast.

### Quick-add composer
- One text field that understands:
  - `#Project` anywhere in the text, including multi-word names (`#Work Admin`), with autocomplete (↑/↓, Tab or Return to accept, Esc to dismiss).
  - **Natural-language due dates** anywhere in the text: `today`, `tomorrow`/`tmrw`, `tonight`, weekdays, `this` or `next <day>`, `next week`, `weekend`, `next month`, `end of month`, `in N days/weeks/hours`, `jan 27`, `27/1` (order follows the user's region), ISO dates, times (`5pm`, `17:00`, `at 17`, `noon`), and a date combined with a time.
  - **Reminders** with `!`: relative to the due time (`!30min`, `!1d`) or absolute (`!tomorrow 9am`, `!3pm`, where a bare time means the due day).
- A recognised token becomes an **inline block** once the following space is typed. **Backspacing over a block** turns it back into plain text that is never re-parsed.
- **An existing task's name is never re-parsed when it's edited.**
- Date and reminder chips show what the text was understood as, and offer pickers.
- Return saves. When adding, the composer stays open for the next task. Esc closes it.

### Due dates and reminders
- A due date is a day with an optional time. Due labels are coloured by urgency (overdue, today, soon).
- **Notifications are macOS-only** (see Platform support). System notifications fire at each reminder and at the due time for timed tasks. A notification offers a Complete action and opens the task when clicked.
- If notifications are disabled, the app warns, and it checks again when it comes back into focus.
- On other platforms, reminders and due dates are still stored, edited and shown; they just don't notify.

### List interaction
- Keyboard: ↑/↓ or J/K to move; ←/→ to collapse or expand (← on a leaf goes to its parent); Space or ⌘Return to complete; Return or ⌘E to edit; Tab/⇧Tab to indent/outdent; ⌥⌘↑/↓ to reorder; Delete to delete, with confirmation.
- Global shortcuts: ⌘N, A or Q to add a task; ⌥⌘N or ⇧A to add a subtask; ⇧⌘N for a new project; ⌘1…9 to switch project; ⇧⌘C to show or hide completed; ⌘O to open the overview.
- **Multi-select** with ⌘-click, ⇧-click and ⇧-arrow. Complete, delete, set due date and move to project apply to the whole selection. Editing and outlining work on one task at a time.
- **Drag** a task or a selection onto a sidebar project to move it, subtasks included.
- Double-clicking a task edits it in place. Clicking a task opens its overview.

### Task overview
- A Todoist-style detail sheet showing the task's project and parent (breadcrumb), its name and description editable in place, its subtasks (each opens its own overview), a button that opens the full composer to add subtasks, and the project, date and reminders down the side.
- Arrow keys move to the previous or next task in the list. **No comments.**
- The overview closes if its task is deleted or the project changes.

### Look and feel
- A sidebar with Inbox and projects, coloured icons and count badges, drawn at large size so items are easy to click and drop onto.
- Large project header. The project's colour lightly tints the page.
- A **flat, Todoist-style list**: hairline dividers that start at the checkbox, no cards, checkboxes aligned with the header, and disclosure chevrons hanging in the left margin.
- Larger type for task names (about 15pt), and the composer uses the same size so text doesn't change size when you start editing.
- Completed tasks are de-emphasised. Accessibility matters: activating a task with a screen reader selects it rather than completing it.

### Storage, sync and backups
- One local SQLite database in the platform's app-data directory, which an environment variable can override (backups are off while the override is set).
- The app, CLI and MCP server **share the database at the same time**. The app picks up outside changes within about a second and reschedules reminders. Concurrent writers wait for each other instead of failing.
- Every multi-step change is **atomic**. Nested transactions use savepoints.
- **Backups**: the user picks a folder in Settings. The app backs up at most once a minute after a change, and again on quit. It keeps one file per day for the last two days (`Ostracon YYYY-MM-DD.sqlite`). Each backup is written outside the folder and then moved in.
- **Restore** replaces the current data with a backup's, after saving the current data as `Ostracon before restore ….sqlite`.
- The live database never sits inside the synced folder.

### MCP server, CLI and skill
- A shared **tool layer** that the MCP server and the CLI both call, so they behave identically:
  - `list_projects`
  - `list_tasks` (filter by project, text, or `due_by: today`)
  - `get_task`
  - `add_task` (accepts quick-add text)
  - `update_task`
  - `complete_task`
  - `delete_task`
  - `add_project`, `update_project`, `delete_project`
- **MCP server**: unauthenticated, stdio by default. `--http [port]` listens on loopback only and rejects requests whose Host or Origin isn't local. Parsing is strict: malformed requests get 400, oversized ones get 413, and bad lengths must never crash it.
- **CLI**: `add`, `list` (`--today`, `-p`, `--all`), `show`, `edit`, `done`, `reopen`, `delete` (asks first; `--yes` skips), `projects`, `project add/edit/delete`. Projects can be given by ID or name. `--json` prints the raw tool output, and `--literal` stops quick-add parsing.
- An **agent skill** (SKILL.md) that teaches coding agents to use the CLI.
- The MCP server and the CLI ship alongside the app.

### Build and release
- Tests cover the parsers, the task tree, the repository, the app state model's invariants, the MCP server and the CLI. The UI state model should be testable without a GUI.
- Pushing a `v*` tag triggers a CI release that runs the tests and attaches the built app to a GitHub release.
- Installing on a second machine should be easy. Consider a build-from-source route such as a Homebrew formula or `cargo install` for the CLI and MCP server, so quarantine and signing problems don't block use.
- Commits are atomic and bisectable, made straight to main.

## Platform support

- Everything is cross-platform by default. Any feature that can't easily be done the same way on every platform is built **for macOS only**, behind a single platform module, and the rest of the app doesn't depend on it.
- At present the only such feature is **notifications**: scheduling them, the Complete action, opening a task from a notification, and checking whether permission was granted.
- Before adding another macOS-only feature, try to find a cross-platform way to do it first.

## Design principles for the rewrite

- **Put the core in a separate library** with no UI: the data model, the repository, the parsers, the task tree, the reminder schedule and backups. The app, CLI and MCP server are thin layers over it.
- **Make state machines explicit.** The composer mode, the selection, the open overview, dialogs and the HTTP connection should each be a proper state type, not a set of loose optional values. After every data reload, one `reconcile` step fixes up the UI state: drop selections and editors pointing at tasks that are gone, hand selection to a parent when its children collapse, close dialogs whose target was deleted.
- **Only update UI state after a write succeeds**, so a rolled-back change never leaves the screen showing something that didn't happen.
- **Keep compatibility with the existing SQLite schema** if it's cheap to do, so a current database or backup opens in the Rust version. Otherwise provide a one-off import.

## Out of scope

Accounts, cloud sync, collaboration, comments, labels, filters, priorities, recurring tasks, and anything else Todoist has beyond the list above, unless asked for later.
