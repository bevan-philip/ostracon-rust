//! Tasks as an indented outline.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

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

/// A project's tasks in display order: its open tasks as an outline, depth-first, then, if `show_completed`,
/// its completed ones below them, as Todoist does. Collapsed tasks hide their descendants.
///
/// The completed part lists each completed task whose parent is open (or that has none) at the top level,
/// most recently completed first, with its subtasks beneath it.
pub fn rows(tasks: &[Task], project: ProjectId, show_completed: bool) -> Vec<TaskRow> {
    let in_project: Vec<&Task> = tasks.iter().filter(|t| t.project_id == project).collect();
    let mut open: HashMap<Option<TaskId>, Vec<&Task>> = HashMap::new();
    let mut every: HashMap<Option<TaskId>, Vec<&Task>> = HashMap::new();
    let mut counts: HashMap<TaskId, (usize, usize)> = HashMap::new();
    for task in &in_project {
        if let Some(parent) = task.parent_id {
            let counts = counts.entry(parent).or_default();
            counts.0 += 1;
            counts.1 += usize::from(task.is_completed());
        }
        every.entry(task.parent_id).or_default().push(task);
        if !task.is_completed() {
            open.entry(task.parent_id).or_default().push(task);
        }
    }
    for siblings in open.values_mut().chain(every.values_mut()) {
        siblings.sort_by_key(|t| (t.position, t.id));
    }

    let row = |task: &Task, depth: usize, children: &HashMap<Option<TaskId>, Vec<&Task>>| {
        let (subtask_count, completed_subtask_count) =
            counts.get(&task.id).copied().unwrap_or_default();
        TaskRow {
            task: task.clone(),
            depth,
            child_count: children.get(&Some(task.id)).map_or(0, Vec::len),
            subtask_count,
            completed_subtask_count,
        }
    };
    fn append(
        rows: &mut Vec<TaskRow>,
        children: &HashMap<Option<TaskId>, Vec<&Task>>,
        row: &impl Fn(&Task, usize, &HashMap<Option<TaskId>, Vec<&Task>>) -> TaskRow,
        siblings: &[&Task],
        depth: usize,
    ) {
        for task in siblings {
            rows.push(row(task, depth, children));
            if !task.collapsed {
                let below = children.get(&Some(task.id)).map_or(&[][..], Vec::as_slice);
                append(rows, children, row, below, depth + 1);
            }
        }
    }

    let mut rows = Vec::new();
    let roots = open.get(&None).map_or(&[][..], Vec::as_slice);
    append(&mut rows, &open, &row, roots, 0);
    if show_completed {
        let open_ids: HashSet<TaskId> = open.values().flatten().map(|t| t.id).collect();
        let mut heads: Vec<&Task> = in_project
            .iter()
            .copied()
            .filter(|t| t.is_completed() && t.parent_id.is_none_or(|p| open_ids.contains(&p)))
            .collect();
        heads.sort_by_key(|t| (Reverse(t.completed_at), t.id));
        append(&mut rows, &every, &row, &heads, 0);
    }
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
    fn completed_tasks_follow_the_outline_newest_first() {
        let tasks = [
            task(1, None, None),
            task(2, Some(1), Some(10)),
            task(3, Some(1), None),
            task(4, None, Some(30)),
            task(5, Some(4), Some(30)),
            task(6, None, Some(20)),
        ];
        assert_eq!(shown(&tasks, false), ["1", " 3"]);
        assert_eq!(shown(&tasks, true), ["1", " 3", "4", " 5", "6", "2"]);
        let outline = rows(&tasks, ProjectId(1), true);
        assert_eq!(
            (outline[0].child_count, outline[2].child_count),
            (1, 1),
            "an open parent's completed subtasks are listed apart from it"
        );
    }
}
