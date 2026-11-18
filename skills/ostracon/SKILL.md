---
name: ostracon
description: Read and manage the user's tasks and projects in Ostracon, their local Todoist-style task manager, with the `ostracon` command. Use when the user asks what's on their list, what's due today or overdue, to add, reschedule, complete or delete a task or subtask, to set a reminder, or to create, rename or remove a project in Ostracon, including "add X to my todo list" or "what do I have on tomorrow" when Ostracon is their task manager.
---

# Ostracon

Ostracon is a local, Todoist-style task manager backed by one SQLite file. The `ostracon` command reads and changes the same database as the app, whether or not the app is open, and the app shows the changes within a second.

## Finding the command

Use the first of these that exists:

1. `ostracon` on the `PATH` (`command -v ostracon`)
2. `/Applications/Ostracon.app/Contents/MacOS/ostracon`
3. `~/.cargo/bin/ostracon`
4. In a checkout of the Ostracon repository: `cargo run -q -p ostracon-cli --`

If none exists, tell the user Ostracon isn't installed (`cargo install --git <repo> ostracon-cli` installs it). Don't set `OSTRACON_DATABASE` unless the user asks; it points the CLI at a different database from the app's.

`ostracon --help`, and `ostracon <command> --help`, list every option.

## Reading

```bash
ostracon list                        # open tasks, grouped by project, in outline order
ostracon list --today                # due today or overdue
ostracon list --due-by 'next week'   # due on or before a day
ostracon list -p Home --all          # one project, including completed tasks
ostracon list --search dentist       # name or description contains the text
ostracon show 12                     # one task with description, reminders and every subtask
ostracon projects                    # projects with IDs and open-task counts
```

Filters combine. Each line starts with the task's ID, and subtasks are indented under their parent:

```
Home
4  [ ] Move house  due tomorrow 17:00 · 0/2 subtasks
5    [ ] Book van
```

Add `--json` to any command to get the tool's JSON instead of text; prefer it when you need to read fields reliably.

## Changing

```bash
ostracon add 'Pay rent tomorrow 9am #Home !1h'   # quick-add: #project, a date and !reminders are read out of the text
ostracon add 'Book van' --parent 4 --due fri      # or use flags; flags win over the text
ostracon add 'Read #hashtags literally' --literal # keep the text exactly as typed
ostracon edit 12 --due 'next mon 9am' --remind 30min
ostracon edit 12 --name 'New name' --description 'Notes'
ostracon edit 12 --no-due --no-reminders
ostracon edit 12 -p Work                          # moves it, with its subtasks
ostracon done 12 13                               # completes them and their subtasks, all or nothing
ostracon reopen 12                                # also reopens completed parents
ostracon delete 12 --yes                          # permanent; prefer done for finished work
ostracon project add 'Work Admin' --color 'lime green'
ostracon project edit 'Work Admin' --name Admin
ostracon project delete Admin --yes               # deletes its tasks too
```

- Always single-quote task text so the shell leaves `#` and `!` alone.
- Projects can be given by ID or name (case-insensitive, with or without `#`).
- `edit --name` is taken literally: quick-add syntax isn't read when renaming.
- Deleting without `--yes` asks on a terminal and fails otherwise, so pass `--yes` once the user has agreed. Confirm with the user before deleting anything they didn't explicitly ask to delete.

## Dates and reminders

Dates are read in the user's local time: `today`, `tomorrow`, `tonight`, `fri`, `this fri`, `next fri`, `next week`, `weekend`, `next month`, `end of month`, `in 3 days`, `in 2 hours`, `jan 27`, `27/1` (numeric order follows the user's region; prefer unambiguous forms), `2026-01-27`, `2026-01-27T17:00`, optionally with a time such as `5pm`, `17:30`, `at 17` or `noon`.

Reminders are either before the due time (`30min`, `1 hour before`, `1d`; they need a due time to fire) or at a moment (`tomorrow 9am`, `3pm`, where a bare time falls on the due day). A task with a due time also notifies at that time.
