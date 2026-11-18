//! The inline composer for adding and editing tasks: the quick-add name field, a description, and chips for
//! the date, reminders and project.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use ostracon_core::quickadd::{BlockValue, Draft, QuickAddResult};
use ostracon_core::{Due, Project, ProjectId, Reminder};

use super::pickers::{DatePicker, PickerEvent, ReminderEditor};
use super::quick_add::{QuickAddEvent, QuickAddInput};
use super::theme;
use crate::format;
use crate::state::Composer as Mode;

pub enum ComposerEvent {
    Submit,
    Cancel,
}

impl EventEmitter<ComposerEvent> for ComposerView {}

enum Picker {
    Closed,
    Date(Entity<DatePicker>),
    Reminders(Entity<ReminderEditor>),
}

pub struct ComposerView {
    pub mode: Mode,
    name: Entity<QuickAddInput>,
    description: Entity<InputState>,
    projects: Vec<Project>,
    /// Chosen with the chips; text in the name overrides the project and date.
    project: ProjectId,
    due: Option<Due>,
    reminders: Vec<Reminder>,
    /// What the name says, refreshed as it changes.
    parsed: QuickAddResult,
    picker: Picker,
    _subscriptions: Vec<Subscription>,
}

/// What the composer starts with.
pub struct ComposerSeed {
    pub mode: Mode,
    pub draft: Draft,
    pub description: String,
    pub project: ProjectId,
    pub due: Option<Due>,
    pub reminders: Vec<Reminder>,
}

impl ComposerView {
    pub fn new(
        seed: ComposerSeed,
        projects: Vec<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match seed.mode {
            Mode::Editing { .. } => "Task name",
            _ => "Task name, e.g. Pay rent tomorrow 9am #Home !1h",
        };
        let name = cx.new(|cx| QuickAddInput::new(seed.draft, projects.clone(), placeholder, cx));
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Description")
                .default_value(seed.description)
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, event, _, cx| match event {
                QuickAddEvent::Changed => this.reparse(cx),
                QuickAddEvent::Submit => cx.emit(ComposerEvent::Submit),
                QuickAddEvent::Cancel => this.cancel(cx),
            }),
            cx.subscribe_in(&description, window, |_, _, event, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    cx.emit(ComposerEvent::Submit);
                }
            }),
        ];
        let mut view = ComposerView {
            mode: seed.mode,
            name,
            description,
            projects,
            project: seed.project,
            due: seed.due,
            reminders: seed.reminders,
            parsed: QuickAddResult {
                name: String::new(),
                project_id: None,
                due: None,
                reminders: vec![],
                due_text: None,
            },
            picker: Picker::Closed,
            _subscriptions: subscriptions,
        };
        view.reparse(cx);
        view
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.name.focus_handle(cx), cx);
    }

    pub fn draft(&self, cx: &App) -> Draft {
        self.name.read(cx).draft().clone()
    }

    pub fn description(&self, cx: &App) -> String {
        self.description.read(cx).value().to_string()
    }

    pub fn project(&self) -> ProjectId {
        self.project
    }

    pub fn due(&self) -> Option<Due> {
        self.due
    }

    pub fn reminders(&self) -> &[Reminder] {
        &self.reminders
    }

    pub fn set_projects(&mut self, projects: Vec<Project>, cx: &mut Context<Self>) {
        self.projects = projects.clone();
        self.name.update(cx, |name, _| name.set_projects(projects));
        self.reparse(cx);
    }

    /// After adding: ready for the next task in the same project, with nothing else carried over.
    pub fn reset_for_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |name, cx| name.clear(cx));
        self.description
            .update(cx, |d, cx| d.set_value("", window, cx));
        self.due = None;
        self.reminders.clear();
        self.picker = Picker::Closed;
        self.focus(window, cx);
        cx.notify();
    }

    fn reparse(&mut self, cx: &mut Context<Self>) {
        let dates = ostracon_core::dates::DateParser::now();
        let mut draft = self.name.read(cx).draft().clone();
        draft.convert(&self.projects, Some(&dates), true);
        self.parsed = draft.result(&dates);
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.picker, Picker::Closed) {
            self.picker = Picker::Closed;
            cx.notify();
        } else {
            cx.emit(ComposerEvent::Cancel);
        }
    }

    fn effective_due(&self) -> Option<Due> {
        self.parsed.due.or(self.due)
    }

    fn effective_project(&self) -> ProjectId {
        self.parsed.project_id.unwrap_or(self.project)
    }

    fn open_date_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.picker, Picker::Date(_)) {
            self.picker = Picker::Closed;
            cx.notify();
            return;
        }
        let picker = cx.new(|cx| DatePicker::new(self.effective_due(), window, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &PickerEvent<Option<Due>>, window, cx| {
                let PickerEvent::Chosen(due) = event;
                this.due = *due;
                // A date picked with the chip replaces any written in the name.
                this.name.update(cx, |name, cx| {
                    name.update_draft(|d| d.remove_blocks(|v| matches!(v, BlockValue::Due(_))), cx)
                });
                this.picker = Picker::Closed;
                this.focus(window, cx);
                cx.notify();
            },
        );
        self._subscriptions.push(subscription);
        picker.update(cx, |picker, cx| picker.focus(window, cx));
        self.picker = Picker::Date(picker);
        cx.notify();
    }

    fn open_reminders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.picker, Picker::Reminders(_)) {
            self.picker = Picker::Closed;
            cx.notify();
            return;
        }
        let editor = cx.new(|cx| {
            ReminderEditor::new(self.reminders.clone(), self.effective_due(), window, cx)
        });
        let subscription = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &PickerEvent<Vec<Reminder>>, _, cx| {
                let PickerEvent::Chosen(reminders) = event;
                this.reminders = reminders.clone();
                cx.notify();
            },
        );
        self._subscriptions.push(subscription);
        editor.update(cx, |editor, cx| editor.focus(window, cx));
        self.picker = Picker::Reminders(editor);
        cx.notify();
    }

    fn choose_project(&mut self, id: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        self.project = id;
        // The chip's choice replaces a #project written in the name.
        self.name.update(cx, |name, cx| {
            name.update_draft(
                |d| d.remove_blocks(|v| matches!(v, BlockValue::Project(_))),
                cx,
            )
        });
        self.focus(window, cx);
        cx.notify();
    }
}

