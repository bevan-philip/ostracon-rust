use std::time::Duration;

use jiff::civil::{date, time};
use ostracon_core::backup::{BackupFolder, BackupPolicy};
use ostracon_core::repo::Error;
use ostracon_core::{Due, NewTask, ProjectColor, Reminder, Repository, TaskId, schedule, tree};

fn add(repo: &Repository, name: &str, parent: Option<TaskId>) -> TaskId {
    repo.add_task(NewTask {
        name: name.into(),
        parent_id: parent,
        ..Default::default()
    })
    .unwrap()
    .id
}

fn names(repo: &Repository, show_completed: bool) -> Vec<String> {
    let inbox = repo.inbox().unwrap().id;
    tree::rows(&repo.tasks().unwrap(), inbox, show_completed)
        .into_iter()
        .map(|row| format!("{}{}", "  ".repeat(row.depth), row.task.name))
        .collect()
}

#[test]
fn starts_with_an_inbox_that_cannot_be_changed() {
    let repo = Repository::in_memory().unwrap();
    let inbox = repo.inbox().unwrap();
    assert_eq!(inbox.name, "Inbox");
    assert!(matches!(
        repo.delete_project(inbox.id),
        Err(Error::InboxIsFixed)
    ));
    assert!(matches!(
        repo.update_project(inbox.id, "Other", ProjectColor::Red),
        Err(Error::InboxIsFixed)
    ));
}

#[test]
fn projects_come_after_the_inbox_in_order() {
    let repo = Repository::in_memory().unwrap();
    repo.create_project("Work", ProjectColor::Red).unwrap();
    let home = repo
        .create_project(" Home ", ProjectColor::LimeGreen)
        .unwrap();
    assert_eq!(home.name, "Home");
    let names: Vec<_> = repo
        .projects()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["Inbox", "Work", "Home"]);
    assert!(matches!(
        repo.create_project("  ", ProjectColor::Red),
        Err(Error::EmptyName)
    ));
}

#[test]
fn deleting_a_project_deletes_its_tasks() {
    let repo = Repository::in_memory().unwrap();
    let work = repo.create_project("Work", ProjectColor::Red).unwrap();
    let parent = repo
        .add_task(NewTask {
            name: "A".into(),
            project_id: Some(work.id),
            ..Default::default()
        })
        .unwrap();
    add(&repo, "B", Some(parent.id));
    assert_eq!(repo.delete_project(work.id).unwrap(), 2);
    assert!(repo.tasks().unwrap().is_empty());
}

