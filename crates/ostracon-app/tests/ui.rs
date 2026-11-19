//! Headless GPUI tests: the real Workspace in a test window, driven with keystrokes.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use ostracon_app::state::{AppState, Composer, Dialog};
use ostracon_app::ui::{actions, workspace::Workspace};
use ostracon_core::{Due, NewTask, ProjectColor, Repository};

fn open(
    cx: &mut TestAppContext,
    seed: impl FnOnce(&Repository),
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        actions::bind_keys(cx);
    });
    let repo = Repository::in_memory().unwrap();
    seed(&repo);
    let state = AppState::new(repo, None);
    let slot: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::default();
    let captured = slot.clone();
    let (_root, cx) = cx.add_window_view(move |window, cx| {
        let workspace = cx.new(|cx| Workspace::new(state, None, window, cx));
        *captured.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    cx.run_until_parked();
    let workspace = slot.borrow().clone().unwrap();
    (workspace, cx)
}

fn names(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    workspace.read_with(cx, |w, _| {
        w.state
            .rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.task.name))
            .collect()
    })
}

#[gpui_kit::test]
fn quick_add_from_the_keyboard(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |repo| {
        repo.create_project("Home Admin", ProjectColor::Green)
            .unwrap();
    });
    cx.simulate_keystrokes("a");
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.composer),
        Composer::Adding { parent: None }
    );

    cx.simulate_input("Pay rent tomorrow 9am !1h ");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let task = workspace.read_with(cx, |w, _| w.state.rows()[0].task.clone());
    assert_eq!(task.name, "Pay rent");
    assert!(task.due.is_some_and(|d| d.has_time()));
    assert_eq!(task.reminders.len(), 1);
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.composer),
        Composer::Adding { parent: None },
        "Return leaves a fresh composer open"
    );

    // #project autocomplete: type a partial name, accept with Tab.
    cx.simulate_input("Fix boiler #hom");
    cx.simulate_keystrokes("tab enter");
    cx.run_until_parked();
    let toast = workspace.read_with(cx, |w, _| w.state.toast.as_ref().map(|t| t.message.clone()));
    assert_eq!(toast.as_deref(), Some("Task added to Home Admin"));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.composer),
        Composer::Closed
    );
}

#[gpui_kit::test]
fn backspace_over_a_block_keeps_the_word(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |_| {});
    cx.simulate_keystrokes("a");
    cx.simulate_input("Plan monday ");
    // Delete the space, then backspace over the "monday" block: it becomes plain text.
    cx.simulate_keystrokes("backspace backspace");
    cx.simulate_input("'s standup");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let task = workspace.read_with(cx, |w, _| w.state.rows()[0].task.clone());
    assert_eq!(task.name, "Plan monday's standup");
    assert_eq!(task.due, None);
}

#[gpui_kit::test]
fn list_keyboard_flow(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |repo| {
        for name in ["A", "B", "C"] {
            repo.add_task(NewTask {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
    });
    cx.simulate_keystrokes("j j tab");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["A", "  B", "C"]);
    cx.simulate_keystrokes("shift-tab k alt-cmd-down");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["B", "A", "C"]);
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["B", "C"]);
    cx.simulate_keystrokes("cmd-z");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["B", "A", "C"]);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(matches!(
        workspace.read_with(cx, |w, _| w.state.dialog.clone()),
        Dialog::ConfirmDeleteTasks(_)
    ));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        names(&workspace, cx),
        ["B", "A"],
        "after undo the selection stayed on C"
    );
}

#[gpui_kit::test]
fn typing_in_the_composer_never_triggers_list_shortcuts(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |_| {});
    cx.simulate_keystrokes("a");
    cx.simulate_input("aqjk");
    cx.simulate_keystrokes("space enter");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["aqjk"]);
}

#[gpui_kit::test]
fn editing_in_place_and_the_overview(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |repo| {
        repo.add_task(NewTask {
            name: "Review friday notes".into(),
            ..Default::default()
        })
        .unwrap();
    });
    cx.simulate_keystrokes("j enter");
    cx.run_until_parked();
    assert!(matches!(
        workspace.read_with(cx, |w, _| w.state.composer),
        Composer::Editing { .. }
    ));
    cx.simulate_keystrokes("end");
    cx.simulate_input(" v2");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let task = workspace.read_with(cx, |w, _| w.state.rows()[0].task.clone());
    assert_eq!(
        task.name, "Review friday notes v2",
        "an existing name is never re-read"
    );
    assert_eq!(task.due, None);

    cx.simulate_keystrokes("cmd-o");
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.overview),
        Some(task.id)
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), None);
}

