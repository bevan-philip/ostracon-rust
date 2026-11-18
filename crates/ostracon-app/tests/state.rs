use jiff::ZonedDateTime;
use jiff::civil::{date, time};
use ostracon_app::state::{AppState, Composer, Dialog, ListItem, ProjectDraft, Selection, Undo};
use ostracon_core::quickadd::Draft;
use ostracon_core::{Due, NewTask, ProjectColor, ProjectId, Repository, TaskId};

fn now() -> ZonedDateTime {
    date(2026, 1, 14)
        .at(10, 30, 0, 0)
        .in_tz("Europe/London")
        .unwrap()
}

fn state() -> AppState {
    AppState::with_clock(Repository::in_memory().unwrap(), None, now)
}

/// Adds tasks described as indented lines ("A", " B" for a subtask of A, ...) to the current project.
fn add_outline(state: &mut AppState, outline: &str) -> Vec<TaskId> {
    let mut stack: Vec<TaskId> = Vec::new();
    let mut ids = Vec::new();
    for line in outline.lines().filter(|l| !l.trim().is_empty()) {
        let depth = line.len() - line.trim_start().len();
        stack.truncate(depth);
        let task = state
            .repository()
            .add_task(NewTask {
                name: line.trim().into(),
                project_id: Some(state.project),
                parent_id: stack.last().copied(),
                ..Default::default()
            })
            .unwrap();
        stack.push(task.id);
        ids.push(task.id);
    }
    state.reload();
    ids
}

fn shown(state: &AppState) -> Vec<String> {
    state
        .rows()
        .iter()
        .map(|r| format!("{}{}", " ".repeat(r.depth), r.task.name))
        .collect()
}

fn typed(text: &str) -> Draft {
    Draft::from_text(text, false)
}

#[test]
fn starts_on_the_inbox() {
    let state = state();
    assert_eq!(state.current_project().unwrap().name, "Inbox");
    assert!(state.rows().is_empty());
}

#[test]
fn adding_keeps_the_composer_open_and_files_by_project() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    state.reload();
    state.start_adding();
    let inbox = state.project;
    assert!(state.submit_composer(&typed("Write report tomorrow 5pm"), "", inbox, None, &[]));
    assert_eq!(
        state.composer,
        Composer::Adding { parent: None },
        "Return leaves the composer open for the next"
    );
    let task = &state.rows()[0].task;
    assert_eq!(task.name, "Write report");
    assert_eq!(
        task.due,
        Some(Due::at(date(2026, 1, 15), time(17, 0, 0, 0)))
    );
    assert!(state.submit_composer(&typed("Expenses #Work"), "", inbox, None, &[]));
    assert_eq!(state.toast.as_ref().unwrap().message, "Task added to Work");
    assert_eq!(state.open_count(work.id), 1);
    assert!(
        !state.submit_composer(&typed("   "), "", inbox, None, &[]),
        "empty names aren't saved"
    );
    // The composer sits at the end of the list.
    assert!(matches!(
        state.list_items().last(),
        Some(ListItem::Composer { depth: 0, .. })
    ));
}

#[test]
fn editing_never_rereads_the_name() {
    let mut state = state();
    let ids = add_outline(&mut state, "Plan monday standup");
    state.start_editing(Some(ids[0]));
    assert_eq!(state.composer, Composer::Editing { task: ids[0] });
    let draft = Draft::from_text("Plan monday standup", true);
    let project = state.project;
    assert!(state.submit_composer(&draft, "Notes", project, None, &[]));
    assert_eq!(state.composer, Composer::Closed);
    let task = state.task(ids[0]).unwrap();
    assert_eq!(
        (task.name.as_str(), task.description.as_str(), task.due),
        ("Plan monday standup", "Notes", None)
    );
}

#[test]
fn subtask_composer_sits_after_the_parents_subtasks() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n B\nC");
    state.select_only(ids[0]);
    state.start_adding_subtask(None);
    let items = state.list_items();
    assert!(
        matches!(items[2], ListItem::Composer { depth: 1, .. }),
        "{items:?}"
    );
    let project = state.project;
    assert!(state.submit_composer(&typed("D"), "", project, None, &[]));
    assert_eq!(shown(&state), ["A", " B", " D", "C"]);
}

#[test]
fn keyboard_selection() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB\nC\nD");
    state.move_selection(1);
    assert_eq!(state.selection.anchor(), Some(ids[0]));
    state.move_selection(1);
    state.extend_selection_by(1);
    state.extend_selection_by(1);
    assert_eq!(state.selection.len(), 3);
    assert_eq!(state.selection.anchor(), Some(ids[1]));
    state.extend_selection_by(-1);
    assert_eq!(state.selection.len(), 2);
    state.toggle_selection(ids[0]);
    assert_eq!(state.acting_on(None), vec![ids[0], ids[1], ids[2]]);
    assert_eq!(
        state.acting_on(Some(ids[3])),
        vec![ids[3]],
        "a task outside the selection acts alone"
    );
    state.toggle_selection(ids[0]);
    assert_eq!(
        state.selection.anchor(),
        Some(ids[1]),
        "the keyboard moves to the nearest selected task"
    );
    state.move_selection(-10);
    assert_eq!(state.selection, Selection::single(ids[0]));
}

