use clap::Parser;
use jiff::ZonedDateTime;
use jiff::civil::date;
use ostracon_cli::{Cli, CliError, Runner};
use ostracon_core::Repository;
use serde_json::Value;

fn now() -> ZonedDateTime {
    date(2026, 1, 14)
        .at(10, 30, 0, 0)
        .in_tz("Europe/London")
        .unwrap()
}

fn run_with(repo: &Repository, args: &str, answer: Option<bool>) -> Result<String, CliError> {
    let args = shell_words(args);
    let cli =
        Cli::try_parse_from(std::iter::once("ostracon".to_owned()).chain(args)).map_err(|e| {
            CliError {
                message: e.to_string(),
                status: 64,
            }
        })?;
    let mut ask = |_: &str| answer.unwrap_or(false);
    let confirm: Option<&mut dyn FnMut(&str) -> bool> = if answer.is_some() {
        Some(&mut ask)
    } else {
        None
    };
    Runner::with_clock(repo, now).run(cli, confirm)
}

fn run(repo: &Repository, args: &str) -> String {
    run_with(repo, args, None).unwrap_or_else(|e| panic!("{args}: {}", e.message))
}

/// Splits on spaces, keeping 'single-quoted' runs together.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '\'' => quoted = !quoted,
            ' ' if !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[test]
fn add_list_show() {
    let repo = Repository::in_memory().unwrap();
    assert_eq!(
        run(&repo, "project add Home --color 'lime green'"),
        "Added project 2 Home (lime_green)"
    );
    assert_eq!(
        run(&repo, "add 'Pay rent tomorrow 9am #Home !1h'"),
        "Added 1  [ ] Pay rent  due tomorrow 09:00 · #Home"
    );
    run(&repo, "add Book van --parent 1 --due fri");
    run(&repo, "add 'Inbox thing'");
    assert_eq!(
        run(&repo, "list"),
        "Inbox\n3  [ ] Inbox thing\n\nHome\n1  [ ] Pay rent  due tomorrow 09:00 · 0/1 subtasks\n2    [ ] Book van  due Friday"
    );
    assert_eq!(
        run(&repo, "list -p home"),
        "1  [ ] Pay rent  due tomorrow 09:00 · 0/1 subtasks\n2    [ ] Book van  due Friday"
    );
    assert_eq!(run(&repo, "list --today"), "No tasks.");
    assert_eq!(
        run(&repo, "list --due-by 'next week'"),
        "Home\n1  [ ] Pay rent  due tomorrow 09:00 · 0/1 subtasks\n2    [ ] Book van  due Friday"
    );
    let shown = run(&repo, "show 1");
    assert!(
        shown.starts_with(
            "[ ] Pay rent\nID 1 · #Home · due tomorrow 09:00\nReminders: 1 hour before"
        ),
        "{shown}"
    );
    assert!(
        shown.ends_with("Subtasks:\n  2  [ ] Book van  due Friday"),
        "{shown}"
    );
}

#[test]
fn edit_done_reopen() {
    let repo = Repository::in_memory().unwrap();
    run(&repo, "add 'Report friday'");
    run(&repo, "add Other");
    assert_eq!(
        run(&repo, "edit 1 --due 'next mon 9am' --remind 30min"),
        "Updated 1  [ ] Report  due Monday 09:00 · #Inbox"
    );
    assert_eq!(
        run(&repo, "edit 1 --no-due --no-reminders --name 'Report v2'"),
        "Updated 1  [ ] Report v2  #Inbox"
    );
    assert_eq!(
        run(&repo, "done 1 2"),
        "Completed 1  [x] Report v2  #Inbox\nCompleted 2  [x] Other  #Inbox"
    );
    assert_eq!(run(&repo, "list"), "No tasks.");
    assert_eq!(run(&repo, "list --all").lines().count(), 3);
    assert_eq!(run(&repo, "reopen 1"), "Reopened 1  [ ] Report v2  #Inbox");
    assert_eq!(run_with(&repo, "edit 1", None).unwrap_err().status, 64);
}

#[test]
fn done_is_all_or_nothing() {
    let repo = Repository::in_memory().unwrap();
    run(&repo, "add A");
    let error = run_with(&repo, "done 1 99", None).unwrap_err();
    assert_eq!(error.message, "No task with ID 99.");
    assert_eq!(run(&repo, "list"), "Inbox\n1  [ ] A");
}

#[test]
fn deleting_asks_first() {
    let repo = Repository::in_memory().unwrap();
    run(&repo, "add Parent");
    run(&repo, "add Child --parent 1");
    let refused = run_with(&repo, "delete 1", Some(false)).unwrap_err();
    assert_eq!(refused.message, "Nothing deleted.");
    let no_terminal = run_with(&repo, "delete 1", None).unwrap_err();
    assert_eq!(
        no_terminal.status, 64,
        "without a terminal, --yes is needed"
    );
    assert_eq!(
        run_with(&repo, "rm 1", Some(true)).unwrap(),
        "Deleted task 1 \"Parent\" and its 1 subtask."
    );
    run(&repo, "project add Old Stuff");
    run(&repo, "add Thing -p 'old stuff'");
    assert_eq!(
        run(&repo, "project delete '#Old Stuff' --yes"),
        "Deleted project \"Old Stuff\" and its 1 task."
    );
}

#[test]
fn json_output_is_the_tools_own() {
    let repo = Repository::in_memory().unwrap();
    let output: Value =
        serde_json::from_str(&run(&repo, "add 'Buy milk tomorrow' --json")).unwrap();
    assert_eq!(output["name"], "Buy milk");
    assert_eq!(output["due"], "2026-01-15");
    let output: Value = serde_json::from_str(&run(&repo, "--json projects")).unwrap();
    assert_eq!(output["projects"][0]["name"], "Inbox");
    let literal: Value =
        serde_json::from_str(&run(&repo, "add 'Buy milk tomorrow' --literal --json")).unwrap();
    assert_eq!(literal["name"], "Buy milk tomorrow");
}

#[test]
fn mistakes() {
    let repo = Repository::in_memory().unwrap();
    assert_eq!(run_with(&repo, "show abc", None).unwrap_err().status, 64);
    assert_eq!(
        run_with(&repo, "list -p Nowhere", None)
            .unwrap_err()
            .message,
        "No project called \"Nowhere\". Run ostracon projects to see them."
    );
    assert_eq!(
        run_with(&repo, "list --today --due-by fri", None)
            .unwrap_err()
            .status,
        64
    );
    assert!(
        run_with(&repo, "project add X --color puce", None)
            .unwrap_err()
            .message
            .starts_with("Unknown colour")
    );
}
