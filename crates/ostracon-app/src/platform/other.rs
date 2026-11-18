//! Platforms without notifications.

use super::{NotificationResponse, Permission};
use crate::state::PendingNotification;

pub struct Notifier;

impl Notifier {
    pub fn new() -> Option<Self> {
        None
    }

    pub fn schedule(&mut self, _: &[PendingNotification]) {}

    pub fn refresh_permission(&self) {}

    pub fn permission(&self) -> Permission {
        Permission::Unknown
    }

    pub fn take_responses(&self) -> Vec<NotificationResponse> {
        Vec::new()
    }
}
