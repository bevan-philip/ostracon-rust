use jiff::ZonedDateTime;
use jiff::civil::{date, time};
use ostracon_app::state::{
    AppState, Composer, Dialog, DropTarget, ListItem, ProjectDraft, Selection, Undo, View,
};
use ostracon_core::quickadd::Draft;
use ostracon_core::settings::{Appearance, Scheme};
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
                project_id: Some(state.home_project()),
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
    let inbox = state.home_project();
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
    // The composer sits at the end of the outline.
    assert!(matches!(
        state.list_items().outline.last(),
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
    let project = state.home_project();
    assert!(state.submit_composer(&draft, "Notes", project, None, &[]));
    assert_eq!(state.composer, Composer::Closed);
    let task = state.task(ids[0]).unwrap();
    assert_eq!(
        (task.name.as_str(), task.description.as_str(), task.due),
        ("Plan monday standup", "Notes", None)
    );
}

#[test]
fn the_overview_name_reads_what_is_typed_into_it() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    state.reload();
    let ids = add_outline(&mut state, "Plan monday standup\nB");
    state.open_overview(ids[0]);

    // The name loads as literal, so saving it as it is changes nothing.
    let mut draft = Draft::from_text("Plan monday standup", true);
    assert!(state.save_overview(ids[0], &draft, ""));
    assert_eq!(state.task(ids[0]).unwrap().due, None);

    let end = draft.len();
    draft.edit(end..end, " tomorrow #Work");
    assert!(state.save_overview(ids[0], &draft, "Notes"));
    let task = state.task(ids[0]).unwrap();
    assert_eq!(
        (task.name.as_str(), task.description.as_str()),
        ("Plan monday standup", "Notes")
    );
    assert_eq!(task.due, Some(Due::day(date(2026, 1, 15))));
    assert_eq!(task.project_id, work.id);
    assert_eq!(state.overview, Some(ids[0]));
    assert_eq!(shown(&state), ["Plan monday standup", "B"]);

    // A name that is only a project keeps the old name.
    assert!(state.save_overview(ids[0], &typed("#Inbox "), "Notes"));
    let task = state.task(ids[0]).unwrap();
    assert_eq!(task.name, "Plan monday standup");
    assert_eq!(task.project_id, state.home_project());
}

#[test]
fn subtask_composer_sits_after_the_parents_subtasks() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n B\nC");
    state.select_only(ids[0]);
    state.start_adding_subtask(None);
    let items = state.list_items();
    assert!(
        matches!(items.outline[2], ListItem::Composer { depth: 1, .. }),
        "{items:?}"
    );
    let project = state.home_project();
    assert!(state.submit_composer(&typed("D"), "", project, None, &[]));
    assert_eq!(shown(&state), ["A", " B", " D", "C"]);
}

#[test]
fn the_overview_adds_subtasks_with_the_full_composer() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n B\nC");
    state.set_collapsed(ids[0], true);
    state.open_overview(ids[0]);
    state.start_adding_subtask_in_overview();
    assert_eq!(
        state.composer,
        Composer::AddingInOverview { parent: ids[0] }
    );
    assert!(
        !state
            .list_items()
            .all()
            .any(|i| matches!(i, ListItem::Composer { .. })),
        "the composer is in the overview, not the list"
    );

    let project = state.home_project();
    assert!(state.submit_composer(&typed("D tomorrow"), "Notes", project, None, &[]));
    assert_eq!(shown(&state), ["A", " B", " D", "C"], "the parent opens");
    let added = state.subtasks(ids[0])[1];
    assert_eq!(
        (added.name.as_str(), added.description.as_str()),
        ("D", "Notes")
    );
    assert!(added.due.is_some());
    assert_eq!(state.overview, Some(ids[0]));
    assert_eq!(
        state.composer,
        Composer::AddingInOverview { parent: ids[0] },
        "it stays open for the next subtask"
    );

    // Stepping to another task, or closing the overview, closes it.
    state.open_overview(ids[2]);
    assert_eq!(state.composer, Composer::Closed);
    state.start_adding_subtask_in_overview();
    state.close_overview();
    assert_eq!(state.composer, Composer::Closed);
}

#[test]
fn dismissing_the_overview_clears_the_selection() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.select_only(ids[0]);
    state.open_overview(ids[0]);
    state.dismiss_overview();
    assert_eq!(state.overview, None);
    assert_eq!(state.selection, Selection::Empty);
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
    assert_eq!(
        shown(&state),
        ["A", " A2", " A1"],
        "completed subtasks follow their open siblings"
    );
}