#[test]
fn tasks_keep_due_dates_and_reminders() {
    let repo = Repository::in_memory().unwrap();
    let due = Due::at(date(2026, 3, 1), time(9, 30, 0, 0));
    let at = "2026-02-28T12:00:00Z".parse().unwrap();
    let task = repo
        .add_task(NewTask {
            name: "  Pay rent ".into(),
            due: Some(due),
            reminders: vec![
                Reminder::Before { minutes: 30 },
                Reminder::At(at),
                Reminder::Before { minutes: 30 },
            ],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(task.name, "Pay rent");
    assert_eq!(task.due, Some(due));
    assert_eq!(
        task.reminders,
        vec![Reminder::At(at), Reminder::Before { minutes: 30 }]
    );
    repo.set_reminders(task.id, &[]).unwrap();
    repo.set_due(task.id, None).unwrap();
    let task = repo.task(task.id).unwrap();
    assert!(task.reminders.is_empty());
    assert_eq!(task.due, None);
}

#[test]
fn a_subtask_goes_in_its_parents_project() {
    let repo = Repository::in_memory().unwrap();
    let work = repo.create_project("Work", ProjectColor::Red).unwrap();
    let parent = repo
        .add_task(NewTask {
            name: "A".into(),
            project_id: Some(work.id),
            ..Default::default()
        })
        .unwrap();
    let child = repo.task(add(&repo, "B", Some(parent.id))).unwrap();
    assert_eq!(child.project_id, work.id);
    let inbox = repo.inbox().unwrap().id;
    let wrong = repo.add_task(NewTask {
        name: "C".into(),
        project_id: Some(inbox),
        parent_id: Some(parent.id),
        ..Default::default()
    });
    assert!(matches!(wrong, Err(Error::ParentInOtherProject(_))));
}

#[test]
fn completing_cascades_down_and_reopening_up() {
    let repo = Repository::in_memory().unwrap();
    let a = add(&repo, "A", None);
    let b = add(&repo, "B", Some(a));
    let c = add(&repo, "C", Some(b));
    repo.set_completed(a, true).unwrap();
    assert!(repo.tasks().unwrap().iter().all(|t| t.is_completed()));
    repo.set_completed(c, false).unwrap();
    assert!(!repo.task(a).unwrap().is_completed());
    assert!(!repo.task(b).unwrap().is_completed());
}

#[test]
fn outlining() {
    let repo = Repository::in_memory().unwrap();
    let a = add(&repo, "A", None);
    let b = add(&repo, "B", None);
    let c = add(&repo, "C", None);
    assert!(!repo.indent(a).unwrap(), "nothing above to indent under");
    assert!(repo.indent(b).unwrap());
    assert!(repo.indent(c).unwrap());
    assert_eq!(names(&repo, false), ["A", "  B", "  C"]);
    assert!(repo.indent(c).unwrap());
    assert_eq!(names(&repo, false), ["A", "  B", "    C"]);
    assert!(repo.outdent(c).unwrap());
    assert!(repo.outdent(b).unwrap());
    assert_eq!(
        names(&repo, false),
        ["A", "  C", "B"],
        "an outdented task goes straight after its parent"
    );
    assert!(!repo.outdent(a).unwrap());
    assert!(repo.move_by(b, -1).unwrap());
    assert_eq!(
        names(&repo, false),
        ["B", "A", "  C"],
        "subtasks move with their parent"
    );
    assert!(!repo.move_by(b, -1).unwrap());
}

#[test]
fn moving_skips_completed_siblings() {
    let repo = Repository::in_memory().unwrap();
    let a = add(&repo, "A", None);
    let b = add(&repo, "B", None);
    let c = add(&repo, "C", None);
    repo.set_completed(b, true).unwrap();
    assert!(repo.move_by(c, -1).unwrap());
    assert_eq!(names(&repo, false), ["C", "A"]);
    assert!(
        repo.indent(a).unwrap(),
        "indents under the open sibling above, not the completed one"
    );
    assert_eq!(names(&repo, false), ["C", "  A"]);
}

#[test]
fn moving_to_a_project_takes_subtasks() {
    let repo = Repository::in_memory().unwrap();
    let work = repo.create_project("Work", ProjectColor::Red).unwrap();
    let a = add(&repo, "A", None);
    let b = add(&repo, "B", Some(a));
    let c = add(&repo, "C", Some(b));
    repo.move_to_project(b, work.id).unwrap();
    let b = repo.task(b).unwrap();
    assert_eq!((b.project_id, b.parent_id), (work.id, None));
    assert_eq!(repo.task(c).unwrap().project_id, work.id);
    assert_eq!(repo.task(a).unwrap().project_id, repo.inbox().unwrap().id);
}

#[test]
fn collapsed_tasks_hide_subtasks_and_parents_count_completed_ones() {
    let repo = Repository::in_memory().unwrap();
    let a = add(&repo, "A", None);
    let b = add(&repo, "B", Some(a));
    add(&repo, "C", Some(a));
    repo.set_completed(b, true).unwrap();
    let inbox = repo.inbox().unwrap().id;
    let rows = tree::rows(&repo.tasks().unwrap(), inbox, false);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].subtask_count,
            rows[0].completed_subtask_count,
            rows[0].hidden_completed_count()
        ),
        (2, 1, 1)
    );
    repo.set_collapsed(a, true).unwrap();
    assert_eq!(names(&repo, true), ["A"]);
}

#[test]
fn a_failed_step_rolls_back_the_whole_change() {
    let repo = Repository::in_memory().unwrap();
    let a = add(&repo, "A", None);
    let result: Result<(), Error> = repo.transaction(|| {
        repo.update_task(a, "Renamed", "")?;
        repo.transaction(|| repo.set_completed(a, true))?;
        repo.update_task(TaskId(999), "Nope", "")
    });
    assert!(matches!(result, Err(Error::NoSuchTask(_))));
    let task = repo.task(a).unwrap();
    assert_eq!(task.name, "A");
    assert!(!task.is_completed());
    // A failed inner step can be caught without losing the outer one.
    repo.transaction(|| {
        repo.update_task(a, "Kept", "")?;
        let _ = repo.transaction(|| -> Result<(), Error> {
            repo.set_completed(a, true)?;
            Err(Error::EmptyName)
        });
        Ok::<_, Error>(())
    })
    .unwrap();
    let task = repo.task(a).unwrap();
    assert_eq!(task.name, "Kept");
    assert!(!task.is_completed());
}

#[test]
fn sees_changes_from_other_connections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let app = Repository::open(&path).unwrap();
    let cli = Repository::open(&path).unwrap();
    assert!(!app.has_external_changes().unwrap());
    add(&app, "Own change", None);
    assert!(
        !app.has_external_changes().unwrap(),
        "its own writes don't count"
    );
    add(&cli, "From the CLI", None);
    assert!(app.has_external_changes().unwrap());
    assert!(!app.has_external_changes().unwrap());
    assert_eq!(app.tasks().unwrap().len(), 2);
}

