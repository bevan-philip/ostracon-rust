//! The date and reminder pickers the composer's chips and the overview open.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::calendar::{Calendar, CalendarEvent, CalendarState, Date as CalendarDate};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use jiff::civil::{Date, Time, Weekday};
use ostracon_core::dates::DateParser;
use ostracon_core::{Due, Reminder};

use super::theme;
use crate::format;

pub enum PickerEvent<T> {
    Chosen(T),
}

/// Picks a due date: typed in words, chosen from quick options or the calendar, with an optional time.
pub struct DatePicker {
    due: Option<Due>,
    text: Entity<InputState>,
    time: Entity<InputState>,
    calendar: Entity<CalendarState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PickerEvent<Option<Due>>> for DatePicker {}

impl DatePicker {
    pub fn new(due: Option<Due>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a date, e.g. next fri 5pm"));
        let time_text = due
            .and_then(|d| d.time)
            .map(|t| t.strftime("%H:%M").to_string())
            .unwrap_or_default();
        let time = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Time, e.g. 5pm")
                .default_value(time_text)
        });
        let calendar = cx.new(|cx| {
            let mut state = CalendarState::new(window, cx);
            if let Some(date) = due.and_then(|d| to_chrono(d.date)) {
                state.set_date(date, window, cx);
            }
            state
        });
        let subscriptions = vec![
            cx.subscribe_in(&text, window, |this, _, event, _, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    if let Some(due) = this.typed(cx) {
                        this.choose(Some(due), cx);
                    }
                }
                _ => {}
            }),
            cx.subscribe_in(&time, window, |this, _, event, _, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let date = this
                        .due
                        .map_or_else(|| ZonedDateTime::now().date(), |d| d.date);
                    let time = this.time_of_day(cx);
                    this.choose(Some(Due { date, time }), cx);
                }
                _ => {}
            }),
            cx.subscribe_in(
                &calendar,
                window,
                |this, _, event: &CalendarEvent, _, cx| {
                    let CalendarEvent::Selected(CalendarDate::Single(Some(date))) = event else {
                        return;
                    };
                    if let Some(date) = from_chrono(*date) {
                        let time = this.time_of_day(cx);
                        this.choose(Some(Due { date, time }), cx);
                    }
                },
            ),
        ];
        DatePicker {
            due,
            text,
            time,
            calendar,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.text.update(cx, |input, cx| input.focus(window, cx));
    }

    /// What the "type a date" field says, if it reads as one.
    fn typed(&self, cx: &App) -> Option<Due> {
        let text = self.text.read(cx).value();
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Due::from_storage(text).or_else(|| DateParser::now().due(text))
    }

    fn time_of_day(&self, cx: &App) -> Option<Time> {
        let text = self.time.read(cx).value();
        DateParser::now()
            .date_fragment(text.trim())
            .and_then(|f| f.time)
    }

    fn choose(&mut self, due: Option<Due>, cx: &mut Context<Self>) {
        self.due = due;
        cx.emit(PickerEvent::Chosen(due));
    }

    /// A quick option, keeping the time already chosen.
    fn quick(&self, label: &'static str, date: Date, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let time = self.due.and_then(|d| d.time);
        let weekday = date.strftime("%a").to_string();
        div()
            .id(label)
            .flex()
            .justify_between()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|d| d.bg(palette.sidebar_hover))
            .text_size(px(13.5))
            .child(div().text_color(palette.text).child(label))
            .child(div().text_color(palette.faint).child(weekday))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(Some(Due { date, time }), cx)))
    }
}

