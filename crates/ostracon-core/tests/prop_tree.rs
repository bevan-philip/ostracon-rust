//! Property tests of the outline: `rows`, `today_rows` and `removing` against straightforward recomputations
//! over random forests of tasks.

use std::cmp::Reverse;
use std::collections::HashMap;

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ZonedDateTime};
use ostracon_core::tree::{self, TaskRow};
use ostracon_core::{Due, Project, ProjectColor, ProjectId, Task, TaskId};
use proptest::prelude::*;

const ZONES: &[&str] = &["UTC", "Europe/London", "America/New_York", "Pacific/Apia"];

#[derive(Debug, Clone)]
struct Spec {
    parent: Option<usize>,
    project: i64,
    position: i64,
    /// Hours before now.
    completed: Option<i64>,
    collapsed: bool,
    /// Days from today, and a time.
    due: Option<(i64, Option<(i8, i8)>)>,
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        proptest::option::weighted(0.6, any::<usize>()),
        1i64..=3,
        0i64..4,
        proptest::option::weighted(0.35, 0i64..40),
        proptest::bool::weighted(0.25),
        proptest::option::weighted(0.6, (-3i64..=2, proptest::option::of((0i8..24, 0i8..60)))),
    )
        .prop_map(
            |(parent, project, position, completed, collapsed, due)| Spec {
                parent,
                project,
                position,
                completed,
                collapsed,
                due,
            },
        )
}

fn now() -> impl Strategy<Value = ZonedDateTime> {
    (
        0i64..(3 * 365),
        0i8..24,
        0i8..60,
        proptest::sample::select(ZONES),
    )
        .prop_map(|(day, h, m, zone)| {
            jiff::civil::date(2025, 1, 1)
                .checked_add(jiff::Span::new().days(day))
                .unwrap()
                .at(h, m, 0, 0)
                .to_zoned_datetime(TimeZone::get(zone).unwrap())
                .unwrap()
        })
}

/// Well-formed tasks: parents come earlier and share the project; ids are 1-based in creation order.
fn build(specs: &[Spec], now: &ZonedDateTime) -> Vec<Task> {
    let mut tasks: Vec<Task> = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        let parent = spec.parent.filter(|_| i > 0).map(|p| p % i);
        let project = parent.map_or(spec.project, |p| tasks[p].project_id.0);
        let due = spec.due.map(|(days, time)| {
            let date: Date = now
                .date()
                .checked_add(jiff::Span::new().days(days))
                .unwrap();
            Due {
                date,
                time: time.map(|(h, m)| Time::new(h, m, 0, 0).unwrap()),
            }
        });
        tasks.push(Task {
            id: TaskId(i as i64 + 1),
            project_id: ProjectId(project),
            parent_id: parent.map(|p| TaskId(p as i64 + 1)),
            name: format!("t{}", i + 1),
            description: String::new(),
            position: spec.position,
            completed_at: spec
                .completed
                .map(|h| now.timestamp() - SignedDuration::from_hours(h)),
            collapsed: spec.collapsed,
            created_at: Timestamp::UNIX_EPOCH,
            due,
            reminders: Vec::new(),
        });
    }
    tasks
}

fn projects() -> Vec<Project> {
    // Sidebar order differs from id order.
    [(1, 0), (3, 1), (2, 2)]
        .into_iter()
        .map(|(id, position)| Project {
            id: ProjectId(id),
            name: id.to_string(),
            color: ProjectColor::Blue,
            position,
            is_inbox: id == 1,
        })
        .collect()
}

fn by_id(tasks: &[Task]) -> HashMap<TaskId, &Task> {
    tasks.iter().map(|t| (t.id, t)).collect()
}

fn chain<'a>(tasks: &HashMap<TaskId, &'a Task>, task: &'a Task) -> Vec<&'a Task> {
    let mut path = vec![task];
    while let Some(parent) = path.last().unwrap().parent_id {
        path.push(tasks[&parent]);
    }
    path.reverse();
    path
}

type SiblingKey = (bool, Reverse<Option<Timestamp>>, i64, TaskId);

fn sibling_key(t: &Task) -> SiblingKey {
    (t.is_completed(), Reverse(t.completed_at), t.position, t.id)
}

/// The project's rows, by sorting each shown task on its path of sibling keys from the top.
fn expected_rows(
    tasks: &[Task],
    project: ProjectId,
    show_completed: bool,
) -> Vec<(TaskId, usize, usize, usize, usize)> {
    let index = by_id(tasks);
    let visible = |t: &Task| t.project_id == project && (show_completed || !t.is_completed());
    let mut shown: Vec<(Vec<SiblingKey>, &Task)> = tasks
        .iter()
        .filter(|t| {
            let path = chain(&index, t);
            path.iter().all(|a| visible(a)) && path[..path.len() - 1].iter().all(|a| !a.collapsed)
        })
        .map(|t| (chain(&index, t).iter().map(|a| sibling_key(a)).collect(), t))
        .collect();
    shown.sort_by(|a, b| a.0.cmp(&b.0));
    shown
        .into_iter()
        .map(|(path, t)| {
            let children: Vec<&Task> = tasks.iter().filter(|c| c.parent_id == Some(t.id)).collect();
            (
                t.id,
                path.len() - 1,
                children.iter().filter(|c| visible(c)).count(),
                children.len(),
                children.iter().filter(|c| c.is_completed()).count(),
            )
        })
        .collect()
}

