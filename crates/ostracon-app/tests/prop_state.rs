//! Random histories of UI actions on [`AppState`], interleaved with changes written behind its back as the CLI
//! would, checking after every step what `reconcile` promises: the selection, composer, overview and dialogs
//! only refer to what exists and is shown.

use std::collections::BTreeSet;

use jiff::ZonedDateTime;
use jiff::civil::date;
use ostracon_app::state::{AppState, Composer, Dialog, Selection, View};
use ostracon_core::quickadd::Draft;
use ostracon_core::{Due, NewTask, ProjectColor, ProjectId, Repository, TaskId, tree};
use proptest::prelude::*;
use proptest::sample::Index;

fn now() -> ZonedDateTime {
    date(2026, 1, 14)
        .at(10, 30, 0, 0)
        .in_tz("Europe/London")
        .unwrap()
}

#[derive(Debug, Clone)]
struct SetupTask {
    parent: Option<Index>,
    project: usize,
    completed: bool,
    collapsed: bool,
    due: Option<i64>,
}

fn setup_task() -> impl Strategy<Value = SetupTask> {
    (
        proptest::option::weighted(0.6, any::<Index>()),
        0usize..3,
        proptest::bool::weighted(0.2),
        proptest::bool::weighted(0.2),
        proptest::option::weighted(0.5, -2i64..=2),
    )
        .prop_map(|(parent, project, completed, collapsed, due)| SetupTask {
            parent,
            project,
            completed,
            collapsed,
            due,
        })
}

