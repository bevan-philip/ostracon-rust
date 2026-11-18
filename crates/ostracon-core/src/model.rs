//! Projects, tasks, due dates and reminders.

use std::fmt;
use std::str::FromStr;

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use serde::{Deserialize, Serialize};

/// A project's database ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub i64);

/// A task's database ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub i64);

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub color: ProjectColor,
    /// Sidebar order among user projects. The Inbox always comes first.
    pub position: i64,
    pub is_inbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub parent_id: Option<TaskId>,
    pub name: String,
    pub description: String,
    /// Order among siblings.
    pub position: i64,
    /// When it was completed; `None` while open.
    pub completed_at: Option<Timestamp>,
    pub collapsed: bool,
    pub created_at: Timestamp,
    pub due: Option<Due>,
    /// Sorted with [`Reminder::sort`].
    pub reminders: Vec<Reminder>,
}

impl Task {
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

/// When a task is due: a calendar day, optionally at a time of day.
///
/// Stored as floating local time, as in Todoist, so "Friday at 9:00" stays 9:00 if the time zone changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Due {
    pub date: Date,
    pub time: Option<Time>,
}

impl Due {
    pub fn day(date: Date) -> Self {
        Due { date, time: None }
    }

    pub fn at(date: Date, time: Time) -> Self {
        Due {
            date,
            time: Some(time),
        }
    }

    pub fn with_time(self, time: Option<Time>) -> Self {
        Due { time, ..self }
    }

    pub fn has_time(&self) -> bool {
        self.time.is_some()
    }

    /// The due moment in `tz`, or the start of the day when there is no time.
    pub fn moment(&self, tz: &TimeZone) -> Timestamp {
        local_moment(self.date, self.time.unwrap_or(Time::midnight()), tz)
    }

    /// Whole days from `now`'s day to the due day: 0 for today, negative when past.
    pub fn days_from(&self, now: &ZonedDateTime) -> i64 {
        days_between(now.date(), self.date)
    }

    /// Overdue once the due time has passed, or for all-day tasks once the day is over.
    pub fn is_overdue(&self, now: &ZonedDateTime) -> bool {
        if self.has_time() {
            self.moment(now.time_zone()) < now.timestamp()
        } else {
            self.date < now.date()
        }
    }

    /// `yyyy-mm-dd`, or `yyyy-mm-ddThh:mm` with a time.
    pub fn storage_value(&self) -> String {
        match self.time {
            None => self.date.strftime("%Y-%m-%d").to_string(),
            Some(time) => format!(
                "{}T{}",
                self.date.strftime("%Y-%m-%d"),
                time.strftime("%H:%M")
            ),
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        let (day, time) = match value.split_once('T') {
            Some((day, time)) => (day, Some(time)),
            None => (value, None),
        };
        let date = Date::strptime("%Y-%m-%d", day).ok()?;
        let time = match time {
            Some(time) => Some(Time::strptime("%H:%M", time).ok()?),
            None => None,
        };
        Some(Due { date, time })
    }
}

/// The moment `date` at `time` happens in `tz`, taking the later reading in a gap (as clocks go forward).
pub fn local_moment(date: Date, time: Time, tz: &TimeZone) -> Timestamp {
    date.to_datetime(time)
        .to_zoned_datetime(tz.clone())
        .map(|zoned: ZonedDateTime| zoned.timestamp())
        .unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Whole days from `from` to `to`.
pub fn days_between(from: Date, to: Date) -> i64 {
    from.until((jiff::Unit::Day, to))
        .map(|span| i64::from(span.get_days()))
        .unwrap_or(0)
}

/// When to notify about a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reminder {
    /// At a fixed moment.
    At(Timestamp),
    /// A number of minutes before the task's due time. Has no effect on tasks without one.
    Before { minutes: i64 },
}

impl Reminder {
    /// When this goes off for a task due at `due`, or `None` if it can't.
    pub fn fire_at(&self, due: Option<Due>, tz: &TimeZone) -> Option<Timestamp> {
        match *self {
            Reminder::At(moment) => Some(moment),
            Reminder::Before { minutes } => {
                let due = due.filter(Due::has_time)?;
                due.moment(tz)
                    .checked_sub(jiff::SignedDuration::from_mins(minutes))
                    .ok()
            }
        }
    }

    /// Absolute reminders first, in time order, then relative ones from the smallest offset to the largest.
    pub fn sort(reminders: &mut Vec<Reminder>) {
        reminders.sort_by_key(|reminder| match *reminder {
            Reminder::At(moment) => (0, moment.as_second()),
            Reminder::Before { minutes } => (1, minutes),
        });
        reminders.dedup();
    }
}

/// The Todoist project colour palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectColor {
    BerryRed,
    Red,
    Orange,
    Yellow,
    OliveGreen,
    LimeGreen,
    Green,
    MintGreen,
    Teal,
    SkyBlue,
    LightBlue,
    Blue,
    Grape,
    Violet,
    Lavender,
    Magenta,
    Salmon,
    Charcoal,
    Grey,
    Taupe,
}

impl ProjectColor {
    pub const ALL: [ProjectColor; 20] = [
        ProjectColor::BerryRed,
        ProjectColor::Red,
        ProjectColor::Orange,
        ProjectColor::Yellow,
        ProjectColor::OliveGreen,
        ProjectColor::LimeGreen,
        ProjectColor::Green,
        ProjectColor::MintGreen,
        ProjectColor::Teal,
        ProjectColor::SkyBlue,
        ProjectColor::LightBlue,
        ProjectColor::Blue,
        ProjectColor::Grape,
        ProjectColor::Violet,
        ProjectColor::Lavender,
        ProjectColor::Magenta,
        ProjectColor::Salmon,
        ProjectColor::Charcoal,
        ProjectColor::Grey,
        ProjectColor::Taupe,
    ];

