//! Everything that is done only on macOS lives here, behind one interface; the rest of the app doesn't depend
//! on how it is done. At present that is notifications: scheduling them, the Complete action, opening a task
//! from one, and checking whether permission was granted. Elsewhere [`Notifier::new`] returns `None`, and
//! reminders are still stored, edited and shown; they just don't notify.

use ostracon_core::TaskId;

use crate::state::PendingNotification;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
use other as imp;

/// What the user did with a delivered notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationResponse {
    /// Clicked it: show the task.
    Open(TaskId),
    /// Pressed Complete.
    Complete(TaskId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Unknown,
    Granted,
    /// Turned off for the app in System Settings, so reminders won't appear.
    Denied,
}

pub struct Notifier(imp::Notifier);

impl Notifier {
    /// The system's notifier, or `None` where there isn't one (other platforms, or a binary that isn't in an
    /// app bundle, such as `cargo run`).
    pub fn new() -> Option<Self> {
        imp::Notifier::new().map(Notifier)
    }

    /// Makes `notifications` the pending set: new ones are added and ones no longer wanted are removed.
    /// Cheap when nothing has changed.
    pub fn schedule(&mut self, notifications: &[PendingNotification]) {
        self.0.schedule(notifications);
    }

    /// Asks the system again whether notifications are allowed; [`Notifier::permission`] reflects it shortly.
    pub fn refresh_permission(&self) {
        self.0.refresh_permission();
    }

    pub fn permission(&self) -> Permission {
        self.0.permission()
    }

    /// Responses that arrived since the last call.
    pub fn take_responses(&self) -> Vec<NotificationResponse> {
        self.0.take_responses()
    }
}

/// The task a notification identifier (`task-<id>-<seconds>`) is for.
pub fn task_of(identifier: &str) -> Option<TaskId> {
    let rest = identifier.strip_prefix("task-")?;
    rest.split('-').next()?.parse().ok().map(TaskId)
}

#[cfg(test)]
mod tests {
    #[test]
    fn identifiers_name_their_task() {
        assert_eq!(
            super::task_of("task-12-1700000000"),
            Some(ostracon_core::TaskId(12))
        );
        assert_eq!(super::task_of("other"), None);
    }
}