#[test]
fn completing_moves_the_selection_and_can_be_undone() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n A1\nB");
    state.select_only(ids[0]);
    state.toggle_completed(None);
    assert_eq!(shown(&state), ["B"]);
    assert_eq!(state.selection.anchor(), Some(ids[2]));
    let toast = state.toast.clone().unwrap();
    assert_eq!(toast.message, "1 task completed");
    assert_eq!(
        toast.undo,
        Some(Undo::Reopen(vec![ids[0], ids[1]])),
        "undo reopens the subtasks it closed too"
    );
    state.undo_toast();
    assert_eq!(shown(&state), ["A", " A1", "B"]);
    assert!(state.toast.is_none());
}

#[test]
fn parents_show_hidden_completed_subtasks() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n A1\n A2");
    state.toggle_completed(Some(ids[1]));
    let parent = &state.rows()[0];
    assert_eq!(
        (
            parent.subtask_count,
            parent.completed_subtask_count,
            parent.hidden_completed_count()
        ),
        (2, 1, 1)
    );
    state.set_show_completed(true);
    assert_eq!(shown(&state), ["A", " A1", " A2"]);
}

#[test]
fn collapsing_hands_the_selection_to_the_parent() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n B\n  C");
    state.select_only(ids[2]);
    state.set_collapsed(ids[0], true);
    assert_eq!(state.selection, Selection::single(ids[0]));
    state.expand_selected();
    assert_eq!(shown(&state), ["A", " B", "  C"]);
    state.select_only(ids[2]);
    state.collapse_or_select_parent();
    assert_eq!(
        state.selection.anchor(),
        Some(ids[1]),
        "← on a leaf goes to its parent"
    );
    state.collapse_or_select_parent();
    assert_eq!(
        shown(&state),
        ["A", " B"],
        "← on an expanded parent collapses it"
    );
}

#[test]
fn outlining_through_the_state() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.select_only(ids[1]);
    state.indent(None);
    assert_eq!(shown(&state), ["A", " B"]);
    state.outdent(None);
    state.move_by(None, -1);
    assert_eq!(shown(&state), ["B", "A"]);
}

#[test]
fn deleting_asks_then_reconciles() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n A1\nB");
    state.select_only(ids[0]);
    state.open_overview(ids[0]);
    state.request_delete(None);
    assert_eq!(state.dialog, Dialog::ConfirmDeleteTasks(vec![ids[0]]));
    state.confirm_dialog();
    assert_eq!(shown(&state), ["B"]);
    assert_eq!(state.dialog, Dialog::None);
    assert_eq!(
        state.overview, None,
        "the overview closes when its task goes"
    );
    assert_eq!(state.selection.anchor(), Some(ids[2]));
}

#[test]
fn changes_from_elsewhere_are_reconciled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let mut state = AppState::with_clock(Repository::open(&path).unwrap(), None, now);
    let ids = add_outline(&mut state, "A\n B\nC");
    state.select_only(ids[1]);
    state.start_editing(None);
    state.request_delete(Some(ids[2]));
    let cli = Repository::open(&path).unwrap();
    cli.delete_task(ids[0]).unwrap();
    assert!(state.reload_if_changed_elsewhere());
    assert_eq!(shown(&state), ["C"]);
    assert_eq!(state.selection, Selection::Empty);
    assert_eq!(
        state.composer,
        Composer::Closed,
        "an editor for a vanished task closes"
    );
    assert_eq!(
        state.dialog,
        Dialog::ConfirmDeleteTasks(vec![ids[2]]),
        "a dialog for a task still there stays"
    );
    cli.delete_task(ids[2]).unwrap();
    state.reload_if_changed_elsewhere();
    assert_eq!(state.dialog, Dialog::None);
    assert!(!state.reload_if_changed_elsewhere());
}

#[test]
fn a_deleted_project_returns_to_the_inbox() {
    let mut state = state();
    state.start_new_project();
    assert!(!state.save_project(&ProjectDraft {
        project: None,
        name: " ".into(),
        color: ProjectColor::Red
    }));
    assert!(state.save_project(&ProjectDraft {
        project: None,
        name: "Work".into(),
        color: ProjectColor::Red
    }));
    assert_eq!(
        state.current_project().unwrap().name,
        "Work",
        "a new project is shown"
    );
    let work = state.project;
    state.request_delete_project(work);
    state.confirm_dialog();
    assert_eq!(state.current_project().unwrap().name, "Inbox");
    let inbox = state.project;
    state.request_delete_project(inbox);
    assert_eq!(state.dialog, Dialog::None, "the Inbox can't be deleted");
}

