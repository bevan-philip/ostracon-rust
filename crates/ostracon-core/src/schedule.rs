//! Which notifications to deliver.

use std::collections::BTreeSet;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::model::{Task, TaskId};

/// A notification to deliver for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScheduledReminder {
    pub fire_at: Timestamp,
    pub task_id: TaskId,
}

impl ScheduledReminder {
    /// Stable across reloads, so an unchanged reminder keeps its pending notification.
    pub fn identifier(&self) -> String {
        format!("task-{}-{}", self.task_id, self.fire_at.as_second())
    }
}

/// macOS keeps at most 64 pending notifications per app; later ones wait for a future reschedule.
pub const NOTIFICATION_LIMIT: usize = 64;

/// The upcoming notifications for open tasks, soonest first. A task with a due time is notified at that time
/// as well as at any reminders of its own.
pub fn upcoming(
    tasks: &[Task],
    after: Timestamp,
    tz: &TimeZone,
    limit: usize,
) -> Vec<ScheduledReminder> {
    let mut scheduled = BTreeSet::new();
    for task in tasks.iter().filter(|t| !t.is_completed()) {
        let due_time = task.due.filter(|d| d.has_time()).map(|d| d.moment(tz));
        let reminders = task
            .reminders
            .iter()
            .filter_map(|r| r.fire_at(task.due, tz));
        for fire_at in reminders.chain(due_time).filter(|&at| at > after) {
            scheduled.insert(ScheduledReminder {
                fire_at,
                task_id: task.id,
            });
        }
    }
    scheduled.into_iter().take(limit).collect()
}