    /// The stable identifier used in the database and the tools, such as `lime_green`.
    pub fn key(self) -> &'static str {
        match self {
            ProjectColor::BerryRed => "berry_red",
            ProjectColor::Red => "red",
            ProjectColor::Orange => "orange",
            ProjectColor::Yellow => "yellow",
            ProjectColor::OliveGreen => "olive_green",
            ProjectColor::LimeGreen => "lime_green",
            ProjectColor::Green => "green",
            ProjectColor::MintGreen => "mint_green",
            ProjectColor::Teal => "teal",
            ProjectColor::SkyBlue => "sky_blue",
            ProjectColor::LightBlue => "light_blue",
            ProjectColor::Blue => "blue",
            ProjectColor::Grape => "grape",
            ProjectColor::Violet => "violet",
            ProjectColor::Lavender => "lavender",
            ProjectColor::Magenta => "magenta",
            ProjectColor::Salmon => "salmon",
            ProjectColor::Charcoal => "charcoal",
            ProjectColor::Grey => "grey",
            ProjectColor::Taupe => "taupe",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ProjectColor::BerryRed => "Berry Red",
            ProjectColor::Red => "Red",
            ProjectColor::Orange => "Orange",
            ProjectColor::Yellow => "Yellow",
            ProjectColor::OliveGreen => "Olive Green",
            ProjectColor::LimeGreen => "Lime Green",
            ProjectColor::Green => "Green",
            ProjectColor::MintGreen => "Mint Green",
            ProjectColor::Teal => "Teal",
            ProjectColor::SkyBlue => "Sky Blue",
            ProjectColor::LightBlue => "Light Blue",
            ProjectColor::Blue => "Blue",
            ProjectColor::Grape => "Grape",
            ProjectColor::Violet => "Violet",
            ProjectColor::Lavender => "Lavender",
            ProjectColor::Magenta => "Magenta",
            ProjectColor::Salmon => "Salmon",
            ProjectColor::Charcoal => "Charcoal",
            ProjectColor::Grey => "Grey",
            ProjectColor::Taupe => "Taupe",
        }
    }

    /// The colour as 24-bit RGB.
    pub fn rgb(self) -> u32 {
        match self {
            ProjectColor::BerryRed => 0xB8255F,
            ProjectColor::Red => 0xDC4C3E,
            ProjectColor::Orange => 0xC77100,
            ProjectColor::Yellow => 0xB29104,
            ProjectColor::OliveGreen => 0x949C31,
            ProjectColor::LimeGreen => 0x65A33A,
            ProjectColor::Green => 0x369307,
            ProjectColor::MintGreen => 0x42A393,
            ProjectColor::Teal => 0x148FAD,
            ProjectColor::SkyBlue => 0x319DC0,
            ProjectColor::LightBlue => 0x6988A4,
            ProjectColor::Blue => 0x4180FF,
            ProjectColor::Grape => 0x692EC2,
            ProjectColor::Violet => 0xCA3FEE,
            ProjectColor::Lavender => 0xA4698C,
            ProjectColor::Magenta => 0xE05095,
            ProjectColor::Salmon => 0xC9766F,
            ProjectColor::Charcoal => 0x808080,
            ProjectColor::Grey => 0x999999,
            ProjectColor::Taupe => 0x8F7A69,
        }
    }
}

impl fmt::Display for ProjectColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Accepts the key (`lime_green`) and looser spellings such as `Lime Green`, `lime-green` or `limeGreen`.
impl FromStr for ProjectColor {
    type Err = UnknownColor;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let wanted: String = text
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(char::to_lowercase)
            .collect();
        ProjectColor::ALL
            .into_iter()
            .find(|color| color.key().replace('_', "") == wanted)
            .ok_or_else(|| UnknownColor(text.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown colour \"{0}\"")]
pub struct UnknownColor(pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn due_storage_round_trips() {
        let day = Due::day(date(2026, 1, 27));
        assert_eq!(day.storage_value(), "2026-01-27");
        assert_eq!(Due::from_storage("2026-01-27"), Some(day));
        let timed = Due::at(date(2026, 1, 27), Time::new(17, 5, 0, 0).unwrap());
        assert_eq!(timed.storage_value(), "2026-01-27T17:05");
        assert_eq!(Due::from_storage("2026-01-27T17:05"), Some(timed));
        assert_eq!(Due::from_storage("2026-13-01"), None);
        assert_eq!(Due::from_storage("nonsense"), None);
    }

    #[test]
    fn colours_parse_loosely() {
        assert_eq!("lime green".parse(), Ok(ProjectColor::LimeGreen));
        assert_eq!("Lime-Green".parse(), Ok(ProjectColor::LimeGreen));
        assert_eq!("limeGreen".parse(), Ok(ProjectColor::LimeGreen));
        assert_eq!("lime_green".parse(), Ok(ProjectColor::LimeGreen));
        assert!("puce".parse::<ProjectColor>().is_err());
    }

    #[test]
    fn relative_reminders_need_a_due_time() {
        let tz = TimeZone::UTC;
        let reminder = Reminder::Before { minutes: 30 };
        assert_eq!(
            reminder.fire_at(Some(Due::day(date(2026, 1, 1))), &tz),
            None
        );
        let due = Due::at(date(2026, 1, 1), Time::new(9, 0, 0, 0).unwrap());
        assert_eq!(
            reminder.fire_at(Some(due), &tz),
            Some("2026-01-01T08:30:00Z".parse().unwrap())
        );
    }
}
