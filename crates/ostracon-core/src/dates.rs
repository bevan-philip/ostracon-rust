//! Todoist-style natural-language dates and `!reminders` in task text.
//!
//! Due dates: `today`, `tomorrow`, `tonight`, `friday`, `this friday`, `next friday`, `next week`, `weekend`,
//! `next month`, `end of month`, `in 3 days`, `in 2 hours`, `jan 27`, `27th january 2027`, `27/1`,
//! `2027-01-27`, each optionally with a time such as `at 5pm`, `17:30`, `9:15am` or `noon`. A time on its own
//! means the next time the clock reads it.
//!
//! Reminders start with `!`: `!30min` or `!1 hour before` (relative to the due time), or `!tomorrow 9am`,
//! `!3pm` (a fixed moment; a bare time falls on the due day).

use std::ops::Range;
use std::sync::LazyLock;

use jiff::civil::{Date, Time, Weekday};
use jiff::{Span, ZonedDateTime};
use regex::Regex;

use crate::model::{Due, Reminder, local_moment};

/// A day and/or time as written, before defaults are filled in. [`DateParser::resolve`] makes it a [`Due`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DateFragment {
    pub day: Option<Date>,
    pub time: Option<Time>,
}

impl DateFragment {
    fn day(day: Date) -> Self {
        DateFragment {
            day: Some(day),
            time: None,
        }
    }

    /// One gives only a day and the other only a time, so together they make a whole date ("5pm … tomorrow").
    pub fn complements(&self, other: &DateFragment) -> bool {
        (self.is_day_only() && other.is_time_only()) || (self.is_time_only() && other.is_day_only())
    }

    pub fn combined(&self, other: &DateFragment) -> DateFragment {
        DateFragment {
            day: self.day.or(other.day),
            time: self.time.or(other.time),
        }
    }

    fn is_day_only(&self) -> bool {
        self.day.is_some() && self.time.is_none()
    }

    fn is_time_only(&self) -> bool {
        self.day.is_none() && self.time.is_some()
    }
}

/// A reminder as written, before it's tied to a due date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReminderFragment {
    Before { minutes: i64 },
    At(DateFragment),
}

/// A date or reminder phrase found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    /// Byte range in the text.
    pub range: Range<usize>,
    pub value: PhraseValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhraseValue {
    Due(DateFragment),
    Reminder(ReminderFragment),
}

/// Where untimed reminders land.
pub const MORNING: Time = Time::constant(9, 0, 0, 0);

/// Reads dates relative to a moment, in its time zone.
#[derive(Debug, Clone)]
pub struct DateParser {
    now: ZonedDateTime,
    /// Whether `3/4` means 3 April (day first) or March 4.
    day_first: bool,
}

struct Match<T> {
    length: usize,
    value: T,
}

fn found<T>(length: usize, value: T) -> Option<Match<T>> {
    Some(Match { length, value })
}

#[derive(Debug, Clone)]
struct Token {
    /// Lowercased, without a leading `!` or trailing `,` or `.`.
    word: String,
    range: Range<usize>,
    /// Started with `!`.
    is_reminder: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl DateParser {
    /// A parser for `now`, with the numeric day/month order of the user's region.
    pub fn new(now: ZonedDateTime) -> Self {
        DateParser {
            now,
            day_first: crate::locale::day_first(),
        }
    }

    /// A parser for this moment in the system time zone.
    pub fn now() -> Self {
        DateParser::new(ZonedDateTime::now())
    }

    pub fn with_day_first(mut self, day_first: bool) -> Self {
        self.day_first = day_first;
        self
    }

    pub fn current(&self) -> &ZonedDateTime {
        &self.now
    }

    fn today(&self) -> Date {
        self.now.date()
    }

    /// Every date and `!reminder` phrase in `text`, left to right, taking the longest phrase at each position.
    pub fn phrases(&self, text: &str) -> Vec<Phrase> {
        let tokens = tokenize(text);
        let mut phrases = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let range =
                |length: usize| tokens[index].range.start..tokens[index + length - 1].range.end;
            if tokens[index].is_reminder {
                if let Some(m) = self.reminder_at(&tokens, index) {
                    phrases.push(Phrase {
                        range: range(m.length),
                        value: PhraseValue::Reminder(m.value),
                    });
                    index += m.length;
                    continue;
                }
            } else if let Some(m) = self.phrase_at(&tokens, index) {
                phrases.push(Phrase {
                    range: range(m.length),
                    value: PhraseValue::Due(m.value),
                });
                index += m.length;
                continue;
            }
            index += 1;
        }
        phrases
    }