#[derive(Debug, Clone)]
enum Op {
    SelectProject(Index),
    SelectToday,
    ShowCompleted(bool),
    SelectOnly(Index),
    Toggle(Index),
    ExtendTo(Index),
    ExtendBy(bool),
    MoveSelection(bool),
    ClearSelection,
    StartAdding,
    StartAddingSubtask(Option<Index>),
    StartEditing(Option<Index>),
    CloseComposer,
    Submit(&'static str),
    OpenOverview(Index),
    CloseOverview,
    DismissOverview,
    EditOverview,
    SaveOverview(&'static str),
    AddSubtaskInOverview(&'static str),
    MoveOverviewTask(Index),
    ToggleCompleted(Option<Index>),
    CompleteThenUndo(Option<Index>),
    SetDue(Option<Index>, Option<i64>),
    Indent(Option<Index>),
    Outdent(Option<Index>),
    MoveBy(Option<Index>, bool),
    MoveSelectionToProject(Index),
    Drag(usize, isize),
    Collapse(Index, bool),
    CollapseOrSelectParent,
    ExpandSelected,
    Delete(Option<Index>),
    DeleteProject(Index),
    Reveal(Index),
    CompleteFromNotification(Index),
    RequestDiscardToOpen(Index),
    ConfirmDialog,
    CancelDialog,
    /// Written by another process, such as the CLI.
    External(External),
}

#[derive(Debug, Clone)]
enum External {
    Delete(Index),
    Complete(Index, bool),
    Move(Index, Index),
    Add(Option<Index>),
    DeleteProject(Index),
}

const NAMES: &[&str] = &[
    "Write report",
    "Call mum tomorrow",
    "Plan #Work",
    "Fix bug #Home today",
    "  ",
    "Ring at 5pm !30min",
    "#Inbox",
];

fn op() -> impl Strategy<Value = Op> {
    let idx = any::<Index>;
    let oidx = || proptest::option::weighted(0.8, any::<Index>());
    let name = || proptest::sample::select(NAMES);
    let external = prop_oneof![
        idx().prop_map(External::Delete),
        (idx(), any::<bool>()).prop_map(|(i, b)| External::Complete(i, b)),
        (idx(), idx()).prop_map(|(i, p)| External::Move(i, p)),
        proptest::option::of(idx()).prop_map(External::Add),
        idx().prop_map(External::DeleteProject),
    ];
    prop_oneof![
        2 => idx().prop_map(Op::SelectProject),
        1 => Just(Op::SelectToday),
        1 => any::<bool>().prop_map(Op::ShowCompleted),
        3 => idx().prop_map(Op::SelectOnly),
        2 => idx().prop_map(Op::Toggle),
        2 => idx().prop_map(Op::ExtendTo),
        1 => any::<bool>().prop_map(Op::ExtendBy),
        1 => any::<bool>().prop_map(Op::MoveSelection),
        1 => Just(Op::ClearSelection),
        1 => Just(Op::StartAdding),
        1 => oidx().prop_map(Op::StartAddingSubtask),
        1 => oidx().prop_map(Op::StartEditing),
        1 => Just(Op::CloseComposer),
        2 => name().prop_map(Op::Submit),
        2 => idx().prop_map(Op::OpenOverview),
        1 => Just(Op::CloseOverview),
        1 => Just(Op::DismissOverview),
        1 => Just(Op::EditOverview),
        1 => name().prop_map(Op::SaveOverview),
        1 => name().prop_map(Op::AddSubtaskInOverview),
        2 => idx().prop_map(Op::MoveOverviewTask),
        3 => oidx().prop_map(Op::ToggleCompleted),
        2 => oidx().prop_map(Op::CompleteThenUndo),
        1 => (oidx(), proptest::option::of(-2i64..=2)).prop_map(|(i, d)| Op::SetDue(i, d)),
        2 => oidx().prop_map(Op::Indent),
        1 => oidx().prop_map(Op::Outdent),
        1 => (oidx(), any::<bool>()).prop_map(|(i, b)| Op::MoveBy(i, b)),
        1 => idx().prop_map(Op::MoveSelectionToProject),
        2 => (0usize..12, -1isize..4).prop_map(|(g, d)| Op::Drag(g, d)),
        1 => (idx(), any::<bool>()).prop_map(|(i, b)| Op::Collapse(i, b)),
        1 => Just(Op::CollapseOrSelectParent),
        1 => Just(Op::ExpandSelected),
        2 => oidx().prop_map(Op::Delete),
        1 => idx().prop_map(Op::DeleteProject),
        1 => idx().prop_map(Op::Reveal),
        1 => idx().prop_map(Op::CompleteFromNotification),
        1 => idx().prop_map(Op::RequestDiscardToOpen),
        2 => Just(Op::ConfirmDialog),
        1 => Just(Op::CancelDialog),
        2 => external.prop_map(Op::External),
    ]
}

fn build(setup: &[SetupTask]) -> AppState {
    let repo = Repository::in_memory().unwrap();
    let inbox = repo.inbox().unwrap().id;
    let work = repo.create_project("Work", ProjectColor::Red).unwrap().id;
    let home = repo.create_project("Home", ProjectColor::Blue).unwrap().id;
    let projects = [inbox, work, home];
    let mut ids: Vec<TaskId> = Vec::new();
    for (i, spec) in setup.iter().enumerate() {
        let parent = spec
            .parent
            .as_ref()
            .filter(|_| i > 0)
            .map(|p| ids[p.index(i)]);
        let task = repo
            .add_task(NewTask {
                name: format!("t{}", i + 1),
                project_id: parent.is_none().then_some(projects[spec.project]),
                parent_id: parent,
                due: spec.due.map(|d| {
                    Due::day(now().date().checked_add(jiff::Span::new().days(d)).unwrap())
                }),
                ..Default::default()
            })
            .unwrap();
        if spec.completed {
            repo.set_completed(task.id, true).unwrap();
        }
        if spec.collapsed {
            repo.set_collapsed(task.id, true).unwrap();
        }
        ids.push(task.id);
    }
    AppState::with_clock(repo, None, now)
}

fn row(state: &AppState, i: &Index) -> Option<TaskId> {
    let rows = state.rows();
    (!rows.is_empty()).then(|| rows[i.index(rows.len())].id())
}

fn task(state: &AppState, i: &Index) -> Option<TaskId> {
    let tasks = state.tasks();
    (!tasks.is_empty()).then(|| tasks[i.index(tasks.len())].id)
}

fn project(state: &AppState, i: &Index) -> ProjectId {
    let projects = state.projects();
    projects[i.index(projects.len())].id
}

fn completion(state: &AppState) -> Vec<(TaskId, bool)> {
    state
        .tasks()
        .iter()
        .map(|t| (t.id, t.is_completed()))
        .collect()
}

fn check(state: &AppState, op: &Op) -> Result<(), TestCaseError> {
    let rows: BTreeSet<TaskId> = state.rows().iter().map(|r| r.id()).collect();
    match state.view {
        View::Today => {}
        View::Project(p) => prop_assert!(
            state.project_by_id(p).is_some(),
            "the project on screen exists"
        ),
    }
    if let Selection::Tasks {
        anchor,
        ids,
        range_end,
    } = &state.selection
    {
        prop_assert!(
            ids.contains(anchor),
            "the anchor is selected: {:?}",
            state.selection
        );
        prop_assert!(
            ids.iter().all(|id| rows.contains(id)),
            "selected tasks are rows: {:?} of {:?}",
            state.selection,
            rows
        );
        prop_assert!(
            range_end.is_none_or(|e| rows.contains(&e)),
            "the range ends on a row"
        );
    }
    match state.composer {
        Composer::Editing { task } | Composer::Adding { parent: Some(task) } => {
            prop_assert!(
                rows.contains(&task),
                "the composer's task is a row: {:?}",
                state.composer
            )
        }
        Composer::AddingInOverview { parent } => prop_assert_eq!(state.overview, Some(parent)),
        _ => {}
    }
    if let Some(id) = state.overview {
        prop_assert!(state.task(id).is_some(), "the overview's task exists");
    }
    prop_assert!(
        !state.overview_editing || state.overview.is_some(),
        "the overview's editor is open only with it"
    );
    match &state.dialog {
        Dialog::ProjectEditor(draft) => prop_assert!(
            draft
                .project
                .is_none_or(|p| state.project_by_id(p).is_some())
        ),
        Dialog::ConfirmDeleteProject(p) => prop_assert!(state.project_by_id(*p).is_some()),
        Dialog::ConfirmDeleteTasks(ids) => {
            prop_assert!(!ids.is_empty());
            prop_assert!(
                ids.iter().all(|&id| state.task(id).is_some()),
                "tasks to delete exist"
            );
        }
        Dialog::ConfirmDiscard(next) => {
            prop_assert!(next.is_none_or(|id| state.task(id).is_some()));
        }
        Dialog::None => {}
    }
    // Without an overview keeping moved tasks in place, the list is exactly the outline of the data.
    if state.overview.is_none() {
        let expected = match state.view {
            View::Project(p) => tree::rows(state.tasks(), p, state.show_completed),
            View::Today => tree::today_rows(
                state.tasks(),
                state.projects(),
                &now(),
                state.show_completed,
            ),
        };
        prop_assert_eq!(state.rows(), expected.all(), "rows after {:?}", op);
    }
    prop_assert_eq!(state.error.as_deref(), None, "no error after {:?}", op);
    Ok(())
}

fn run(state: &mut AppState, op: &Op) -> Result<(), TestCaseError> {
    state.error = None;
    let selected_before = !state.selection.is_empty();
    // Left open: with the overview keeping moved tasks in the list, removing one of their subtasks can close
    // the overview and take the row the selection was handed to with it.
    let keeps_moved_tasks = state
        .view
        .project()
        .is_some_and(|p| state.rows().iter().any(|r| r.task.project_id != p));
    // Whether the op takes the selected tasks out of the list, after which the selection should move on.
    let mut removes_selection = false;
    match op {
        Op::SelectProject(i) => {
            let p = project(state, i);
            state.select_project(p)
        }
        Op::SelectToday => state.select_today(),
        Op::ShowCompleted(b) => state.set_show_completed(*b),
        Op::SelectOnly(i) => {
            if let Some(id) = row(state, i) {
                state.select_only(id)
            }
        }
        Op::Toggle(i) => {
            if let Some(id) = row(state, i) {
                state.toggle_selection(id)
            }
        }
        Op::ExtendTo(i) => {
            if let Some(id) = row(state, i) {
                state.extend_selection_to(id)
            }
        }
        Op::ExtendBy(down) => state.extend_selection_by(if *down { 1 } else { -1 }),
        Op::MoveSelection(down) => state.move_selection(if *down { 1 } else { -1 }),
        Op::ClearSelection => state.clear_selection(),
        Op::StartAdding => state.start_adding(),
        Op::StartAddingSubtask(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.start_adding_subtask(id)
        }
        Op::StartEditing(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.start_editing(id)
        }
        Op::CloseComposer => state.close_composer(),
        Op::Submit(name) => {
            let home = state.home_project();
            let due = state.default_due();
            state.submit_composer(&Draft::from_text(name, false), "", home, due, &[]);
        }
        Op::OpenOverview(i) => {
            if let Some(id) = task(state, i) {
                state.open_overview(id)
            }
        }
        Op::CloseOverview => state.close_overview(),
        Op::DismissOverview => state.dismiss_overview(),
        Op::EditOverview => state.start_editing_overview(),
        Op::SaveOverview(name) => {
            if let Some(id) = state.overview.filter(|_| state.overview_editing) {
                state.save_overview(id, &Draft::from_text(name, false), "");
            }
        }
        Op::AddSubtaskInOverview(name) => {
            if let Some(id) = state.overview {
                state.start_adding_subtask_in_overview();
                let project = state.task(id).unwrap().project_id;
                state.submit_composer(&Draft::from_text(name, false), "", project, None, &[]);
            }
        }
        Op::MoveOverviewTask(p) => {
            if let Some(id) = state.overview {
                let p = project(state, p);
                let before: Vec<TaskId> = state.rows().iter().map(|r| r.id()).collect();
                state.move_task_to(id, p);
                // The moved task and its subtasks keep their place until the overview closes.
                let after: Vec<TaskId> = state.rows().iter().map(|r| r.id()).collect();
                if before.contains(&id) && state.view.project().is_some() {
                    prop_assert_eq!(after, before, "the list doesn't shift under the overview");
                }
            }
        }
        Op::ToggleCompleted(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            let acting = state.acting_on(id);
            removes_selection = !state.show_completed
                && acting.iter().any(|&a| state.selection.contains(a))
                && acting
                    .iter()
                    .any(|&a| state.task(a).is_some_and(|t| !t.is_completed()));
            state.toggle_completed(id)
        }
        Op::CompleteThenUndo(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            let acting = state.acting_on(id);
            // Left open: an open subtask of a completed task (adding a subtask to a completed one makes one)
            // gets its completed ancestors reopened by undo, as reopening always reopens ancestors.
            let open_under_completed = state.tasks().iter().any(|t| {
                !t.is_completed()
                    && state
                        .ancestors(t.id)
                        .iter()
                        .any(|&a| state.task(a).is_some_and(|a| a.is_completed()))
            });
            if !open_under_completed
                && acting
                    .iter()
                    .any(|&a| state.task(a).is_some_and(|t| !t.is_completed()))
            {
                let before = completion(state);
                state.toggle_completed(id);
                prop_assert!(
                    state.toast.as_ref().is_some_and(|t| t.undo.is_some()),
                    "completing offers undo"
                );
                state.undo_toast();
                prop_assert_eq!(
                    completion(state),
                    before,
                    "undo reopens exactly what completing closed"
                );
            }
        }
        Op::SetDue(i, days) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            let due = days
                .map(|d| Due::day(now().date().checked_add(jiff::Span::new().days(d)).unwrap()));
            state.set_due(id, due)
        }
        Op::Indent(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.indent(id)
        }
        Op::Outdent(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.outdent(id)
        }
        Op::MoveBy(i, down) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.move_by(id, if *down { 1 } else { -1 })
        }
        Op::MoveSelectionToProject(p) => {
            let p = project(state, p);
            let ids = state.acting_on(None);
            removes_selection = state.view.project().is_some_and(|v| v != p)
                && ids
                    .iter()
                    .any(|&id| state.task(id).is_some_and(|t| t.project_id != p));
            state.move_to_project(&ids, p);
        }
        Op::Drag(gap, depth) => {
            let dragged = state.acting_on(None);
            // The overview's backdrop covers the list, so nothing is dragged while it's open.
            if !dragged.is_empty() && state.view.project().is_some() && state.overview.is_none() {
                let target = state.drop_target(&dragged, *gap, *depth);
                state.drop_tasks(&dragged, target);
            }
        }
        Op::Collapse(i, b) => {
            if let Some(id) = row(state, i) {
                state.set_collapsed(id, *b)
            }
        }
        Op::CollapseOrSelectParent => state.collapse_or_select_parent(),
        Op::ExpandSelected => state.expand_selected(),
        Op::Delete(i) => {
            let id = i.as_ref().and_then(|i| row(state, i));
            state.request_delete(id);
            if let Dialog::ConfirmDeleteTasks(ids) = &state.dialog {
                removes_selection = ids.iter().any(|&id| state.selection.contains(id));
                state.confirm_dialog();
            }
        }
        Op::DeleteProject(p) => {
            let p = project(state, p);
            state.request_delete_project(p);
        }
        Op::Reveal(i) => {
            if let Some(id) = task(state, i) {
                state.reveal(id)
            }
        }
        Op::CompleteFromNotification(i) => {
            if let Some(id) = task(state, i) {
                state.complete_from_notification(id)
            }
        }
        Op::RequestDiscardToOpen(i) => {
            if let Some(id) = row(state, i) {
                state.request_discard_to_open(id)
            }
        }
        Op::ConfirmDialog => state.confirm_dialog(),
        Op::CancelDialog => state.cancel_dialog(),
        Op::External(external) => {
            let repo = state.repository();
            let pick = |i: &Index| task(state, i);
            let _ = match external {
                External::Delete(i) => pick(i).map(|id| repo.delete_task(id).unwrap()),
                External::Complete(i, b) => pick(i).map(|id| repo.set_completed(id, *b).unwrap()),
                External::Move(i, p) => {
                    pick(i).map(|id| repo.move_to_project(id, project(state, p)).unwrap())
                }
                External::Add(parent) => {
                    let parent = parent.as_ref().and_then(pick);
                    repo.add_task(NewTask {
                        name: "external".into(),
                        parent_id: parent,
                        ..Default::default()
                    })
                    .map(drop)
                    .ok()
                }
                External::DeleteProject(p) => {
                    let p = project(state, p);
                    let _ = repo.delete_project(p);
                    None
                }
            };
            state.reload();
        }
    }
    check(state, op)?;
    if removes_selection && selected_before && !keeps_moved_tasks && !state.rows().is_empty() {
        prop_assert!(
            !state.selection.is_empty(),
            "the selection moves on to a remaining row after {:?}",
            op
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn state_stays_consistent(setup in proptest::collection::vec(setup_task(), 0..12), ops in proptest::collection::vec(op(), 1..30)) {
        let mut state = build(&setup);
        check(&state, &Op::ClearSelection)?;
        for op in &ops {
            run(&mut state, op)?;
        }
    }
}
