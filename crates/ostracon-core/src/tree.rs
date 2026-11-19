//! Tasks as an indented outline.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use jiff::ZonedDateTime;

use crate::model::{Project, ProjectId, Task, TaskId};

/// A task positioned in the flattened outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub task: Task,
    pub depth: usize,
    /// Direct subtasks shown in the outline, ignoring collapsing.
    pub child_count: usize,
    /// All direct subtasks, including completed ones that are hidden.
    pub subtask_count: usize,
    /// Direct subtasks that are completed, shown or not.
    pub completed_subtask_count: usize,
}

impl TaskRow {
    pub fn id(&self) -> TaskId {
        self.task.id
    }

    pub fn has_children(&self) -> bool {
        self.child_count > 0
    }

    /// Completed subtasks that aren't in the list because completed tasks are hidden.
    pub fn hidden_completed_count(&self) -> usize {
        self.subtask_count - self.child_count
    }
}

/// A list: its outline of open tasks, and below it, when they're shown, its completed top-level tasks, as in
/// Todoist. A project's are depth-first, with each task's subtasks beneath it. Today's are flat, with the
/// overdue tasks in a section of their own above the outline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    rows: Vec<TaskRow>,
    overdue_end: usize,
    completed_start: usize,
}

impl Rows {
    /// Every row: the overdue ones, the outline, then the completed ones.
    pub fn all(&self) -> &[TaskRow] {
        &self.rows
    }

    /// Open tasks due before today, in Today. Empty in a project.
    pub fn overdue(&self) -> &[TaskRow] {
        &self.rows[..self.overdue_end]
    }

    /// The open top-level tasks, with their subtasks: completed ones too, when shown.
    pub fn outline(&self) -> &[TaskRow] {
        &self.rows[self.overdue_end..self.completed_start]
    }

    /// The completed top-level tasks, with their subtasks.
    pub fn completed(&self) -> &[TaskRow] {
        &self.rows[self.completed_start..]
    }
}

/// A project's tasks in display order. Collapsed tasks hide their descendants. Unless `show_completed`,
/// completed tasks (and so everything beneath them) are left out; when shown, they follow their open
/// siblings, most recently completed first, and the top-level ones go below the outline.
pub fn rows(tasks: &[Task], project: ProjectId, show_completed: bool) -> Rows {
    let in_project: Vec<&Task> = tasks.iter().filter(|t| t.project_id == project).collect();
    let mut visible: HashMap<Option<TaskId>, Vec<&Task>> = HashMap::new();
    let mut all: HashMap<TaskId, (usize, usize)> = HashMap::new();
    for task in &in_project {
        if let Some(parent) = task.parent_id {
            let counts = all.entry(parent).or_default();
            counts.0 += 1;
            counts.1 += usize::from(task.is_completed());
        }
        if show_completed || !task.is_completed() {
            visible.entry(task.parent_id).or_default().push(task);
        }
    }
    for siblings in visible.values_mut() {
        siblings.sort_by_key(|t| (t.is_completed(), Reverse(t.completed_at), t.position, t.id));
    }

    /// Pushes `task`'s row, then its subtasks' unless it's collapsed.
    fn push(
        rows: &mut Vec<TaskRow>,
        visible: &HashMap<Option<TaskId>, Vec<&Task>>,
        all: &HashMap<TaskId, (usize, usize)>,
        task: &Task,
        depth: usize,
    ) {
        let (subtask_count, completed_subtask_count) =
            all.get(&task.id).copied().unwrap_or_default();
        let children = visible.get(&Some(task.id)).map_or(&[][..], Vec::as_slice);
        rows.push(TaskRow {
            task: task.clone(),
            depth,
            child_count: children.len(),
            subtask_count,
            completed_subtask_count,
        });
        if !task.collapsed {
            for child in children {
                push(rows, visible, all, child, depth + 1);
            }
        }
    }
    let mut outline = Vec::new();
    let mut completed = Vec::new();
    for root in visible.get(&None).into_iter().flatten() {
        let section = if root.is_completed() {
            &mut completed
        } else {
            &mut outline
        };
        push(section, &visible, &all, root, 0);
    }
    let completed_start = outline.len();
    outline.append(&mut completed);
    Rows {
        rows: outline,
        overdue_end: 0,
        completed_start,
    }
}

