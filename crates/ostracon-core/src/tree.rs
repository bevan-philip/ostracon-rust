//! Tasks as an indented outline.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::model::{ProjectId, Task, TaskId};

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

/// A project's list: its outline of open tasks, and below it, when they're shown, its completed top-level
/// tasks, as in Todoist. Both are depth-first, with each task's subtasks beneath it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    rows: Vec<TaskRow>,
    completed_start: usize,
}

impl Rows {
    /// Every row, the outline first.
    pub fn all(&self) -> &[TaskRow] {
        &self.rows
    }

    /// The open top-level tasks, with their subtasks: completed ones too, when shown.
    pub fn outline(&self) -> &[TaskRow] {
        &self.rows[..self.completed_start]
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
        completed_start,
    }
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