#[test]
fn moving_a_selection_to_another_project() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    state.reload();
    let ids = add_outline(&mut state, "A\n A1\nB\nC");
    state.select_only(ids[0]);
    state.toggle_selection(ids[1]);
    state.toggle_selection(ids[2]);
    assert!(state.move_to_project(&state.acting_on(None), work.id));
    assert_eq!(shown(&state), ["C"]);
    assert_eq!(
        state.toast.as_ref().unwrap().message,
        "2 tasks moved to Work"
    );
    assert_eq!(state.selection.anchor(), Some(ids[3]));
    state.select_project(work.id);
    assert_eq!(shown(&state), ["A", " A1", "B"]);
}

#[test]
fn rescheduling_a_selection_keeps_times() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.set_due_of_only(ids[0], Some(Due::at(date(2026, 1, 20), time(9, 0, 0, 0))));
    state.select_only(ids[0]);
    state.toggle_selection(ids[1]);
    state.set_due(None, Some(Due::day(date(2026, 1, 22))));
    assert_eq!(
        state.task(ids[0]).unwrap().due,
        Some(Due::at(date(2026, 1, 22), time(9, 0, 0, 0)))
    );
    assert_eq!(
        state.task(ids[1]).unwrap().due,
        Some(Due::day(date(2026, 1, 22)))
    );
}

#[test]
fn revealing_a_task_from_a_notification() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    let parent = state
        .repository()
        .add_task(NewTask {
            name: "P".into(),
            project_id: Some(work.id),
            ..Default::default()
        })
        .unwrap();
    let child = state
        .repository()
        .add_task(NewTask {
            name: "C".into(),
            parent_id: Some(parent.id),
            ..Default::default()
        })
        .unwrap();
    state.repository().set_collapsed(parent.id, true).unwrap();
    state.reload();
    state.reveal(child.id);
    assert_eq!(state.project, work.id);
    assert_eq!(state.selection.anchor(), Some(child.id));
    state.complete_from_notification(child.id);
    assert!(state.task(child.id).unwrap().is_completed());
}

#[test]
fn notifications_for_reminders_and_due_times() {
    let mut state = state();
    let project = state.project;
    state.start_adding();
    state.submit_composer(
        &typed("Call bank friday 3pm !30min"),
        "",
        project,
        None,
        &[],
    );
    let notifications = state.notifications();
    assert_eq!(notifications.len(), 2);
    assert_eq!(notifications[0].title, "Call bank");
    assert_eq!(
        notifications[0].body, "Due Today 15:00 · Inbox",
        "phrased as at the moment it fires"
    );
    assert!(notifications[0].fire_at < notifications[1].fire_at);
}

#[test]
fn backups_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("OneDrive");
    std::fs::create_dir(&backups).unwrap();
    let settings = dir.path().join("settings.json");
    let repo = Repository::open(dir.path().join("db.sqlite")).unwrap();
    let mut state = AppState::with_clock(repo, Some(settings.clone()), now);
    add_outline(&mut state, "Keep");
    state.set_backup_folder(Some(backups.clone()));
    assert_eq!(state.backup.error, None);
    let backup = state.backup.folder.as_ref().unwrap().backups()[0]
        .path
        .clone();
    add_outline(&mut state, "Lose");
    state.restore(&backup);
    assert_eq!(shown(&state), ["Keep"]);
    assert_eq!(
        state.toast.as_ref().unwrap().message,
        "Restored Ostracon 2026-01-14"
    );
    let files = std::fs::read_dir(&backups).unwrap().count();
    assert_eq!(files, 2, "the pre-restore copy sits beside the backup");
    // The folder is remembered.
    let reopened = AppState::with_clock(
        Repository::open(dir.path().join("db.sqlite")).unwrap(),
        Some(settings),
        now,
    );
    assert_eq!(reopened.backup.folder.as_ref().unwrap().path, backups);
}

#[test]
fn failed_writes_leave_state_alone() {
    let mut state = state();
    let ids = add_outline(&mut state, "A");
    state.start_editing(Some(ids[0]));
    let project = ProjectId(999);
    assert!(!state.submit_composer(&Draft::from_text("A2", true), "", project, None, &[]));
    assert_eq!(
        state.composer,
        Composer::Editing { task: ids[0] },
        "the editor stays open on failure"
    );
    assert_eq!(
        state.task(ids[0]).unwrap().name,
        "A",
        "the rename was rolled back with the move"
    );
    assert!(state.error.is_some());
}