/// Today's list, as in Todoist: the open tasks due today or before, from every project, each at the top level
/// whatever its parent. The overdue ones come first, oldest first, then today's, those with a time first in
/// time order. Ties keep sidebar and outline order. When `show_completed`, below them come the completed tasks
/// due today, and those due earlier that were completed today, most recently completed first.
pub fn today_rows(
    tasks: &[Task],
    projects: &[Project],
    now: &ZonedDateTime,
    show_completed: bool,
) -> Rows {
    let today = now.date();
    let project_order: HashMap<ProjectId, usize> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();
    let outline_order = outline_order(tasks);
    let mut counts: HashMap<TaskId, (usize, usize)> = HashMap::new();
    for task in tasks {
        if let Some(parent) = task.parent_id {
            let counts = counts.entry(parent).or_default();
            counts.0 += 1;
            counts.1 += usize::from(task.is_completed());
        }
    }
    let row = |task: &Task| {
        let (subtask_count, completed_subtask_count) =
            counts.get(&task.id).copied().unwrap_or_default();
        TaskRow {
            task: task.clone(),
            depth: 0,
            child_count: 0,
            subtask_count,
            completed_subtask_count,
        }
    };

    let mut open: Vec<&Task> = tasks
        .iter()
        .filter(|t| !t.is_completed() && t.due.is_some_and(|d| d.date <= today))
        .collect();
    open.sort_by_key(|t| {
        let due = t.due.expect("only tasks with a due date");
        (
            due.date,
            due.time.is_none(),
            due.time,
            project_order.get(&t.project_id).copied(),
            outline_order.get(&t.id).copied(),
        )
    });
    let mut completed: Vec<&Task> = if show_completed {
        tasks
            .iter()
            .filter(|t| t.is_completed())
            .filter(|t| {
                let completed_today = t.completed_at.is_some_and(|at| {
                    ZonedDateTime::new(at, now.time_zone().clone()).date() == today
                });
                t.due
                    .is_some_and(|d| d.date == today || (d.date < today && completed_today))
            })
            .collect()
    } else {
        Vec::new()
    };
    completed.sort_by_key(|t| (Reverse(t.completed_at), t.id));

    let overdue_end = open
        .iter()
        .take_while(|t| t.due.is_some_and(|d| d.date < today))
        .count();
    let completed_start = open.len();
    Rows {
        rows: open.into_iter().chain(completed).map(row).collect(),
        overdue_end,
        completed_start,
    }
}

/// Where each task comes in its project's outline, expanded: the order its row would have if nothing were
/// collapsed.
fn outline_order(tasks: &[Task]) -> HashMap<TaskId, usize> {
    let ids: HashSet<TaskId> = tasks.iter().map(|t| t.id).collect();
    let mut children: HashMap<Option<TaskId>, Vec<&Task>> = HashMap::new();
    for task in tasks {
        // A task whose parent is missing counts as top-level, so it still gets a place.
        let parent = task.parent_id.filter(|p| ids.contains(p));
        children.entry(parent).or_default().push(task);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|t| (t.position, t.id));
    }
    let mut order = HashMap::new();
    let mut stack: Vec<&Task> = children
        .get(&None)
        .into_iter()
        .flatten()
        .rev()
        .copied()
        .collect();
    while let Some(task) = stack.pop() {
        // Each task once, in case a corrupt database has a cycle.
        if order.contains_key(&task.id) {
            continue;
        }
        order.insert(task.id, order.len());
        stack.extend(
            children
                .get(&Some(task.id))
                .into_iter()
                .flatten()
                .rev()
                .copied(),
        );
    }
    order
}

/// Open tasks due today or before, from every project, subtasks included.
pub fn today_count(tasks: &[Task], now: &ZonedDateTime) -> usize {
    tasks
        .iter()
        .filter(|t| !t.is_completed() && t.due.is_some_and(|d| d.date <= now.date()))
        .count()
}