    /// Reads the whole of `text` as a date. `None` unless all of it is understood.
    pub fn date_fragment(&self, text: &str) -> Option<DateFragment> {
        let tokens = tokenize(text);
        let m = self.phrase_at(&tokens, 0)?;
        (m.length == tokens.len()).then_some(m.value)
    }

    /// Reads the whole of `text` as a reminder, with or without its leading `!`.
    pub fn reminder_fragment(&self, text: &str) -> Option<ReminderFragment> {
        let trimmed = text.trim();
        let text = if trimmed.starts_with('!') {
            trimmed.to_owned()
        } else {
            format!("!{trimmed}")
        };
        let tokens = tokenize(&text);
        let m = self.reminder_at(&tokens, 0)?;
        (m.length == tokens.len()).then_some(m.value)
    }

    /// Reads the whole of `text` as a due date, for a "type a date" field.
    pub fn due(&self, text: &str) -> Option<Due> {
        self.date_fragment(text)
            .map(|fragment| self.resolve(fragment))
    }

    /// Reads the whole of `text` as a reminder for a task due at `due`.
    pub fn reminder(&self, text: &str, due: Option<Due>) -> Option<Reminder> {
        self.reminder_fragment(text)
            .map(|fragment| self.resolve_reminder(fragment, due))
    }

    /// A bare time means today, or tomorrow if that time has already gone.
    pub fn resolve(&self, fragment: DateFragment) -> Due {
        if let Some(day) = fragment.day {
            return Due {
                date: day,
                time: fragment.time,
            };
        }
        let time = fragment.time.unwrap_or(MORNING);
        let today = self.today();
        if local_moment(today, time, self.now.time_zone()) <= self.now.timestamp() {
            Due::at(today.tomorrow().unwrap_or(today), time)
        } else {
            Due::at(today, time)
        }
    }

    /// A bare reminder time falls on the due day if there is one; a bare day is reminded at 9am.
    pub fn resolve_reminder(&self, fragment: ReminderFragment, due: Option<Due>) -> Reminder {
        match fragment {
            ReminderFragment::Before { minutes } => Reminder::Before { minutes },
            ReminderFragment::At(date) => {
                let time = date.time.unwrap_or(MORNING);
                let tz = self.now.time_zone();
                if let (None, Some(due)) = (date.day, due) {
                    return Reminder::At(local_moment(due.date, time, tz));
                }
                let resolved = self.resolve(DateFragment {
                    day: date.day,
                    time: Some(time),
                });
                Reminder::At(resolved.moment(tz))
            }
        }
    }

    // Grammar

    /// `date [time]`, `time [on] [date]`, or `on date [time]`.
    fn phrase_at(&self, tokens: &[Token], index: usize) -> Option<Match<DateFragment>> {
        let token = tokens.get(index)?;
        if token.is_reminder {
            return None;
        }
        if token.word == "on"
            && let Some(m) = self
                .phrase_at(tokens, index + 1)
                .filter(|m| m.value.day.is_some())
        {
            return found(m.length + 1, m.value);
        }
        if let Some(date) = self.date_at(tokens, index) {
            if date.value.time.is_none()
                && let Some(time) = time_at(tokens, index + date.length)
            {
                return found(
                    date.length + time.length,
                    DateFragment {
                        day: date.value.day,
                        time: Some(time.value),
                    },
                );
            }
            return Some(date);
        }
        if let Some(time) = time_at(tokens, index) {
            let mut next = index + time.length;
            if tokens
                .get(next)
                .is_some_and(|t| t.word == "on" && !t.is_reminder)
            {
                next += 1;
            }
            if let Some(date) = self
                .date_at(tokens, next)
                .filter(|d| d.value.time.is_none())
            {
                return found(
                    next - index + date.length,
                    DateFragment {
                        day: date.value.day,
                        time: Some(time.value),
                    },
                );
            }
            return found(
                time.length,
                DateFragment {
                    day: None,
                    time: Some(time.value),
                },
            );
        }
        None
    }

