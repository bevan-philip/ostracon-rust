//! Notifications through `UNUserNotificationCenter`, scheduled ahead so they fire even when the app is closed.
//! Adapted from gpui-pre-macos's `system_notifications.rs`, which only posts immediately.

use std::collections::HashSet;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use jiff::Timestamp;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationDefaultActionIdentifier,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNNotificationSound, UNTimeIntervalNotificationTrigger,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::{NotificationResponse, Permission, task_of};
use crate::state::PendingNotification;

const CATEGORY: &str = "ostracon-task";
const COMPLETE_ACTION: &str = "complete";

pub struct Notifier {
    center: Retained<UNUserNotificationCenter>,
    /// The center holds its delegate weakly.
    _delegate: Retained<Delegate>,
    responses: Receiver<NotificationResponse>,
    permission: Arc<AtomicU8>,
    /// Identifiers this process has asked to be pending.
    scheduled: HashSet<String>,
}

impl Notifier {
    pub fn new() -> Option<Self> {
        // The notification center aborts the process outside an app bundle, which is how `cargo run` runs.
        NSBundle::mainBundle().bundleIdentifier()?;
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let (sender, responses) = channel();
        let delegate = Delegate::new(sender);
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        let complete = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(COMPLETE_ACTION),
            &NSString::from_str("Complete"),
            UNNotificationActionOptions::empty(),
        );
        let category =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(CATEGORY),
                &NSArray::from_retained_slice(&[complete]),
                &NSArray::new(),
                UNNotificationCategoryOptions::empty(),
            );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
        // Start from a clean slate: whatever an earlier run left pending is rescheduled from the database.
        center.removeAllPendingNotificationRequests();

        let notifier = Notifier {
            center,
            _delegate: delegate,
            responses,
            permission: Arc::new(AtomicU8::new(0)),
            scheduled: HashSet::new(),
        };
        notifier.refresh_permission();
        Some(notifier)
    }

    pub fn schedule(&mut self, notifications: &[PendingNotification]) {
        let wanted: HashSet<&str> = notifications
            .iter()
            .map(|n| n.identifier.as_str())
            .collect();
        let stale: Vec<Retained<NSString>> = self
            .scheduled
            .iter()
            .filter(|id| !wanted.contains(id.as_str()))
            .map(|id| NSString::from_str(id))
            .collect();
        if !stale.is_empty() {
            self.center
                .removePendingNotificationRequestsWithIdentifiers(&NSArray::from_retained_slice(
                    &stale,
                ));
        }
        self.scheduled.retain(|id| wanted.contains(id.as_str()));

        let now = Timestamp::now();
        let new: Vec<&PendingNotification> = notifications
            .iter()
            .filter(|n| !self.scheduled.contains(&n.identifier))
            .collect();
        if new.is_empty() {
            return;
        }
        self.request_authorization();
        for notification in new {
            let seconds = notification.fire_at.duration_since(now).as_secs_f64();
            if seconds <= 0.0 {
                continue;
            }
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notification.title));
            content.setBody(&NSString::from_str(&notification.body));
            content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let trigger =
                UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(seconds, false);
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&notification.identifier),
                &content,
                Some(&trigger),
            );
            let completion = RcBlock::new(|error: *mut NSError| {
                // SAFETY: when non-null, `error` is a valid NSError for the duration of the callback.
                if let Some(error) = unsafe { error.as_ref() } {
                    eprintln!(
                        "ostracon: couldn't schedule a notification: {}",
                        error.localizedDescription()
                    );
                }
            });
            self.center
                .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
            self.scheduled.insert(notification.identifier.clone());
        }
    }

    fn request_authorization(&self) {
        if self.permission() != Permission::Unknown {
            return;
        }
        let permission = Arc::clone(&self.permission);
        let completion = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            // Not granted isn't the same as denied: the request also fails while the user hasn't answered
            // (for one, when the app launched in the background), and that mustn't show the "off" banner.
            if granted.as_bool() {
                permission.store(1, Ordering::Relaxed);
            } else {
                read_permission(
                    &UNUserNotificationCenter::currentNotificationCenter(),
                    Arc::clone(&permission),
                );
            }
        });
        self.center
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &completion,
            );
    }

    pub fn refresh_permission(&self) {
        read_permission(&self.center, Arc::clone(&self.permission));
    }

    pub fn permission(&self) -> Permission {
        match self.permission.load(Ordering::Relaxed) {
            1 => Permission::Granted,
            2 => Permission::Denied,
            _ => Permission::Unknown,
        }
    }

    pub fn take_responses(&self) -> Vec<NotificationResponse> {
        self.responses.try_iter().collect()
    }
}

struct DelegateIvars {
    sender: Mutex<Sender<NotificationResponse>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Delegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // Called when the user activates a delivered notification, possibly off the main thread; the channel
        // hands the response to the app's poll.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_notification_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let identifier = response.notification().request().identifier().to_string();
            let action = response.actionIdentifier();
            if let Some(task) = task_of(&identifier) {
                let response = if action.to_string() == COMPLETE_ACTION {
                    Some(NotificationResponse::Complete(task))
                } else if &*action == unsafe { UNNotificationDefaultActionIdentifier } {
                    Some(NotificationResponse::Open(task))
                } else {
                    None
                };
                if let (Some(response), Ok(sender)) = (response, self.ivars().sender.lock()) {
                    let _ = sender.send(response);
                }
            }
            completion_handler.call(());
        }

        // Without this, macOS suppresses banners while the app is frontmost.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present_notification(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

impl Delegate {
    fn new(sender: Sender<NotificationResponse>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars {
            sender: Mutex::new(sender),
        });
        // SAFETY: NSObject's init is its designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// Stores the system's actual authorization status in `permission`, when it arrives.
fn read_permission(center: &UNUserNotificationCenter, permission: Arc<AtomicU8>) {
    let completion = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: the settings object is valid for the duration of the callback.
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let value = match status {
            UNAuthorizationStatus::Denied => 2,
            UNAuthorizationStatus::NotDetermined => 0,
            _ => 1,
        };
        permission.store(value, Ordering::Relaxed);
    });
    center.getNotificationSettingsWithCompletionHandler(&completion);
}