#[test]
fn completed_tasks_stay_out_of_the_outline() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.toggle_completed(Some(ids[0]));
    state.set_show_completed(true);
    assert_eq!(shown(&state), ["B", "A"]);
    assert_eq!(state.outline().len(), 1);

    state.start_adding();
    let items = state.list_items();
    assert!(
        matches!(items.outline[1], ListItem::Composer { .. }),
        "a new task is added at the end of the outline, above the completed ones"
    );
    assert!(
        matches!(&items.completed[..], [ListItem::Task { row, index: 1 }] if row.id() == ids[0]),
        "{items:?}"
    );

    let target = state.drop_target(&[ids[1]], 2, 0);
    assert_eq!(
        (target.before, target.parent),
        (None, None),
        "nothing can be dropped among the completed tasks"
    );
}

#[test]
fn the_composer_stays_with_a_completed_task() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.toggle_completed(Some(ids[1]));
    state.set_show_completed(true);

    state.start_editing(Some(ids[1]));
    let items = state.list_items();
    assert!(
        matches!(&items.completed[..], [ListItem::Composer { depth: 0, .. }]),
        "{items:?}"
    );

    state.close_composer();
    state.start_adding_subtask(Some(ids[1]));
    let items = state.list_items();
    assert_eq!(items.outline.len(), 1);
    assert!(
        matches!(
            &items.completed[..],
            [ListItem::Task { .. }, ListItem::Composer { depth: 1, .. }]
        ),
        "{items:?}"
    );
}

#[test]
fn nothing_is_dropped_among_completed_subtasks() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n A1\n A2\nB\nC");
    state.toggle_completed(Some(ids[1]));
    state.set_show_completed(true);
    assert_eq!(shown(&state), ["A", " A2", " A1", "B", "C"]);
    assert_eq!(state.outline().len(), 5);
    // C dropped below A1, one level in, goes after A2, the last open subtask.
    let target = state.drop_target(&[ids[4]], 3, 1);
    assert_eq!(
        (target.before, target.parent, target.after),
        (Some(ids[3]), Some(ids[0]), Some(ids[2]))
    );
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
fn drop_targets_follow_the_outline() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\n A1\n  A2\nB\nC");
    let [a, a1, a2, b, c] = ids[..] else {
        unreachable!()
    };
    let target = |state: &AppState, dragged, gap, depth| {
        let t: DropTarget = state.drop_target(&[dragged], gap, depth);
        (t.before, t.depth, t.parent, t.after)
    };
    // Below A2 and above B: anywhere from the top level to a subtask of A2.
    assert_eq!(target(&state, c, 3, -1), (Some(b), 0, None, Some(a)));
    assert_eq!(target(&state, c, 3, 1), (Some(b), 1, Some(a), Some(a1)));
    assert_eq!(target(&state, c, 3, 9), (Some(b), 3, Some(a2), None));
    // Above A1 it must be a subtask of A, as A1 is.
    assert_eq!(target(&state, c, 1, 0), (Some(a1), 1, Some(a), None));
    assert_eq!(target(&state, b, 0, 4), (Some(a), 0, None, None));
    // Dragging A, its own subtree is left out: the gaps inside it are the gap before B.
    assert_eq!(target(&state, a, 2, 1), (Some(b), 0, None, None));
    assert_eq!(target(&state, a, 5, 0), (None, 0, None, Some(c)));

    state.drop_tasks(&[c], state.drop_target(&[c], 3, 1));
    assert_eq!(shown(&state), ["A", " A1", "  A2", " C", "B"]);
    state.drop_tasks(&[a], state.drop_target(&[a], 5, 0));
    assert_eq!(shown(&state), ["B", "A", " A1", "  A2", " C"]);
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
fn clicking_a_task_asks_before_discarding_the_composer() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.start_adding();
    state.request_discard_to_open(ids[1]);
    assert_eq!(state.dialog, Dialog::ConfirmDiscard(Some(ids[1])));
    state.cancel_dialog();
    assert_eq!(state.composer, Composer::Adding { parent: None });
    assert_eq!(state.overview, None);

    state.request_discard_to_open(ids[1]);
    state.confirm_dialog();
    assert_eq!(state.dialog, Dialog::None);
    assert_eq!(state.composer, Composer::Closed);
    assert_eq!(state.overview, Some(ids[1]));
    assert_eq!(state.selection.anchor(), Some(ids[1]));

    state.close_overview();
    state.start_editing(Some(ids[0]));
    state.request_discard_to_open(ids[1]);
    state.repository().delete_task(ids[1]).unwrap();
    state.reload();
    assert_eq!(state.dialog, Dialog::None, "the task to open is gone");
    assert_eq!(state.composer, Composer::Editing { task: ids[0] });
}