    fn date_at(&self, tokens: &[Token], index: usize) -> Option<Match<DateFragment>> {
        let word = |offset: usize| -> Option<&str> {
            tokens
                .get(index + offset)
                .filter(|t| !t.is_reminder)
                .map(|t| t.word.as_str())
        };
        let day = |length: usize, date: Option<Date>| {
            date.map(|d| Match {
                length,
                value: DateFragment::day(d),
            })
        };
        let first = word(0)?;
        let today = self.today();

        match first {
            "today" => return day(1, Some(today)),
            "tomorrow" | "tmr" | "tmrw" => return day(1, today.tomorrow().ok()),
            "tonight" => {
                return found(
                    1,
                    DateFragment {
                        day: Some(today),
                        time: Some(Time::constant(19, 0, 0, 0)),
                    },
                );
            }
            "weekend" => return day(1, Some(self.this_weekend())),
            "eom" => return day(1, Some(today.last_of_month())),
            _ => {}
        }

        if let Some(weekday) = weekday(first) {
            return day(1, Some(self.next_weekday(weekday, false)));
        }

        if first == "this"
            && let Some(second) = word(1)
        {
            if second == "weekend" {
                return day(2, Some(self.this_weekend()));
            }
            if let Some(weekday) = weekday(second) {
                return day(2, Some(self.next_weekday(weekday, true)));
            }
        }

        if first == "next"
            && let Some(second) = word(1)
        {
            let monday = self.next_weekday(Weekday::Monday, false);
            match second {
                "week" => return day(2, Some(monday)),
                "weekend" => return day(2, monday.checked_add(Span::new().days(5)).ok()),
                "month" => {
                    return day(
                        2,
                        today
                            .first_of_month()
                            .checked_add(Span::new().months(1))
                            .ok(),
                    );
                }
                _ => {
                    if let Some(weekday) = weekday(second) {
                        let offset = i64::from(weekday.to_monday_zero_offset());
                        return day(2, monday.checked_add(Span::new().days(offset)).ok());
                    }
                }
            }
        }

        if first == "end" && word(1) == Some("of") {
            if word(2) == Some("month") {
                return day(3, Some(today.last_of_month()));
            }
            if word(2) == Some("the") && word(3) == Some("month") {
                return day(4, Some(today.last_of_month()));
            }
        }

        if first == "in"
            && let Some(amount_word) = word(1)
        {
            let amount = amount_word
                .parse::<i64>()
                .ok()
                .or_else(|| ["a", "an", "one"].contains(&amount_word).then_some(1));
            if let (Some(amount), Some(unit)) = (amount, word(2).and_then(duration_unit)) {
                return self
                    .offset(amount, unit)
                    .map(|value| Match { length: 3, value });
            }
            if let Some((amount, unit)) = compact_duration(amount_word) {
                return self
                    .offset(amount, unit)
                    .map(|value| Match { length: 2, value });
            }
        }

        // jan 27 [2027], 27th january [2027]
        if let (Some(month), Some(day_number)) = (month(first), word(1).and_then(day_number)) {
            let year = word(2).and_then(year);
            let length = if year.is_some() { 3 } else { 2 };
            return day(length, self.calendar_date(year, month, day_number));
        }
        if let (Some(day_number), Some(month)) = (day_number(first), word(1).and_then(month)) {
            let year = word(2).and_then(year);
            let length = if year.is_some() { 3 } else { 2 };
            return day(length, self.calendar_date(year, month, day_number));
        }

        day(1, self.numeric_date(first))
    }