#[gpui_kit::test]
fn dragging_a_task_onto_a_project_moves_it(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let mut garden = None;
    let (workspace, cx) = open(cx, |repo| {
        garden = Some(
            repo.create_project("Garden", ProjectColor::Green)
                .unwrap()
                .id,
        );
        repo.add_task(NewTask {
            name: "Water plants".into(),
            ..Default::default()
        })
        .unwrap();
    });
    let garden = garden.unwrap();
    let task = workspace.read_with(cx, |w, _| w.state.rows()[0].id());
    cx.update(|window, cx| {
        window.drag_to(
            ("task", task.0 as usize),
            ("project", garden.0 as usize),
            cx,
        )
    });
    cx.run_until_parked();
    let project = workspace.read_with(cx, |w, _| w.state.task(task).unwrap().project_id);
    assert_eq!(project, garden);
}

#[gpui_kit::test]
fn dragging_a_task_within_the_list_reorders_and_nests_it(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{point, px};
    let (workspace, cx) = open(cx, |repo| {
        for name in ["A", "B", "C"] {
            repo.add_task(NewTask {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
    });
    let ids = workspace.read_with(cx, |w, _| {
        w.state.rows().iter().map(|r| r.id()).collect::<Vec<_>>()
    });
    let row = |id: ostracon_core::TaskId| ("task", id.0 as usize);
    // Onto the top half of A: straight above it.
    cx.update(|window, cx| window.drag_to(row(ids[2]), row(ids[0]), cx));
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["C", "A", "B"]);

    // Onto the bottom half of A, and an indent's width to the right: a subtask of A.
    let (from, to) = cx.update(|window, _| {
        (
            window.find(row(ids[1])).bounds().center(),
            window.find(row(ids[0])).bounds(),
        )
    });
    let to = point(from.x + px(28.), to.bottom() - px(4.));
    cx.update(|window, cx| window.drag(from, to, cx));
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["C", "A", "  B"]);
}

#[gpui_kit::test]
fn right_click_menu_selects_the_row_and_deletes_it(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (workspace, cx) = open(cx, |repo| {
        for name in ["A", "B"] {
            repo.add_task(NewTask {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
    });
    let (a, b) = workspace.read_with(cx, |w, _| (w.state.rows()[0].id(), w.state.rows()[1].id()));
    cx.simulate_keystrokes("j");
    cx.run_until_parked();
    assert!(workspace.read_with(cx, |w, _| w.state.selection.contains(a)));

    cx.update(|window, cx| window.right_click(("task", b.0 as usize), cx));
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.acting_on(None)),
        [b],
        "a right click outside the selection selects that row"
    );
    // Open, Edit, Add sub-task, —, Complete, Move to, —, Delete…
    cx.update(|window, cx| window.within("popup-menu").click(7usize, cx));
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.state.dialog.clone()),
        Dialog::ConfirmDeleteTasks(vec![b])
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["A"]);
}

#[gpui_kit::test]
fn escape_closes_an_overview_picker_then_the_overview(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (workspace, cx) = open(cx, |repo| {
        repo.add_task(NewTask {
            name: "A".into(),
            ..Default::default()
        })
        .unwrap();
    });
    cx.simulate_keystrokes("j cmd-o");
    cx.run_until_parked();
    for button in ["overview-date", "overview-reminders"] {
        cx.update(|window, cx| window.click(button, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(
            workspace.read_with(cx, |w, _| w.state.overview.is_some()),
            "the first Esc closes only the picker"
        );
    }
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), None);
}

#[gpui_kit::test]
fn the_overview_adds_subtasks_with_the_full_composer(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (workspace, cx) = open(cx, |repo| {
        repo.add_task(NewTask {
            name: "A".into(),
            ..Default::default()
        })
        .unwrap();
    });
    cx.simulate_keystrokes("j cmd-o");
    cx.run_until_parked();
    cx.update(|window, cx| window.click("overview-add-subtask", cx));
    cx.run_until_parked();
    assert!(matches!(
        workspace.read_with(cx, |w, _| w.state.composer),
        Composer::AddingInOverview { .. }
    ));

    cx.simulate_input("B tomorrow ");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["A", "  B"]);
    assert!(workspace.read_with(cx, |w, _| w.state.rows()[1].task.due.is_some()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| (w.state.composer, w.state.overview.is_some())),
        (Composer::Closed, true),
        "the first Esc closes only the composer"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), None);
}