#[test]
fn leaving_the_overview_editor_asks_before_discarding() {
    let mut state = state();
    let ids = add_outline(&mut state, "A\nB");
    state.open_overview(ids[0]);
    state.request_discard_to_leave_overview(None);
    assert_eq!(state.dialog, Dialog::None, "nothing to discard");

    state.start_editing_overview();
    state.request_discard_to_leave_overview(Some(ids[1]));
    assert_eq!(state.dialog, Dialog::ConfirmDiscard(Some(ids[1])));
    state.cancel_dialog();
    assert!(state.overview_editing, "back to the edit");
    state.request_discard_to_leave_overview(Some(ids[1]));
    state.confirm_dialog();
    assert_eq!(state.overview, Some(ids[1]));
    assert!(!state.overview_editing);

    state.start_editing_overview();
    state.request_discard_to_leave_overview(None);
    state.confirm_dialog();
    assert_eq!(state.overview, None);
    assert!(!state.overview_editing);

    // Saving closes the editor; a dialog whose task goes is dropped.
    state.open_overview(ids[0]);
    state.start_editing_overview();
    assert!(state.save_overview(ids[0], &Draft::from_text("A2", true), ""));
    assert!(!state.overview_editing);
    state.start_editing_overview();
    state.request_discard_to_leave_overview(Some(ids[1]));
    state.repository().delete_task(ids[1]).unwrap();
    state.reload();
    assert_eq!(state.dialog, Dialog::None);
    assert!(state.overview_editing);
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
    let work = state.home_project();
    state.request_delete_project(work);
    state.confirm_dialog();
    assert_eq!(state.current_project().unwrap().name, "Inbox");
    let inbox = state.home_project();
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
fn moving_from_the_overview_keeps_the_task_in_the_list_until_it_closes() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    let home = state
        .repository()
        .create_project("Home", ProjectColor::Blue)
        .unwrap();
    state.reload();
    let ids = add_outline(&mut state, "A\nB\n B1\n  B2\nC");
    state.open_overview(ids[1]);
    state.move_task_to(ids[1], work.id);
    assert_eq!(state.overview, Some(ids[1]), "the overview stays open");
    assert_eq!(state.task(ids[1]).unwrap().project_id, work.id);
    assert_eq!(state.task(ids[2]).unwrap().project_id, work.id);
    assert_eq!(shown(&state), ["A", "B", " B1", "  B2", "C"]);
    assert_eq!(state.selection.anchor(), Some(ids[1]));
    assert_eq!(state.toast.as_ref().unwrap().message, "Task moved to Work");

    // Stepping away and back finds it where it was.
    state.open_overview(state.overview_neighbour(1).unwrap());
    assert_eq!(state.overview, Some(ids[2]));
    state.open_overview(state.overview_neighbour(-1).unwrap());
    assert_eq!(state.overview, Some(ids[1]));

    // So does a subtask moved on its own, at its old depth.
    state.open_overview(ids[3]);
    state.move_task_to(ids[3], home.id);
    assert_eq!(state.task(ids[3]).unwrap().parent_id, None);
    assert_eq!(shown(&state), ["A", "B", " B1", "  B2", "C"]);

    state.open_overview(ids[1]);
    state.close_overview();
    assert_eq!(shown(&state), ["A", "C"]);
    assert_eq!(
        state.selection.anchor(),
        Some(ids[4]),
        "the selection moves on as it would after any move"
    );
    state.select_project(work.id);
    assert_eq!(shown(&state), ["B", " B1"]);
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
    assert_eq!(state.view, View::Project(work.id));
    assert_eq!(state.selection.anchor(), Some(child.id));
    state.complete_from_notification(child.id);
    assert!(state.task(child.id).unwrap().is_completed());
}