    /// `!30min [before]`, `!1 hour before`, or `!` followed by a date and/or time.
    fn reminder_at(&self, tokens: &[Token], index: usize) -> Option<Match<ReminderFragment>> {
        let token = tokens.get(index)?;
        if !token.is_reminder || token.word.is_empty() {
            return None;
        }
        let mut tokens = tokens.to_vec();
        tokens[index].is_reminder = false;

        let before = |length: usize| {
            if tokens
                .get(index + length)
                .is_some_and(|t| t.word == "before" && !t.is_reminder)
            {
                length + 1
            } else {
                length
            }
        };
        let word = tokens[index].word.as_str();
        if let Some(minutes) =
            compact_duration(word).and_then(|(amount, unit)| minutes(amount, unit))
        {
            return found(before(1), ReminderFragment::Before { minutes });
        }
        if let Ok(amount) = word.parse::<i64>() {
            let unit = tokens
                .get(index + 1)
                .filter(|t| !t.is_reminder)
                .and_then(|t| duration_unit(&t.word));
            if let Some(minutes) = unit.and_then(|unit| minutes(amount, unit)) {
                return found(before(2), ReminderFragment::Before { minutes });
            }
        }
        self.phrase_at(&tokens, index).map(|m| Match {
            length: m.length,
            value: ReminderFragment::At(m.value),
        })
    }

    // Calendar helpers

    /// The next given weekday, or today if it is that day and `including_today`.
    fn next_weekday(&self, weekday: Weekday, including_today: bool) -> Date {
        let today = self.today();
        let current = i64::from(today.weekday().to_monday_zero_offset());
        let target = i64::from(weekday.to_monday_zero_offset());
        let mut days = (target - current).rem_euclid(7);
        if days == 0 && !including_today {
            days = 7;
        }
        today.checked_add(Span::new().days(days)).unwrap_or(today)
    }

    /// Today if it's Sunday, otherwise the coming Saturday (today, if it is Saturday).
    fn this_weekend(&self) -> Date {
        let today = self.today();
        if today.weekday() == Weekday::Sunday {
            today
        } else {
            self.next_weekday(Weekday::Saturday, true)
        }
    }

    fn offset(&self, amount: i64, unit: Unit) -> Option<DateFragment> {
        let span = match unit {
            Unit::Minute => Span::new().try_minutes(amount).ok()?,
            Unit::Hour => Span::new().try_hours(amount).ok()?,
            Unit::Day => Span::new().try_days(amount).ok()?,
            Unit::Week => Span::new().try_weeks(amount).ok()?,
            Unit::Month => Span::new().try_months(amount).ok()?,
            Unit::Year => Span::new().try_years(amount).ok()?,
        };
        let moment = self.now.checked_add(span).ok()?;
        match unit {
            Unit::Minute | Unit::Hour => {
                let time = moment.time();
                Some(DateFragment {
                    day: Some(moment.date()),
                    time: Time::new(time.hour(), time.minute(), 0, 0).ok(),
                })
            }
            _ => Some(DateFragment::day(moment.date())),
        }
    }

    /// A valid date; without a year, the next time that day comes round.
    fn calendar_date(&self, year: Option<i16>, month: i8, day: i8) -> Option<Date> {
        if let Some(year) = year {
            return Date::new(year, month, day).ok();
        }
        let today = self.today();
        match Date::new(today.year(), month, day) {
            Ok(date) if date >= today => Some(date),
            _ => Date::new(today.year() + 1, month, day).ok(),
        }
    }

    /// `2027-01-27`, or `27/1`, `27/1/27`, `27/01/2027` in the region's day–month order.
    fn numeric_date(&self, word: &str) -> Option<Date> {
        static ISO: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})$").unwrap());
        static SLASHED: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^(\d{1,2})/(\d{1,2})(?:/(\d{4}|\d{2}))?$").unwrap());
        if let Some(c) = ISO.captures(word) {
            return self.calendar_date(
                Some(c[1].parse().ok()?),
                c[2].parse().ok()?,
                c[3].parse().ok()?,
            );
        }
        let c = SLASHED.captures(word)?;
        let first: i8 = c[1].parse().ok()?;
        let second: i8 = c[2].parse().ok()?;
        let year = match c.get(3) {
            Some(y) => {
                let y: i16 = y.as_str().parse().ok()?;
                Some(if y < 100 { 2000 + y } else { y })
            }
            None => None,
        };
        let day_first = first > 12 || (second <= 12 && self.day_first);
        if day_first {
            self.calendar_date(year, second, first)
        } else {
            self.calendar_date(year, first, second)
        }
    }
}

