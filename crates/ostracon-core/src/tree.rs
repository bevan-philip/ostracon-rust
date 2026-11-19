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

/// A project's tasks in display order, depth-first. Collapsed tasks hide their descendants. Unless
/// `show_completed`, completed tasks (and so everything beneath them) are left out; when shown, they follow
/// their open siblings, most recently completed first, as in Todoist.
pub fn rows(tasks: &[Task], project: ProjectId, show_completed: bool) -> Vec<TaskRow> {
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

    fn append(
        rows: &mut Vec<TaskRow>,
        visible: &HashMap<Option<TaskId>, Vec<&Task>>,
        all: &HashMap<TaskId, (usize, usize)>,
        parent: Option<TaskId>,
        depth: usize,
    ) {
        for task in visible.get(&parent).into_iter().flatten() {
            let (subtask_count, completed_subtask_count) =
                all.get(&task.id).copied().unwrap_or_default();
            rows.push(TaskRow {
                task: (*task).clone(),
                depth,
                child_count: visible.get(&Some(task.id)).map_or(0, Vec::len),
                subtask_count,
                completed_subtask_count,
            });
            if !task.collapsed {
                append(rows, visible, all, Some(task.id), depth + 1);
            }
        }
    }
    let mut rows = Vec::new();
    append(&mut rows, &visible, &all, None, 0);
    rows
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
    }
}
