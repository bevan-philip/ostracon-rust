# Ostracon

## The human summary before the slop
- Truthfully, I love Todoist, and will continue to be a paying customer.
- I love it so much that my inability to use it at work was irritating me. I just wanted a local to-do list app, with similar UX to Todoist.
- So, this does that. Again, I'm never going to use this in a personal capacity. 

## Description

A local, Todoist-style to-do list for a work machine where Todoist can't be used. Everything stays in one
SQLite file on the machine: no accounts, no cloud.

The app uses [GPUI](https://gpui.rs) through gpui-kit; the CLI and MCP server share its core and its database.

- **Inbox** plus your own colour-coded **projects**
- **Quick add**: `#Project` anywhere (multi-word names work, with autocomplete), Todoist-style dates
  (`tomorrow 5pm`, `next fri`, `in 3 days`, `jan 27`, `27/1`, `end of month`) and `!reminders`
  (`!30min`, `!tomorrow 9am`). Each becomes an inline block once you type the space after it; backspace
  over a block to turn it back into plain words for good, so a task can still be called "Plan Monday's
  standup". An existing task's name is never re-read when you edit it.
- **Nested tasks**: indent, outdent, reorder, collapse; parents show "1/2" and "1 completed hidden"
- **Keyboard first**: Return saves and opens the composer for the next task
- **Multi-select** with ⌘-click, ⇧-click and ⇧-arrows; **drag** tasks to reorder them (sideways to nest or un-nest) or onto a sidebar project
- **Task overview**: click a task for its description, sub-tasks, project, date and reminders
- **Notifications** at each reminder and at due times (macOS), with a Complete button
- **Backups** to a synced folder such as OneDrive, never exposing a half-written file
- **CLI and MCP server** over the same data, whether or not the app is open

## Install

### From source (recommended on a managed Mac)

Building locally means nothing downloaded carries a quarantine flag, so Gatekeeper doesn't get in the way.
You need Xcode (or the Command Line Tools) and [rustup](https://rustup.rs); `rust-toolchain.toml` pins the
Rust version GPUI needs, and rustup installs it on first build.

```bash
scripts/bundle-macos.sh --open        # builds dist/Ostracon.app and launches it
```

Or with Homebrew, which does the same build:

```bash
brew tap bevan-philip/ostracon-rust https://github.com/bevan-philip/ostracon-rust
brew install --HEAD ostracon
```

Just the command-line tools, on any platform:

```bash
cargo install --git https://github.com/bevan-philip/ostracon-rust ostracon-cli ostracon-mcp
```

### Releases

Pushing a `v*` tag runs the tests and attaches `Ostracon-<tag>-macos-arm64.zip` (the app) and
`ostracon-tools-<tag>-macos-arm64.tar.gz` (CLI, MCP server and agent skill) to a GitHub release. The app is
ad-hoc signed, not notarised: after downloading, run `xattr -dr com.apple.quarantine Ostracon.app` or build
from source instead.

## Data

Data lives in the platform's app-data folder: `~/Library/Application Support/Ostracon/ostracon.sqlite` on
macOS. Set `OSTRACON_DATABASE=/some/path.sqlite` to use another file; backups are off while it is set, so a
test database never overwrites your real backups.

The app, CLI and MCP server use the database at the same time. The app picks up outside changes within a
second and reschedules reminders. Every change is one transaction, and concurrent writers wait for each other
rather than fail.

### Appearance

**Settings (⌘,)** chooses light or dark, or **System** (the default) to follow the system as it changes.

**Colours** switches between **Classic**, Todoist's colours, and [Flexoki](https://stephango.com/flexoki). Each has
a light and a dark variant.

### Backups

In **Settings (⌘,)**, choose a folder, for example one in OneDrive. Ostracon copies your tasks there when they
change (at most once a minute) and when it quits, keeping one file a day (`Ostracon 2026-10-30.sqlite`) for
the last two days. Each copy is written outside the folder and then moved in, so the sync service only ever
sees complete files. The live database never sits in the synced folder. **Restore** replaces your tasks with
a backup's, after saving the current ones as `Ostracon before restore ….sqlite`.

## Command line

```bash
ostracon add 'Pay rent tomorrow 9am #Home !1h'   # quick-add syntax, as in the app
ostracon add Book van --parent 12 --due fri       # or flags
ostracon list --today                             # due today or overdue
ostracon list -p Home --all                       # one project, including completed tasks
ostracon show 12
ostracon edit 12 --due 'next mon 9am' --remind 30min
ostracon done 12 13
ostracon delete 12                                # asks first; --yes to skip
ostracon projects
ostracon project add 'Work Admin' --color 'lime green'
```

Projects can be given by ID or name. Single-quote task text so the shell leaves `#` and `!` alone. `--json`
prints the tool's JSON, and `--literal` stops quick-add parsing. Inside the app bundle the CLI is
`Ostracon.app/Contents/MacOS/ostracon-cli` (a case-insensitive disk can't hold both `Ostracon` and
`ostracon`); link it onto your `PATH` as `ostracon`.

[`skills/ostracon`](skills/ostracon/SKILL.md) is an agent skill that teaches coding agents such as Claude
Code to manage your tasks with the CLI. Copy the folder into `~/.claude/skills/`.

## MCP server

`ostracon-mcp` serves the same tools over the [Model Context Protocol](https://modelcontextprotocol.io):
`list_projects`, `list_tasks` (filter by project, text or `due_by: "today"`), `get_task`, `add_task`
(understands quick-add text), `update_task`, `complete_task`, `delete_task`, `add_project`,
`update_project`, `delete_project`.

It has **no authentication**. Over stdio (the default) only the client that launched it can talk to it. With
`--http [port]` it listens on `http://127.0.0.1:8765/mcp`, loopback only, and refuses requests whose `Host` or
`Origin` isn't local, but any program on your machine can still use it.

```bash
claude mcp add ostracon -- /Applications/Ostracon.app/Contents/MacOS/ostracon-mcp
```

## Keyboard

| Where | Key | Action |
| --- | --- | --- |
| Anywhere | ⌘N, or A / Q in the list | Add a task |
| | ⌥⌘N, or ⇧A in the list | Add a sub-task to the selected task, or in the overview to its task |
| | ⇧⌘N | New project |
| | ⌘1 … ⌘9 | Inbox, then projects |
| | ⇧⌘C | Show or hide completed tasks |
| | ⌘O | Open the selected task's overview |
| | ⌘, | Settings |
| Task list | ↑ ↓ or J K, ⇧↑ ⇧↓ | Move or extend the selection |
| | ← → | Collapse or expand (← on a leaf goes to its parent) |
| | Space or ⌘Return | Complete or reopen |
| | Return or ⌘E | Edit in place |
| | Tab / ⇧Tab (or ⌘] / ⌘[) | Indent / outdent |
| | ⌥⌘↑ / ⌥⌘↓ | Move up / down |
| | Delete | Delete (asks first) |
| | ⌘Z | Undo the last completion while its toast shows |
| Composer | Return | Save; when adding, start the next task |
| | `#` | Project autocomplete: ↑ ↓, Tab or Return to accept, Esc to dismiss |
| | Backspace after a block | Turn the block back into text |
| | Esc | Close |
| Overview | ↑ ↓ | Previous or next task |
| | Return or ⌘E, or click the name | Edit the name and description; Return saves, Esc cancels |

On Windows and Linux, Ctrl takes the place of ⌘ and Alt of ⌥.

## Layout

```
crates/ostracon-core   Model, SQLite repository, date and quick-add parsers, task tree, reminder schedule,
                       backups, and the tool layer shared by the CLI and MCP server. No UI.
crates/ostracon-cli    The ostracon command
crates/ostracon-mcp    ostracon-mcp: JSON-RPC handling, stdio and loopback HTTP
crates/ostracon-app    The GPUI app: state.rs (the UI state model, no GPUI), ui/ (views), platform/
                       (macOS-only notifications)
skills/ostracon        Agent skill for the CLI
scripts/               App bundling and icon
Formula/               Homebrew formula (build from source)
```

```bash
cargo test --workspace          # everything, including headless GPUI tests
cargo run -p ostracon-app       # run the app (notifications need the bundle)
```