/// `5pm`, `5 pm`, `5:30pm`, `17:30`, `noon`, `midnight`; after `at` or `@`, a bare hour such as `at 17`.
fn time_at(tokens: &[Token], index: usize) -> Option<Match<Time>> {
    static TIME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d{1,2})(?::(\d{2}))?(am|pm|a|p)?$").unwrap());
    let mut position = index;
    let has_marker = tokens
        .get(position)
        .is_some_and(|t| !t.is_reminder && (t.word == "at" || t.word == "@"));
    if has_marker {
        position += 1;
    }
    let token = tokens.get(position).filter(|t| !t.is_reminder)?;
    let consumed = position - index + 1;
    match token.word.as_str() {
        "noon" | "midday" => return found(consumed, Time::constant(12, 0, 0, 0)),
        "midnight" => return found(consumed, Time::midnight()),
        _ => {}
    }
    let c = TIME.captures(&token.word)?;
    let hour: i8 = c[1].parse().ok()?;
    let has_minutes = c.get(2).is_some();
    let minute: i8 = c.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
    if minute >= 60 {
        return None;
    }
    let mut meridiem = c.get(3).map(|m| m.as_str().to_owned());
    let mut length = consumed;
    if meridiem.is_none()
        && let Some(next) = tokens
            .get(position + 1)
            .filter(|t| !t.is_reminder && (t.word == "am" || t.word == "pm"))
    {
        meridiem = Some(next.word.clone());
        length += 1;
    }
    if let Some(meridiem) = meridiem {
        if !(1..=12).contains(&hour) {
            return None;
        }
        let pm = meridiem.starts_with('p');
        return found(
            length,
            Time::new(hour % 12 + if pm { 12 } else { 0 }, minute, 0, 0).ok()?,
        );
    }
    // Without am/pm, only clearly-a-time forms count, so "buy 2 apples" keeps its 2.
    if !(has_minutes || has_marker) || !(0..=23).contains(&hour) {
        return None;
    }
    found(length, Time::new(hour, minute, 0, 0).ok()?)
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut push = |range: Range<usize>| {
        let raw = text[range.clone()].to_lowercase();
        let is_reminder = raw.starts_with('!');
        let word = raw
            .strip_prefix('!')
            .unwrap_or(&raw)
            .trim_end_matches([',', '.'])
            .to_owned();
        tokens.push(Token {
            word,
            range,
            is_reminder,
        });
    };
    for (index, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                push(s..index);
                start = None;
            }
            (false, None) => start = Some(index),
            _ => {}
        }
    }
    if let Some(s) = start {
        push(s..text.len());
    }
    tokens
}

/// Full names, plus abbreviations unlikely to be ordinary words ("sat" and "sun" are left out).
fn weekday(word: &str) -> Option<Weekday> {
    Some(match word {
        "sunday" => Weekday::Sunday,
        "monday" | "mon" => Weekday::Monday,
        "tuesday" | "tue" | "tues" => Weekday::Tuesday,
        "wednesday" | "wed" => Weekday::Wednesday,
        "thursday" | "thu" | "thur" | "thurs" => Weekday::Thursday,
        "friday" | "fri" => Weekday::Friday,
        "saturday" => Weekday::Saturday,
        _ => return None,
    })
}

fn month(word: &str) -> Option<i8> {
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    if let Some(index) = NAMES.iter().position(|name| *name == word) {
        return Some(index as i8 + 1);
    }
    if word == "sept" {
        return Some(9);
    }
    if word.chars().count() != 3 {
        return None;
    }
    NAMES
        .iter()
        .position(|name| name.starts_with(word))
        .map(|index| index as i8 + 1)
}

