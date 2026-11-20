//! Property tests of the date grammar: dates and times written out in every supported form read back as
//! themselves, relative phrases land where they say, and nothing panics on arbitrary text.

use jiff::ZonedDateTime;
use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use ostracon_core::Due;
use ostracon_core::dates::{DateFragment, DateParser, PhraseValue};
use proptest::prelude::*;

const ZONES: &[&str] = &[
    "UTC",
    "Europe/London",
    "America/New_York",
    "Australia/Lord_Howe",
    "Pacific/Apia",
    "Asia/Kolkata",
    "America/Sao_Paulo",
];

const MONTHS: [&str; 12] = [
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

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("monday", Weekday::Monday),
    ("tuesday", Weekday::Tuesday),
    ("wednesday", Weekday::Wednesday),
    ("thursday", Weekday::Thursday),
    ("friday", Weekday::Friday),
    ("saturday", Weekday::Saturday),
    ("sunday", Weekday::Sunday),
];

fn date_in(years: std::ops::RangeInclusive<i16>) -> impl Strategy<Value = Date> {
    (years, 1i8..=12, 1i8..=31).prop_filter_map("a real date", |(y, m, d)| Date::new(y, m, d).ok())
}

fn time() -> impl Strategy<Value = Time> {
    (0i8..24, 0i8..60).prop_map(|(h, m)| Time::new(h, m, 0, 0).unwrap())
}

fn now() -> impl Strategy<Value = ZonedDateTime> {
    (
        date_in(1971..=2098),
        0i8..24,
        0i8..60,
        proptest::sample::select(ZONES),
    )
        .prop_map(|(date, h, m, zone)| {
            date.at(h, m, 0, 0)
                .to_zoned_datetime(TimeZone::get(zone).unwrap())
                .unwrap()
        })
}

fn parser() -> impl Strategy<Value = DateParser> {
    (now(), any::<bool>())
        .prop_map(|(now, day_first)| DateParser::new(now).with_day_first(day_first))
}