fn chip(id: impl Into<ElementId>, palette: &theme::Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .h(px(26.))
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(palette.card_border)
        .text_size(px(12.5))
        .text_color(palette.muted)
        .cursor_pointer()
        .hover(|d| d.bg(palette.sidebar_hover))
}

impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let now = ZonedDateTime::now();
        let due = self.effective_due();
        let project_id = self.effective_project();
        let project = self.projects.iter().find(|p| p.id == project_id).cloned();
        let reminder_count = self.reminders.len() + self.parsed.reminders.len();
        let editing = matches!(self.mode, Mode::Editing { .. });
        let can_save = !self.parsed.name.is_empty();

        let date_chip = {
            let (label, color) = match due {
                Some(due) => (
                    format::due_label(due, &now),
                    palette.urgency(format::urgency(due, &now)),
                ),
                None => ("Date".to_owned(), palette.muted),
            };
            let written = self.parsed.due.is_some();
            div().relative().child(
                chip("date-chip", &palette)
                    .text_color(color)
                    .when(written, |d| d.bg(palette.block_due))
                    .child(gpui_kit::component::Icon::new(IconName::Calendar).xsmall())
                    .child(label)
                    .on_click(cx.listener(|this, _, window, cx| this.open_date_picker(window, cx))),
            )
        };
        let reminder_chip = chip("reminder-chip", &palette)
            .child(gpui_kit::component::Icon::new(IconName::Bell).xsmall())
            .child(if reminder_count == 0 {
                "Reminders".to_owned()
            } else {
                format!("{reminder_count}")
            })
            .on_click(cx.listener(|this, _, window, cx| this.open_reminders(window, cx)));

        let entity = cx.entity();
        let projects = self.projects.clone();
        let project_button = Button::new("project-chip")
            .ghost()
            .small()
            .label(
                project
                    .as_ref()
                    .map_or("Inbox".to_owned(), |p| p.name.clone()),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for project in &projects {
                    let id = project.id;
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(project.name.clone()).on_click(
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| this.choose_project(id, window, cx));
                        },
                    ));
                }
                menu
            });

        let picker: Option<AnyElement> = match &self.picker {
            Picker::Closed => None,
            Picker::Date(picker) => Some(picker.clone().into_any_element()),
            Picker::Reminders(editor) => Some(editor.clone().into_any_element()),
        };
        let suggestions = self
            .name
            .update(cx, |name, cx| name.render_suggestions(&palette, cx));

        div()
            .flex()
            .flex_col()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(palette.card_border)
            .bg(palette.card)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .pt_2()
                    .child(self.name.clone())
                    .when_some(suggestions, |d, s| {
                        d.child(deferred(anchored().child(s)).with_priority(2))
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .child(Input::new(&self.description).appearance(false).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .py_2()
                            .child(date_chip)
                            .child(reminder_chip),
                    )
                    .when_some(picker, |d, picker| {
                        d.child(
                            deferred(
                                anchored().snap_to_window().child(
                                    div()
                                        .occlude()
                                        .p_2()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(palette.card_border)
                                        .bg(palette.card)
                                        .shadow_lg()
                                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                            this.picker = Picker::Closed;
                                            cx.notify();
                                        }))
                                        .child(picker),
                                ),
                            )
                            .with_priority(1),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1p5()
                    .border_t_1()
                    .border_color(palette.hairline)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(9.)).rounded_full().bg(
                                project.map_or(palette.accent, |p| theme::project_color(p.color)),
                            ))
                            .child(project_button),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(Button::new("cancel").small().label("Cancel").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Cancel)),
                            ))
                            .child(
                                Button::new("save")
                                    .small()
                                    .primary()
                                    .label(if editing { "Save" } else { "Add task" })
                                    .disabled(!can_save)
                                    .on_click(
                                        cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Submit)),
                                    ),
                            ),
                    ),
            )
    }
}