#[test]
fn notifications_for_reminders_and_due_times() {
    let mut state = state();
    let project = state.home_project();
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
fn appearance_and_scheme_are_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let mut saved = AppState::with_clock(
        Repository::in_memory().unwrap(),
        Some(settings.clone()),
        now,
    );
    assert_eq!(saved.appearance, Appearance::System);
    assert_eq!(saved.scheme, Scheme::Classic);
    saved.set_appearance(Appearance::Dark);
    saved.set_scheme(Scheme::Flexoki);
    assert_eq!(saved.appearance, Appearance::Dark);
    assert_eq!(saved.scheme, Scheme::Flexoki);
    let reopened = AppState::with_clock(Repository::in_memory().unwrap(), Some(settings), now);
    assert_eq!(reopened.appearance, Appearance::Dark);
    assert_eq!(reopened.scheme, Scheme::Flexoki);

    // Without a settings file they last for the session.
    let mut state = state();
    state.set_appearance(Appearance::Light);
    state.set_scheme(Scheme::Flexoki);
    assert_eq!(state.appearance, Appearance::Light);
    assert_eq!(state.scheme, Scheme::Flexoki);
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

/// Adds a task due `due` to `project`, or the Inbox.
fn add_due(
    state: &mut AppState,
    name: &str,
    project: Option<ProjectId>,
    due: Option<Due>,
) -> TaskId {
    let id = state
        .repository()
        .add_task(NewTask {
            name: name.into(),
            project_id: project,
            due,
            ..Default::default()
        })
        .unwrap()
        .id;
    state.reload();
    id
}

fn names(items: &[ListItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            ListItem::Task { row, .. } => row.task.name.clone(),
            ListItem::Composer { .. } => "(composer)".into(),
        })
        .collect()
}

#[test]
fn today_lists_what_is_due_today_and_overdue_from_every_project() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    let (yesterday, today, tomorrow) = (date(2026, 1, 13), date(2026, 1, 14), date(2026, 1, 15));
    add_due(&mut state, "Pay rent", None, Some(Due::day(yesterday)));
    let report = add_due(&mut state, "Report", Some(work.id), Some(Due::day(today)));
    add_due(
        &mut state,
        "Standup",
        Some(work.id),
        Some(Due::at(today, time(9, 0, 0, 0))),
    );
    add_due(&mut state, "Later", Some(work.id), Some(Due::day(tomorrow)));
    add_due(&mut state, "Someday", None, None);
    // A subtask due today is listed on its own, though its parent isn't due.
    let parent = add_due(&mut state, "Trip", None, None);
    state
        .repository()
        .add_task(NewTask {
            name: "Book train".into(),
            parent_id: Some(parent),
            due: Some(Due::day(today)),
            ..Default::default()
        })
        .unwrap();
    state.reload();

    state.select_today();
    assert_eq!(state.view, View::Today);
    assert!(state.current_project().is_none());
    let items = state.list_items();
    assert_eq!(names(&items.overdue), ["Pay rent"]);
    assert_eq!(names(&items.outline), ["Standup", "Book train", "Report"]);
    assert_eq!(state.today_count(), 4);

    // Completing one hands the selection on and, with completed tasks shown, lists it below.
    state.select_only(report);
    state.toggle_completed(None);
    assert_eq!(
        names(&state.list_items().outline),
        ["Standup", "Book train"]
    );
    assert_eq!(state.today_count(), 3);
    state.set_show_completed(true);
    assert_eq!(names(&state.list_items().completed), ["Report"]);
}

#[test]
fn adding_in_today_files_to_the_inbox_due_today() {
    let mut state = state();
    state.select_today();
    let inbox = state.home_project();
    assert_eq!(state.project_by_id(inbox).unwrap().name, "Inbox");
    let due = state.default_due();
    assert_eq!(due, Some(Due::day(date(2026, 1, 14))));

    state.start_adding();
    assert!(state.submit_composer(&typed("Call mum"), "", inbox, due, &[]));
    assert_eq!(
        names(&state.list_items().outline),
        ["Call mum", "(composer)"]
    );
    assert!(state.toast.is_none());

    // A date typed in the name wins, and as the task won't show here, a toast says where it went.
    assert!(state.submit_composer(&typed("Plan trip tomorrow"), "", inbox, due, &[]));
    assert_eq!(
        names(&state.list_items().outline),
        ["Call mum", "(composer)"]
    );
    assert_eq!(state.toast.as_ref().unwrap().message, "Task added to Inbox");

    state.select_project(inbox);
    assert_eq!(state.default_due(), None, "only Today dates new tasks");
}

#[test]
fn today_has_no_outline_to_rearrange() {
    let mut state = state();
    let today = Some(Due::day(date(2026, 1, 14)));
    let a = add_due(&mut state, "A", None, today);
    let b = add_due(&mut state, "B", None, today);
    state.select_today();
    state.select_only(b);
    state.indent(None);
    state.move_by(None, -1);
    let target = state.drop_target(&[b], 0, 0);
    state.drop_tasks(&[b], target);
    assert_eq!(state.task(b).unwrap().parent_id, None);
    assert_eq!(shown(&state), ["A", "B"]);
    assert!(state.task(a).unwrap().position < state.task(b).unwrap().position);
}

