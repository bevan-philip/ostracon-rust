//! Property tests of the notification schedule against a brute-force recomputation.

use jiff::civil::{Time, date};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use ostracon_core::schedule::{self, ScheduledReminder};
use ostracon_core::{Due, ProjectId, Reminder, Task, TaskId};
use proptest::prelude::*;

fn reminder() -> impl Strategy<Value = Reminder> {
    prop_oneof![
        (-48i64..48).prop_map(|h| Reminder::At(base() + SignedDuration::from_mins(h * 30))),
        (0i64..3000).prop_map(|minutes| Reminder::Before { minutes }),
    ]
}

fn base() -> Timestamp {
    "2026-03-29T00:30:00Z".parse().unwrap()
}

/// Whether it's completed; its due day (from 29 March) and time; its reminders.
type TaskSpec = (bool, Option<(i64, Option<(i8, i8)>)>, Vec<Reminder>);

fn task() -> impl Strategy<Value = TaskSpec> {
    (
        proptest::bool::weighted(0.2),
        proptest::option::weighted(
            0.7,
            (
                -1i64..=2,
                proptest::option::of((0i8..24, proptest::sample::select(vec![0i8, 15, 30]))),
            ),
        ),
        proptest::collection::vec(reminder(), 0..4),
    )
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1024),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn upcoming_is_every_future_notification_soonest_first(
        specs in proptest::collection::vec(task(), 0..10),
        after_mins in -600i64..3000,
        limit in 0usize..12,
        zone in proptest::sample::select(vec!["UTC", "Europe/London", "America/New_York"]),
    ) {
        let tz = TimeZone::get(zone).unwrap();
        let tasks: Vec<Task> = specs
            .iter()
            .enumerate()
            .map(|(i, (completed, due, reminders))| Task {
                id: TaskId(i as i64 + 1),
                project_id: ProjectId(1),
                parent_id: None,
                name: String::new(),
                description: String::new(),
                position: 0,
                completed_at: completed.then_some(Timestamp::UNIX_EPOCH),
                collapsed: false,
                created_at: Timestamp::UNIX_EPOCH,
                due: due.map(|(days, time)| Due {
                    // Around the night the clocks go forward in London.
                    date: date(2026, 3, 29).checked_add(jiff::Span::new().days(days)).unwrap(),
                    time: time.map(|(h, m)| Time::new(h, m, 0, 0).unwrap()),
                }),
                reminders: reminders.clone(),
            })
            .collect();
        let after = base() + SignedDuration::from_mins(after_mins);

        let mut expected: Vec<ScheduledReminder> = Vec::new();
        for task in tasks.iter().filter(|t| !t.is_completed()) {
            let mut moments: Vec<Timestamp> = Vec::new();
            if let Some(due) = task.due.filter(|d| d.time.is_some()) {
                let local = due.date.to_datetime(due.time.unwrap());
                let due_at = local.to_zoned_datetime(tz.clone()).unwrap().timestamp();
                moments.push(due_at);
                for reminder in &task.reminders {
                    if let Reminder::Before { minutes } = reminder {
                        moments.push(due_at - SignedDuration::from_mins(*minutes));
                    }
                }
            }
            for reminder in &task.reminders {
                if let Reminder::At(at) = reminder {
                    moments.push(*at);
                }
            }
            for fire_at in moments.into_iter().filter(|&m| m > after) {
                let scheduled = ScheduledReminder { fire_at, task_id: task.id };
                if !expected.contains(&scheduled) {
                    expected.push(scheduled);
                }
            }
        }
        expected.sort();
        expected.truncate(limit);
        prop_assert_eq!(schedule::upcoming(&tasks, after, &tz, limit), expected);
    }
}