fn ordinal(day: i8) -> String {
    let suffix = match (day % 10, day) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// The first day on or after `from` with this month and day, found by walking forward.
fn next_occurrence(from: Date, month: i8, day: i8) -> Option<Date> {
    let mut date = from;
    for _ in 0..(366 * 9) {
        if date.month() == month && date.day() == day {
            return Some(date);
        }
        date = date.tomorrow().ok()?;
    }
    None
}

/// A time as people write it with am/pm.
fn twelve_hour(time: Time) -> (i8, &'static str) {
    let hour = match time.hour() % 12 {
        0 => 12,
        h => h,
    };
    (hour, if time.hour() < 12 { "am" } else { "pm" })
}

/// `PROPTEST_CASES` when set, for longer searches, or `default`.
fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(1024),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn due_dates_round_trip_through_storage(date in date_in(1..=9999), time in proptest::option::of(time())) {
        let due = Due { date, time };
        prop_assert_eq!(Due::from_storage(&due.storage_value()), Some(due));
    }

    #[test]
    fn dates_with_a_year_read_back(parser in parser(), date in date_in(1000..=9999)) {
        let (y, m, d) = (date.year(), date.month(), date.day());
        let name = MONTHS[(m - 1) as usize];
        let short = &name[..3];
        let mut forms = vec![
            format!("{name} {d} {y}"),
            format!("{short} {d} {y}"),
            format!("{} {name} {y}", ordinal(d)),
            format!("{d} {short} {y}"),
            format!("{y}-{m:02}-{d:02}"),
            format!("{y}-{m}-{d}"),
        ];
        let probe = parser.clone().with_day_first(true);
        forms.push(format!("{d}/{m}/{y}"));
        for form in &forms[..forms.len() - 1] {
            prop_assert_eq!(parser.due(form), Some(Due::day(date)), "{}", form);
        }
        prop_assert_eq!(probe.due(&format!("{d}/{m}/{y}")), Some(Due::day(date)));
        prop_assert_eq!(parser.clone().with_day_first(false).due(&format!("{m}/{d}/{y}")), Some(Due::day(date)));
    }

    #[test]
    fn dates_without_a_year_are_the_next_time_that_day_comes_round(
        parser in parser(),
        (month, day) in prop_oneof![
            (1i8..=12, 1i8..=31),
            // Month ends, leap days included, where the calendar is most irregular.
            (1i8..=12, 28i8..=31),
            Just((2i8, 29i8)),
        ]
        // Only real days of the month (in a leap year); impossible ones such as 31 April are covered below.
        .prop_map(|(month, day)| (month, day.min(Date::new(2000, month, 1).unwrap().days_in_month()))),
    ) {
        let today = parser.current().date();
        let expected = next_occurrence(today, month, day).map(Due::day);
        let name = MONTHS[(month - 1) as usize];
        prop_assert_eq!(parser.due(&format!("{name} {day}")), expected, "{} {}", name, day);
        prop_assert_eq!(parser.due(&format!("{} {name}", ordinal(day))), expected);
        prop_assert_eq!(parser.clone().with_day_first(true).due(&format!("{day}/{month}")), expected);
        prop_assert_eq!(parser.clone().with_day_first(false).due(&format!("{month}/{day}")), expected);
    }

    #[test]
    fn impossible_days_are_not_dates(
        parser in parser(),
        (month, day) in proptest::sample::select(vec![(2i8, 30i8), (2, 31), (4, 31), (6, 31), (9, 31), (11, 31)]),
    ) {
        let name = MONTHS[(month - 1) as usize];
        prop_assert_eq!(parser.due(&format!("{name} {day}")), None);
    }

    #[test]
    fn times_read_back(parser in parser(), time in time(), day_word in proptest::sample::select(vec!["", "today", "tomorrow"])) {
        let (h, m) = (time.hour(), time.minute());
        let (h12, meridiem) = twelve_hour(time);
        let mut forms = vec![format!("{h}:{m:02}"), format!("{h12}:{m:02}{meridiem}"), format!("{h12}:{m:02} {meridiem}")];
        if m == 0 {
            forms.extend([format!("at {h}"), format!("{h12}{meridiem}"), format!("at {h12} {meridiem}")]);
        }
        let today = parser.current().date();
        for form in forms {
            let (text, expected_day) = match day_word {
                "" => (form.clone(), None),
                "today" => (format!("today {form}"), Some(today)),
                _ => (format!("{form} tomorrow"), today.tomorrow().ok()),
            };
            let fragment = parser.date_fragment(&text);
            prop_assert_eq!(fragment, Some(DateFragment { day: expected_day, time: Some(time) }), "{}", text);
        }
    }

    /// A bare time is the next time the clock reads it: later than now, and no more than a day (and a DST
    /// shift) away.
    #[test]
    fn a_bare_time_is_the_next_time_the_clock_reads_it(parser in parser(), time in time()) {
        let now = parser.current().clone();
        let due = parser.resolve(DateFragment { day: None, time: Some(time) });
        prop_assert_eq!(due.time, Some(time));
        let today = now.date();
        prop_assert!(due.date == today || Some(due.date) == today.tomorrow().ok(), "{:?} from {}", due, now);
        let moment = due.moment(now.time_zone());
        prop_assert!(moment > now.timestamp() || due.date == today.tomorrow().unwrap(), "{:?} from {}", due, now);
        if due.date == today.tomorrow().unwrap() {
            // Tomorrow only because today's has gone.
            let todays = Due::at(today, time).moment(now.time_zone());
            prop_assert!(todays <= now.timestamp(), "{:?} from {}", due, now);
        }
    }

    #[test]
    fn weekdays_land_in_the_right_week(parser in parser(), which in 0usize..7, prefix in proptest::sample::select(vec!["", "this ", "next ", "on "])) {
        let (name, weekday) = WEEKDAYS[which];
        let today = parser.current().date();
        let due = parser.due(&format!("{prefix}{name}"));
        let due = due.expect("a weekday is a date");
        prop_assert_eq!(due.date.weekday(), weekday);
        let days = today.until((jiff::Unit::Day, due.date)).unwrap().get_days();
        match prefix {
            "this " => prop_assert!((0..7).contains(&days), "{} days", days),
            "next " => {
                // Somewhere in the Monday-to-Sunday week after this one.
                let monday = today.checked_sub(jiff::Span::new().days(i64::from(today.weekday().to_monday_zero_offset()))).unwrap();
                let from_monday = monday.until((jiff::Unit::Day, due.date)).unwrap().get_days();
                prop_assert!((7..14).contains(&from_monday), "{} days from this Monday", from_monday);
            }
            _ => prop_assert!((1..=7).contains(&days), "{} days", days),
        }
    }

    #[test]
    fn in_n_units_counts_forward(parser in parser(), n in 0i64..400, unit in proptest::sample::select(vec!["days", "weeks", "months", "years"])) {
        let today = parser.current().date();
        let span = match unit {
            "days" => jiff::Span::new().days(n),
            "weeks" => jiff::Span::new().weeks(n),
            "months" => jiff::Span::new().months(n),
            _ => jiff::Span::new().years(n),
        };
        let due = parser.due(&format!("in {n} {unit}"));
        let expected = parser.current().checked_add(span).ok().map(|z| Due::day(z.date()));
        prop_assert_eq!(due, expected);
        if let Some(due) = due {
            prop_assert!(due.date >= today);
        }
    }

    #[test]
    fn phrases_never_panic_and_stay_in_bounds(parser in parser(), text in "\\PC{0,40}") {
        let phrases = parser.phrases(&text);
        let mut end = 0;
        for phrase in &phrases {
            prop_assert!(phrase.range.start >= end && phrase.range.start < phrase.range.end);
            prop_assert!(text.is_char_boundary(phrase.range.start) && text.is_char_boundary(phrase.range.end));
            end = phrase.range.end;
            if let PhraseValue::Due(fragment) = phrase.value {
                let _ = parser.resolve(fragment);
            }
        }
        prop_assert!(end <= text.len());
        let _ = parser.due(&text);
        let _ = parser.reminder(&text, None);
    }

    #[test]
    fn date_words_never_panic(
        parser in parser(),
        words in proptest::collection::vec(
            proptest::sample::select(vec![
                "in", "a", "on", "at", "@", "next", "this", "end", "of", "the", "month", "week", "weekend", "today",
                "tomorrow", "tonight", "eom", "friday", "fri", "jan", "feb", "29", "31", "29th", "2027", "9999",
                "0001", "12", "0", "5pm", "12am", "17:30", "24:00", "am", "pm", "noon", "midnight", "before",
                "!30min", "!1", "hour", "!tomorrow", "!3pm", "99999999999", "-3", "days", "29/2", "2/29/2027",
                "2027-02-29", "2028-02-29", "1/13", "13/1", "1h", "3d", "2w", "1y",
            ]),
            0..8,
        ),
    ) {
        let text = words.join(" ");
        for phrase in parser.phrases(&text) {
            match phrase.value {
                PhraseValue::Due(fragment) => { let _ = parser.resolve(fragment); }
                PhraseValue::Reminder(fragment) => { let _ = parser.resolve_reminder(fragment, None); }
            }
        }
        let _ = parser.due(&text);
        let _ = parser.reminder(&text, Some(Due::day(parser.current().date())));
    }
}