fn summary(rows: &[TaskRow]) -> Vec<(TaskId, usize, usize, usize, usize)> {
    rows.iter()
        .map(|r| {
            (
                r.id(),
                r.depth,
                r.child_count,
                r.subtask_count,
                r.completed_subtask_count,
            )
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2048),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn project_rows_match_a_path_sort(specs in proptest::collection::vec(spec(), 0..16), now in now(), project in 1i64..=3, show_completed in any::<bool>()) {
        let tasks = build(&specs, &now);
        let rows = tree::rows(&tasks, ProjectId(project), show_completed);
        prop_assert_eq!(summary(rows.all()), expected_rows(&tasks, ProjectId(project), show_completed));
        prop_assert!(rows.overdue().is_empty());
        prop_assert!(rows.outline().iter().all(|r| r.depth > 0 || !r.task.is_completed()));
        prop_assert!(rows.completed().iter().all(|r| r.depth > 0 || r.task.is_completed()));
        prop_assert!(rows.completed().first().is_none_or(|r| r.depth == 0));
    }

    #[test]
    fn today_rows_match_a_recomputation(specs in proptest::collection::vec(spec(), 0..16), now in now(), show_completed in any::<bool>()) {
        let tasks = build(&specs, &now);
        let projects = projects();
        let index = by_id(&tasks);
        let today = now.date();
        let sidebar = |p: ProjectId| projects.iter().position(|x| x.id == p);
        let outline_path = |t: &Task| -> Vec<(i64, TaskId)> { chain(&index, t).iter().map(|a| (a.position, a.id)).collect() };
        let mut open: Vec<&Task> = tasks.iter().filter(|t| !t.is_completed() && t.due.is_some_and(|d| d.date <= today)).collect();
        open.sort_by_key(|t| {
            let due = t.due.unwrap();
            (due.date, due.time.is_none(), due.time, sidebar(t.project_id), outline_path(t))
        });
        let mut done: Vec<&Task> = if show_completed {
            tasks.iter().filter(|t| {
                let Some(at) = t.completed_at else { return false };
                let Some(due) = t.due else { return false };
                let completed_today = ZonedDateTime::new(at, now.time_zone().clone()).date() == today;
                due.date == today || (due.date < today && completed_today)
            }).collect()
        } else {
            Vec::new()
        };
        done.sort_by_key(|t| (Reverse(t.completed_at), t.id));

        let rows = tree::today_rows(&tasks, &projects, &now, show_completed);
        let ids = |rows: &[TaskRow]| rows.iter().map(TaskRow::id).collect::<Vec<_>>();
        let overdue: Vec<TaskId> = open.iter().filter(|t| t.due.unwrap().date < today).map(|t| t.id).collect();
        let due_today: Vec<TaskId> = open.iter().filter(|t| t.due.unwrap().date == today).map(|t| t.id).collect();
        prop_assert_eq!(ids(rows.overdue()), overdue);
        prop_assert_eq!(ids(rows.outline()), due_today);
        prop_assert_eq!(ids(rows.completed()), done.iter().map(|t| t.id).collect::<Vec<_>>());
        prop_assert!(rows.all().iter().all(|r| r.depth == 0 && r.child_count == 0));
        prop_assert_eq!(tree::today_count(&tasks, &now), rows.overdue().len() + rows.outline().len());
    }

    /// `removing` takes a row's subtasks with it: in a project, every row under a picked one by its parents.
    #[test]
    fn removing_takes_exactly_the_picked_subtrees(specs in proptest::collection::vec(spec(), 0..16), now in now(), project in 1i64..=3, show_completed in any::<bool>(), picks in proptest::collection::vec(any::<bool>(), 16)) {
        let tasks = build(&specs, &now);
        let index = by_id(&tasks);
        let rows = tree::rows(&tasks, ProjectId(project), show_completed);
        let picked = |id: TaskId| picks[(id.0 as usize - 1) % picks.len()];
        let removed: Vec<bool> = tree::removing(rows.all(), |r| picked(r.id())).map(|(_, removed)| removed).collect();
        let expected: Vec<bool> = rows.all().iter().map(|r| chain(&index, &r.task).iter().any(|a| picked(a.id))).collect();
        prop_assert_eq!(removed, expected);
    }
}