#[test]
fn concurrent_writers_wait_for_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    Repository::open(&path).unwrap();
    let threads: Vec<_> = (0..4)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                let repo = Repository::open(&path).unwrap();
                for i in 0..25 {
                    repo.transaction(|| {
                        add(&repo, &format!("{n}-{i}"), None);
                        std::thread::sleep(Duration::from_micros(200));
                        Ok::<_, Error>(())
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(Repository::open(&path).unwrap().tasks().unwrap().len(), 100);
}

#[test]
fn backups_keep_two_days_and_restore_replaces_everything() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(dir.path().join("db.sqlite")).unwrap();
    let folder_path = dir.path().join("OneDrive");
    std::fs::create_dir(&folder_path).unwrap();
    let folder = BackupFolder::new(&folder_path);
    add(&repo, "Before", None);
    for day in [1, 2, 3] {
        let now = date(2026, 1, day).at(12, 0, 0, 0).in_tz("UTC").unwrap();
        folder.back_up(&repo, &now).unwrap();
    }
    let names: Vec<_> = folder.backups().iter().map(|b| b.title()).collect();
    assert_eq!(names, ["Ostracon 2026-01-03", "Ostracon 2026-01-02"]);
    let leftovers: Vec<_> = std::fs::read_dir(&folder_path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with("Ostracon 2026"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "nothing half-written is left in the folder: {leftovers:?}"
    );

    let latest = folder.backups()[0].path.clone();
    add(&repo, "After", None);
    let other = Repository::open(dir.path().join("db.sqlite")).unwrap();
    let now = date(2026, 1, 3).at(13, 0, 0, 0).in_tz("UTC").unwrap();
    let saved = folder.back_up_before(&repo, "restore", &now).unwrap();
    assert_eq!(saved.title(), "Ostracon before restore 2026-01-03 13.00.00");
    repo.restore_from(&latest).unwrap();
    let names: Vec<_> = repo.tasks().unwrap().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["Before"]);
    assert!(
        other.has_external_changes().unwrap(),
        "other connections see the restore"
    );
    assert_eq!(
        folder.backups().len(),
        2,
        "one-off backups don't count against the daily ones"
    );

    let junk = dir.path().join("junk.sqlite");
    std::fs::write(&junk, b"not a database").unwrap();
    assert!(matches!(
        repo.restore_from(&junk),
        Err(Error::NotADatabase(_))
    ));
    assert_eq!(repo.tasks().unwrap().len(), 1);
}

#[test]
fn backup_policy_throttles_and_skips_unchanged() {
    let t = |s: i64| jiff::Timestamp::from_second(s).unwrap();
    let mut policy = BackupPolicy::default();
    assert!(policy.is_due(t(100), Some(t(50)), true));
    policy.last_attempt = Some(t(100));
    policy.last_backup = Some(t(100));
    assert!(
        !policy.is_due(t(130), Some(t(120)), true),
        "within a minute of the last attempt"
    );
    assert!(
        policy.is_due(t(130), Some(t(120)), false),
        "unless not throttled, as on quit"
    );
    assert!(policy.is_due(t(161), Some(t(120)), true));
    assert!(
        !policy.is_due(t(161), Some(t(90)), true),
        "nothing changed since"
    );
}

#[test]
fn schedule_includes_due_times_and_skips_the_past_and_completed() {
    let repo = Repository::in_memory().unwrap();
    let tz = jiff::tz::TimeZone::UTC;
    let due = Due::at(date(2026, 1, 10), time(9, 0, 0, 0));
    let a = repo
        .add_task(NewTask {
            name: "A".into(),
            due: Some(due),
            reminders: vec![
                Reminder::Before { minutes: 60 },
                Reminder::At("2026-01-01T00:00:00Z".parse().unwrap()),
            ],
            ..Default::default()
        })
        .unwrap();
    let b = repo
        .add_task(NewTask {
            name: "B".into(),
            due: Some(due),
            ..Default::default()
        })
        .unwrap();
    repo.set_completed(b.id, true).unwrap();
    repo.add_task(NewTask {
        name: "Untimed".into(),
        due: Some(Due::day(date(2026, 1, 10))),
        reminders: vec![Reminder::Before { minutes: 5 }],
        ..Default::default()
    })
    .unwrap();
    let now = "2026-01-05T00:00:00Z".parse().unwrap();
    let upcoming = schedule::upcoming(&repo.tasks().unwrap(), now, &tz, 64);
    let times: Vec<String> = upcoming.iter().map(|s| s.fire_at.to_string()).collect();
    assert_eq!(times, ["2026-01-10T08:00:00Z", "2026-01-10T09:00:00Z"]);
    assert!(upcoming.iter().all(|s| s.task_id == a.id));
    assert_eq!(
        upcoming[0].identifier(),
        format!("task-{}-{}", a.id, upcoming[0].fire_at.as_second())
    );
}