/// Each of `rows` with whether it goes when the ones that `remove` picks go, taking their subtasks with them.
pub fn removing<'a>(
    rows: &'a [TaskRow],
    mut remove: impl FnMut(&TaskRow) -> bool + 'a,
) -> impl Iterator<Item = (&'a TaskRow, bool)> + 'a {
    let mut removing_below: Option<usize> = None;
    rows.iter().map(move |row| {
        if removing_below.is_some_and(|depth| row.depth > depth) {
            return (row, true);
        }
        let removed = remove(row);
        removing_below = removed.then_some(row.depth);
        (row, removed)
    })
}

/// Open tasks in a project, subtasks included.
pub fn open_count(tasks: &[Task], project: ProjectId) -> usize {
    tasks
        .iter()
        .filter(|t| t.project_id == project && !t.is_completed())
        .count()
}

/// A task's parent, grandparent and so on, nearest first.
pub fn ancestors(tasks: &[Task], id: TaskId) -> Vec<TaskId> {
    let parents: HashMap<TaskId, Option<TaskId>> =
        tasks.iter().map(|t| (t.id, t.parent_id)).collect();
    let mut result = Vec::new();
    let mut current = parents.get(&id).copied().flatten();
    while let Some(parent) = current {
        if result.contains(&parent) {
            break;
        }
        result.push(parent);
        current = parents.get(&parent).copied().flatten();
    }
    result
}

/// A task's direct subtasks in order.
pub fn children(tasks: &[Task], id: TaskId) -> Vec<&Task> {
    let mut children: Vec<&Task> = tasks.iter().filter(|t| t.parent_id == Some(id)).collect();
    children.sort_by_key(|t| (t.position, t.id));
    children
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;

    fn task(id: i64, parent: Option<i64>, completed_at: Option<i64>) -> Task {
        Task {
            id: TaskId(id),
            project_id: ProjectId(1),
            parent_id: parent.map(TaskId),
            name: id.to_string(),
            description: String::new(),
            position: id,
            completed_at: completed_at.map(|s| Timestamp::from_second(s).unwrap()),
            collapsed: false,
            created_at: Timestamp::UNIX_EPOCH,
            due: None,
            reminders: Vec::new(),
        }
    }

    fn shown(tasks: &[Task], show_completed: bool) -> Vec<String> {
        rows(tasks, ProjectId(1), show_completed)
            .all()
            .iter()
            .map(|r| format!("{}{}", " ".repeat(r.depth), r.task.name))
            .collect()
    }

    #[test]
    fn completed_tasks_follow_their_open_siblings_newest_first() {
        let tasks = [
            task(1, None, None),
            task(2, Some(1), Some(10)),
            task(3, Some(1), None),
            task(4, None, Some(30)),
            task(5, Some(4), Some(30)),
            task(6, None, Some(20)),
        ];
        assert_eq!(shown(&tasks, false), ["1", " 3"]);
        assert_eq!(shown(&tasks, true), ["1", " 3", " 2", "4", " 5", "6"]);
        let rows = rows(&tasks, ProjectId(1), true);
        assert_eq!((rows.outline().len(), rows.completed().len()), (3, 3));
    }

    #[test]
    fn today_lists_overdue_then_today_flat_and_timed_first() {
        use jiff::civil::{date, time};

        use crate::model::{Due, ProjectColor};

        let now = date(2026, 1, 14).at(10, 0, 0, 0).in_tz("UTC").unwrap();
        let project = |id, position| Project {
            id: ProjectId(id),
            name: id.to_string(),
            color: ProjectColor::Charcoal,
            position,
            is_inbox: false,
        };
        let projects = [project(1, 0), project(2, 1)];
        let due = |mut task: Task, due: Due| {
            task.due = Some(due);
            task
        };
        let (yesterday, today, tomorrow) =
            (date(2026, 1, 13), date(2026, 1, 14), date(2026, 1, 15));
        let mut tasks = vec![
            due(task(1, None, None), Due::day(today)),
            due(task(2, Some(1), None), Due::day(today)),
            due(task(3, None, None), Due::at(today, time(15, 0, 0, 0))),
            due(task(4, None, None), Due::day(yesterday)),
            due(task(5, None, None), Due::day(tomorrow)),
            task(6, None, None),
            due(task(7, None, None), Due::at(today, time(9, 0, 0, 0))),
            // Completed today, though due yesterday; and completed yesterday, though due today.
            due(task(8, None, Some(1_768_383_000)), Due::day(yesterday)),
            due(task(9, None, Some(1_768_300_000)), Due::day(today)),
            // Completed yesterday and due then: done with before today.
            due(task(10, None, Some(1_768_300_000)), Due::day(yesterday)),
        ];
        // Another project's task comes after this one's, even placed first.
        let mut other = due(task(11, None, None), Due::day(today));
        other.project_id = ProjectId(2);
        other.position = 0;
        tasks.push(other);

        let names = |rows: &[TaskRow]| -> Vec<String> {
            rows.iter()
                .map(|r| format!("{}{}", " ".repeat(r.depth), r.task.name))
                .collect()
        };
        let rows = today_rows(&tasks, &projects, &now, false);
        assert_eq!(names(rows.overdue()), ["4"]);
        assert_eq!(names(rows.outline()), ["7", "3", "1", "2", "11"]);
        assert!(rows.completed().is_empty());
        assert_eq!(rows.outline()[0].subtask_count, 0);
        assert_eq!(
            (
                rows.outline()[2].subtask_count,
                rows.outline()[2].child_count
            ),
            (1, 0),
            "subtasks are counted but not nested"
        );
        let rows = today_rows(&tasks, &projects, &now, true);
        assert_eq!(names(rows.completed()), ["8", "9"]);
        assert_eq!(today_count(&tasks, &now), 6);
    }

    #[test]
    fn removing_takes_subtasks_along() {
        let tasks = [
            task(1, None, None),
            task(2, Some(1), None),
            task(3, Some(2), None),
            task(4, Some(1), None),
            task(5, None, None),
        ];
        let rows = rows(&tasks, ProjectId(1), false);
        let removed: Vec<bool> = removing(rows.all(), |r| r.id() == TaskId(2))
            .map(|(_, removed)| removed)
            .collect();
        assert_eq!(removed, [false, true, true, false, false]);
    }
}