impl Render for DatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let now = ZonedDateTime::now();
        let today = now.date();
        let preview = {
            let typed = self.text.read(cx).value();
            (!typed.trim().is_empty()).then(|| match self.typed(cx) {
                Some(due) => format!("{} · press Return", format::due_label(due, &now)),
                None => "Not a date I understand".to_owned(),
            })
        };
        let next_week = next_weekday(today, Weekday::Monday);
        let weekend = if today.weekday() == Weekday::Sunday {
            today
        } else {
            next_weekday_inclusive(today, Weekday::Saturday)
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(300.))
            .child(Input::new(&self.text).small())
            .when_some(preview, |d, preview| {
                d.child(
                    div()
                        .px_1()
                        .text_size(px(12.))
                        .text_color(palette.muted)
                        .child(preview),
                )
            })
            .child(div().h(px(1.)).my_1().bg(palette.hairline))
            .child(self.quick("Today", today, cx))
            .child(self.quick("Tomorrow", today.tomorrow().unwrap_or(today), cx))
            .child(self.quick("This weekend", weekend, cx))
            .child(self.quick("Next week", next_week, cx))
            .child(
                div()
                    .id("no-date")
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|d| d.bg(palette.sidebar_hover))
                    .text_size(px(13.5))
                    .text_color(palette.text)
                    .child("No date")
                    .on_click(cx.listener(|this, _, _, cx| this.choose(None, cx))),
            )
            .child(div().h(px(1.)).my_1().bg(palette.hairline))
            // Weeks start on Monday, as Todoist's default and "next week" have it.
            .child(
                Calendar::new(&self.calendar)
                    .small()
                    .first_day_of_week(chrono::Weekday::Mon),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.time).small()))
                    .child(
                        Button::new("set-time")
                            .small()
                            .primary()
                            .label("Set time")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let date = this
                                    .due
                                    .map_or_else(|| ZonedDateTime::now().date(), |d| d.date);
                                let time = this.time_of_day(cx);
                                this.choose(Some(Due { date, time }), cx);
                            })),
                    ),
            )
    }
}

fn next_weekday(from: Date, weekday: Weekday) -> Date {
    let days = (i64::from(weekday.to_monday_zero_offset())
        - i64::from(from.weekday().to_monday_zero_offset()))
    .rem_euclid(7);
    let days = if days == 0 { 7 } else { days };
    from.checked_add(jiff::Span::new().days(days))
        .unwrap_or(from)
}

fn next_weekday_inclusive(from: Date, weekday: Weekday) -> Date {
    if from.weekday() == weekday {
        from
    } else {
        next_weekday(from, weekday)
    }
}

/// Edits a task's reminders: each listed with a button to remove it, and a field to add more.
pub struct ReminderEditor {
    reminders: Vec<Reminder>,
    due: Option<Due>,
    input: Entity<InputState>,
    error: Option<String>,
    _subscription: Subscription,
}

impl EventEmitter<PickerEvent<Vec<Reminder>>> for ReminderEditor {}

impl ReminderEditor {
    pub fn new(
        reminders: Vec<Reminder>,
        due: Option<Due>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("e.g. 30min, 1 day before, tomorrow 9am")
        });
        let subscription = cx.subscribe_in(&input, window, |this, input, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = input.read(cx).value();
                if text.trim().is_empty() {
                    return;
                }
                match DateParser::now().reminder(&text, this.due) {
                    Some(reminder) => {
                        this.reminders.push(reminder);
                        Reminder::sort(&mut this.reminders);
                        this.error = None;
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                        cx.emit(PickerEvent::Chosen(this.reminders.clone()));
                    }
                    None => {
                        this.error =
                            Some(format!("Couldn't read \"{}\" as a reminder", text.trim()))
                    }
                }
                cx.notify();
            }
        });
        ReminderEditor {
            reminders,
            due,
            input,
            error: None,
            _subscription: subscription,
        }
    }

    pub fn set_due(&mut self, due: Option<Due>) {
        self.due = due;
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }
}

impl Render for ReminderEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let now = ZonedDateTime::now();
        let rows = self
            .reminders
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, reminder)| {
                let inert = format::reminder_is_inert(reminder, self.due);
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .text_size(px(13.5))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_color(palette.text)
                                    .child(format::reminder_label(reminder, &now)),
                            )
                            .when(inert, |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(palette.faint)
                                        .child("Needs a due time to go off"),
                                )
                            }),
                    )
                    .child(
                        Button::new(("remove-reminder", index))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.reminders.remove(index);
                                cx.emit(PickerEvent::Chosen(this.reminders.clone()));
                                cx.notify();
                            })),
                    )
            });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(280.))
            .child(
                div()
                    .px_1()
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child("Reminders"),
            )
            .when(self.reminders.is_empty(), |d| {
                d.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(px(13.))
                        .text_color(palette.faint)
                        .child("No reminders yet"),
                )
            })
            .children(rows)
            .child(Input::new(&self.input).small())
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .px_1()
                        .text_size(px(12.))
                        .text_color(palette.overdue)
                        .child(error),
                )
            })
    }
}

/// A date in chrono, for gpui-kit's calendar. Everything else in Ostracon uses jiff.
fn to_chrono(date: Date) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(date.year().into(), date.month() as u32, date.day() as u32)
}

fn from_chrono(date: chrono::NaiveDate) -> Option<Date> {
    use chrono::Datelike as _;
    Date::new(date.year() as i16, date.month() as i8, date.day() as i8).ok()
}