#[test]
fn the_overview_stays_open_on_a_task_rescheduled_out_of_today() {
    let mut state = state();
    let id = add_due(&mut state, "A", None, Some(Due::day(date(2026, 1, 14))));
    state.select_today();
    state.open_overview(id);
    state.set_due_of_only(id, Some(Due::day(date(2026, 1, 15))));
    assert_eq!(state.overview, Some(id));
    assert!(!state.is_in_list(id));
    state.dismiss_overview();
    assert!(state.rows().is_empty());
}

#[test]
fn dropping_on_today_schedules_tasks_for_today_at_their_times() {
    let mut state = state();
    let a = add_due(
        &mut state,
        "A",
        None,
        Some(Due::at(date(2026, 1, 20), time(9, 0, 0, 0))),
    );
    let b = add_due(&mut state, "B", None, None);
    state.schedule_for_today(&[a, b]);
    assert_eq!(
        state.task(a).unwrap().due,
        Some(Due::at(date(2026, 1, 14), time(9, 0, 0, 0)))
    );
    assert_eq!(
        state.task(b).unwrap().due,
        Some(Due::day(date(2026, 1, 14)))
    );
}

#[test]
fn today_moves_on_at_midnight() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let clock = Rc::new(RefCell::new(now()));
    let reading = clock.clone();
    let mut state = AppState::with_clock(Repository::in_memory().unwrap(), None, move || {
        reading.borrow().clone()
    });
    add_due(&mut state, "A", None, Some(Due::day(date(2026, 1, 15))));
    state.select_today();
    assert!(state.rows().is_empty());
    assert!(!state.refresh_if_day_changed());

    *clock.borrow_mut() = date(2026, 1, 15)
        .at(0, 0, 1, 0)
        .in_tz("Europe/London")
        .unwrap();
    assert!(state.refresh_if_day_changed());
    assert_eq!(shown(&state), ["A"]);
    assert!(!state.refresh_if_day_changed());
}

#[test]
fn revealing_a_task_listed_in_today_stays_in_today() {
    let mut state = state();
    let work = state
        .repository()
        .create_project("Work", ProjectColor::Red)
        .unwrap();
    let id = add_due(
        &mut state,
        "A",
        Some(work.id),
        Some(Due::day(date(2026, 1, 14))),
    );
    state.select_today();
    state.reveal(id);
    assert_eq!(state.view, View::Today);
    assert_eq!(state.selection.anchor(), Some(id));
}

#[test]
fn switching_lists_closes_the_overviews_editor() {
    let mut state = state();
    let id = add_due(&mut state, "A", None, None);
    state.select_today();
    state.open_overview(id);
    state.start_editing_overview();
    let inbox = state.home_project();
    state.select_project(inbox);
    assert_eq!(state.overview, None);
    assert!(
        !state.overview_editing,
        "the editor closes with the overview it belongs to"
    );
}

#[test]
fn completing_in_today_moves_the_selection_past_subtasks_that_go_too() {
    let mut state = state();
    let yesterday = Some(Due::day(date(2026, 1, 13)));
    let parent = add_due(&mut state, "A", None, yesterday);
    let subtask = state
        .repository()
        .add_task(NewTask {
            name: "A1".into(),
            parent_id: Some(parent),
            due: yesterday,
            ..Default::default()
        })
        .unwrap()
        .id;
    let next = add_due(&mut state, "B", None, Some(Due::day(date(2026, 1, 14))));
    state.select_today();
    assert_eq!(
        shown(&state),
        ["A", "A1", "B"],
        "the subtask is listed flat"
    );
    state.select_only(parent);
    state.toggle_completed(None);
    assert!(state.task(subtask).unwrap().is_completed());
    assert_eq!(state.selection, Selection::single(next));
}

#[test]
fn deleting_in_today_moves_the_selection_past_subtasks_that_go_too() {
    let mut state = state();
    let yesterday = Some(Due::day(date(2026, 1, 13)));
    let parent = add_due(&mut state, "A", None, yesterday);
    state
        .repository()
        .add_task(NewTask {
            name: "A1".into(),
            parent_id: Some(parent),
            due: yesterday,
            ..Default::default()
        })
        .unwrap();
    let next = add_due(&mut state, "B", None, Some(Due::day(date(2026, 1, 14))));
    state.select_today();
    state.select_only(parent);
    state.request_delete(None);
    state.confirm_dialog();
    assert_eq!(shown(&state), ["B"]);
    assert_eq!(state.selection, Selection::single(next));
}