#[gpui_kit::test]
fn a_subtask_overview_opens_its_parent(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (workspace, cx) = open(cx, |repo| {
        let a = repo
            .add_task(NewTask {
                name: "A".into(),
                ..Default::default()
            })
            .unwrap();
        repo.add_task(NewTask {
            name: "B".into(),
            parent_id: Some(a.id),
            ..Default::default()
        })
        .unwrap();
    });
    let (a, b) = workspace.read_with(cx, |w, _| {
        (w.state.rows()[0].task.id, w.state.rows()[1].task.id)
    });
    cx.simulate_keystrokes("j j cmd-o");
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), Some(b));
    cx.update(|window, cx| window.click("overview-parent", cx));
    cx.run_until_parked();
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), Some(a));
}

#[gpui_kit::test]
fn deleting_from_the_overview(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let (workspace, cx) = open(cx, |repo| {
        for name in ["A", "B"] {
            repo.add_task(NewTask {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
    });
    cx.simulate_keystrokes("j cmd-o");
    cx.run_until_parked();
    cx.update(|window, cx| window.click("overview-more", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(0usize, cx));
    cx.run_until_parked();
    assert!(matches!(
        workspace.read_with(cx, |w, _| w.state.dialog.clone()),
        Dialog::ConfirmDeleteTasks(_)
    ));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["B"]);
    assert_eq!(workspace.read_with(cx, |w, _| w.state.overview), None);
}

#[gpui_kit::test]
fn the_overview_name_reads_a_project_typed_into_it(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let (workspace, cx) = open(cx, |repo| {
        repo.create_project("Garden", ProjectColor::Green).unwrap();
        for name in ["A", "B"] {
            repo.add_task(NewTask {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
    });
    cx.simulate_keystrokes("j cmd-o");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let name = workspace.read(cx).overview.as_ref().unwrap().name.clone();
        window.focus(&name.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("end");
    cx.simulate_input(" two #Garden ");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (task, garden) = workspace.read_with(cx, |w, _| {
        let task = w.state.task(w.state.overview.unwrap()).unwrap().clone();
        let garden = w
            .state
            .projects()
            .iter()
            .find(|p| p.name == "Garden")
            .unwrap()
            .id;
        (task, garden)
    });
    assert_eq!(task.name, "A two");
    assert_eq!(task.project_id, garden);
    assert_eq!(
        names(&workspace, cx),
        ["A two", "B"],
        "it stays in the list"
    );
    let shown = workspace.read_with(cx, |w, cx| {
        w.overview
            .as_ref()
            .unwrap()
            .name
            .read(cx)
            .draft()
            .plain_text()
    });
    assert_eq!(shown, "A two", "the project leaves the name");

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(names(&workspace, cx), ["B"]);
}

#[gpui_kit::test]
fn overview_side_values_sit_flush_with_their_labels(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, |repo| {
        let project = repo
            .create_project("close to the edge", ProjectColor::Blue)
            .unwrap();
        repo.add_task(NewTask {
            name: "Siberian Khatru".into(),
            project_id: Some(project.id),
            due: Some(Due::day(jiff::civil::date(2026, 11, 20))),
            ..Default::default()
        })
        .unwrap();
    });
    let id = workspace.read_with(cx, |w, _| w.state.tasks()[0].id);
    workspace.update_in(cx, |w, window, cx| {
        w.act(window, cx, |s| s.open_overview(id))
    });
    cx.run_until_parked();
    // The project value is wrapped in a dropdown menu and the date's isn't. Stretched across the side panel,
    // either button would centre its icon and text instead of lining them up with the label.
    for (label, value) in [
        ("overview-label-Project", "overview-project"),
        ("overview-label-Date", "overview-date"),
    ] {
        let label = cx.debug_bounds(label).expect(label);
        let value = cx.debug_bounds(value).expect(value);
        assert_eq!(value.left(), label.left(), "{value:?} under {label:?}");
    }
}
