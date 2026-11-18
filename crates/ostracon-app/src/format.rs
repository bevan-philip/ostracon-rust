//! How dates and reminders are written on screen, Todoist style.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use ostracon_core::model::days_between;
use ostracon_core::{Due, Reminder};

/// How pressing a due date is, which picks its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Overdue,
    Today,
    Tomorrow,
    /// Within the next week.
    Soon,
    Later,
}

pub fn urgency(due: Due, now: &ZonedDateTime) -> Urgency {
    if due.is_overdue(now) {
        return Urgency::Overdue;
    }
    match due.days_from(now) {
        0 => Urgency::Today,
        1 => Urgency::Tomorrow,
        2..=6 => Urgency::Soon,
        _ => Urgency::Later,
    }
}

/// "Today", "Tomorrow 17:00", "Friday", "27 Jan", "27 Jan 2027".
pub fn due_label(due: Due, now: &ZonedDateTime) -> String {
    let day = day_label(due.date, now);
    match due.time {
        Some(time) => format!("{day} {}", time.strftime("%H:%M")),
        None => day,
    }
}

pub fn day_label(date: Date, now: &ZonedDateTime) -> String {
    match days_between(now.date(), date) {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => date.strftime("%A").to_string(),
        _ if date.year() == now.year() => date.strftime("%-d %b").to_string(),
        _ => date.strftime("%-d %b %Y").to_string(),
    }
}

pub fn moment_label(at: Timestamp, now: &ZonedDateTime) -> String {
    let local = ZonedDateTime::new(at, now.time_zone().clone());
    format!(
        "{} {}",
        day_label(local.date(), now),
        local.strftime("%H:%M")
    )
}

/// "30 min before", "1 hour before", "Tomorrow 09:00".
pub fn reminder_label(reminder: Reminder, now: &ZonedDateTime) -> String {
    match reminder {
        Reminder::Before { minutes } => before_label(minutes),
        Reminder::At(at) => moment_label(at, now),
    }
}

pub fn before_label(minutes: i64) -> String {
    let (amount, unit) = match minutes {
        0 => return "At due time".into(),
        m if m % 10080 == 0 => (m / 10080, "week"),
        m if m % 1440 == 0 => (m / 1440, "day"),
        m if m % 60 == 0 => (m / 60, "hour"),
        m => (m, "min"),
    };
    let s = if amount == 1 || unit == "min" {
        ""
    } else {
        "s"
    };
    format!("{amount} {unit}{s} before")
}

/// Whether a relative reminder can fire for this due date.
pub fn reminder_is_inert(reminder: Reminder, due: Option<Due>) -> bool {
    matches!(reminder, Reminder::Before { .. }) && reminder.fire_at(due, &TimeZone::UTC).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    fn now() -> ZonedDateTime {
        date(2026, 1, 14).at(10, 30, 0, 0).in_tz("UTC").unwrap()
    }

    #[test]
    fn labels() {
        assert_eq!(due_label(Due::day(date(2026, 1, 14)), &now()), "Today");
        assert_eq!(
            due_label(Due::at(date(2026, 1, 15), time(17, 0, 0, 0)), &now()),
            "Tomorrow 17:00"
        );
        assert_eq!(due_label(Due::day(date(2026, 1, 16)), &now()), "Friday");
        assert_eq!(due_label(Due::day(date(2026, 2, 3)), &now()), "3 Feb");
        assert_eq!(due_label(Due::day(date(2027, 2, 3)), &now()), "3 Feb 2027");
        assert_eq!(before_label(30), "30 min before");
        assert_eq!(before_label(120), "2 hours before");
        assert_eq!(before_label(1440), "1 day before");
    }

    #[test]
    fn urgencies() {
        assert_eq!(
            urgency(Due::day(date(2026, 1, 13)), &now()),
            Urgency::Overdue
        );
        assert_eq!(
            urgency(Due::at(date(2026, 1, 14), time(9, 0, 0, 0)), &now()),
            Urgency::Overdue
        );
        assert_eq!(
            urgency(Due::at(date(2026, 1, 14), time(11, 0, 0, 0)), &now()),
            Urgency::Today
        );
        assert_eq!(
            urgency(Due::day(date(2026, 1, 15)), &now()),
            Urgency::Tomorrow
        );
        assert_eq!(urgency(Due::day(date(2026, 1, 19)), &now()), Urgency::Soon);
        assert_eq!(urgency(Due::day(date(2026, 1, 30)), &now()), Urgency::Later);
    }
}