/// `27`, `27th`, `1st`, `2nd`, `3rd`.
fn day_number(word: &str) -> Option<i8> {
    static DAY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d{1,2})(st|nd|rd|th)?$").unwrap());
    let day: i8 = DAY.captures(word)?[1].parse().ok()?;
    (1..=31).contains(&day).then_some(day)
}

fn year(word: &str) -> Option<i16> {
    (word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit()))
        .then(|| word.parse().ok())
        .flatten()
}

fn duration_unit(word: &str) -> Option<Unit> {
    Some(match word {
        "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minute,
        "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hour,
        "d" | "day" | "days" => Unit::Day,
        "w" | "wk" | "wks" | "week" | "weeks" => Unit::Week,
        "month" | "months" => Unit::Month,
        "y" | "yr" | "yrs" | "year" | "years" => Unit::Year,
        _ => return None,
    })
}

/// `30min`, `2h`, `3d`.
fn compact_duration(word: &str) -> Option<(i64, Unit)> {
    static COMPACT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)([a-z]+)$").unwrap());
    let c = COMPACT.captures(word)?;
    Some((c[1].parse().ok()?, duration_unit(&c[2])?))
}

/// Relative reminders are measured in minutes; months and years are too vague to count back from.
fn minutes(amount: i64, unit: Unit) -> Option<i64> {
    match unit {
        Unit::Minute => Some(amount),
        Unit::Hour => amount.checked_mul(60),
        Unit::Day => amount.checked_mul(60 * 24),
        Unit::Week => amount.checked_mul(60 * 24 * 7),
        Unit::Month | Unit::Year => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};

    /// Wednesday 14 January 2026, 10:30 in London.
    fn parser() -> DateParser {
        let now = date(2026, 1, 14)
            .at(10, 30, 0, 0)
            .in_tz("Europe/London")
            .unwrap();
        DateParser::new(now).with_day_first(true)
    }

    fn due(text: &str) -> Option<Due> {
        parser().due(text)
    }

    fn day(y: i16, m: i8, d: i8) -> Option<Due> {
        Some(Due::day(date(y, m, d)))
    }

    fn at(y: i16, m: i8, d: i8, h: i8, min: i8) -> Option<Due> {
        Some(Due::at(date(y, m, d), time(h, min, 0, 0)))
    }

    #[test]
    fn relative_days() {
        assert_eq!(due("today"), day(2026, 1, 14));
        assert_eq!(due("tomorrow"), day(2026, 1, 15));
        assert_eq!(due("tmrw"), day(2026, 1, 15));
        assert_eq!(due("tonight"), at(2026, 1, 14, 19, 0));
        assert_eq!(due("in 3 days"), day(2026, 1, 17));
        assert_eq!(due("in a week"), day(2026, 1, 21));
        assert_eq!(due("in 2 weeks"), day(2026, 1, 28));
        assert_eq!(due("in 2 hours"), at(2026, 1, 14, 12, 30));
        assert_eq!(due("in 3d"), day(2026, 1, 17));
        assert_eq!(due("in a month"), day(2026, 2, 14));
    }

    #[test]
    fn weekdays() {
        assert_eq!(due("friday"), day(2026, 1, 16));
        assert_eq!(due("fri"), day(2026, 1, 16));
        assert_eq!(
            due("wednesday"),
            day(2026, 1, 21),
            "a bare weekday is never today"
        );
        assert_eq!(due("this wednesday"), day(2026, 1, 14));
        assert_eq!(due("this friday"), day(2026, 1, 16));
        assert_eq!(due("next friday"), day(2026, 1, 23));
        assert_eq!(due("next monday"), day(2026, 1, 19));
        assert_eq!(due("next sunday"), day(2026, 1, 25));
        assert_eq!(due("next week"), day(2026, 1, 19));
        assert_eq!(due("weekend"), day(2026, 1, 17));
        assert_eq!(due("next weekend"), day(2026, 1, 24));
        assert_eq!(due("sat"), None, "too ordinary a word");
    }

    #[test]
    fn months() {
        assert_eq!(due("next month"), day(2026, 2, 1));
        assert_eq!(due("end of month"), day(2026, 1, 31));
        assert_eq!(due("end of the month"), day(2026, 1, 31));
        assert_eq!(due("eom"), day(2026, 1, 31));
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(due("jan 27"), day(2026, 1, 27));
        assert_eq!(
            due("jan 2"),
            day(2027, 1, 2),
            "past dates roll to next year"
        );
        assert_eq!(due("27th january 2027"), day(2027, 1, 27));
        assert_eq!(due("1st march"), day(2026, 3, 1));
        assert_eq!(due("2027-01-27"), day(2027, 1, 27));
        assert_eq!(due("feb 30"), None);
    }

    #[test]
    fn numeric_dates_follow_the_region() {
        assert_eq!(due("27/1"), day(2026, 1, 27));
        assert_eq!(due("3/4"), day(2026, 4, 3));
        assert_eq!(parser().with_day_first(false).due("3/4"), day(2026, 3, 4));
        assert_eq!(
            parser().with_day_first(false).due("27/1"),
            day(2026, 1, 27),
            "only one reading is valid"
        );
        assert_eq!(due("27/01/27"), day(2027, 1, 27));
    }

    #[test]
    fn times() {
        assert_eq!(due("5pm"), at(2026, 1, 14, 17, 0));
        assert_eq!(
            due("9am"),
            at(2026, 1, 15, 9, 0),
            "a time already gone means tomorrow"
        );
        assert_eq!(due("5:30pm"), at(2026, 1, 14, 17, 30));
        assert_eq!(due("17:00"), at(2026, 1, 14, 17, 0));
        assert_eq!(due("at 17"), at(2026, 1, 14, 17, 0));
        assert_eq!(due("5 pm"), at(2026, 1, 14, 17, 0));
        assert_eq!(due("noon"), at(2026, 1, 14, 12, 0));
        assert_eq!(due("17"), None, "a bare number isn't a time");
        assert_eq!(due("13pm"), None);
    }

    #[test]
    fn dates_with_times() {
        assert_eq!(due("tomorrow 5pm"), at(2026, 1, 15, 17, 0));
        assert_eq!(due("tomorrow at 17:00"), at(2026, 1, 15, 17, 0));
        assert_eq!(due("5pm tomorrow"), at(2026, 1, 15, 17, 0));
        assert_eq!(due("5pm on friday"), at(2026, 1, 16, 17, 0));
        assert_eq!(due("on friday at noon"), at(2026, 1, 16, 12, 0));
        assert_eq!(due("jan 27 9:15am"), at(2026, 1, 27, 9, 15));
    }

    #[test]
    fn phrases_are_found_inside_text() {
        let text = "Buy 2 apples tomorrow at 5pm, then call";
        let phrases = parser().phrases(text);
        assert_eq!(phrases.len(), 1);
        assert_eq!(&text[phrases[0].range.clone()], "tomorrow at 5pm,");
    }

    #[test]
    fn reminders() {
        let p = parser();
        let due = at(2026, 1, 16, 17, 0);
        assert_eq!(
            p.reminder("30min", due),
            Some(Reminder::Before { minutes: 30 })
        );
        assert_eq!(
            p.reminder("!1 hour before", due),
            Some(Reminder::Before { minutes: 60 })
        );
        assert_eq!(
            p.reminder("1d", due),
            Some(Reminder::Before { minutes: 1440 })
        );
        let london = p.current().time_zone().clone();
        assert_eq!(
            p.reminder("3pm", due),
            Some(Reminder::At(local_moment(
                date(2026, 1, 16),
                time(15, 0, 0, 0),
                &london
            ))),
            "a bare time falls on the due day"
        );
        assert_eq!(
            p.reminder("tomorrow 9am", due),
            Some(Reminder::At(local_moment(
                date(2026, 1, 15),
                time(9, 0, 0, 0),
                &london
            )))
        );
        assert_eq!(p.reminder("2 months", due), None);
        let phrases = p.phrases("Call !30min before and !tomorrow 9am");
        assert_eq!(phrases.len(), 2);
        assert!(matches!(
            phrases[0].value,
            PhraseValue::Reminder(ReminderFragment::Before { minutes: 30 })
        ));
    }
}
