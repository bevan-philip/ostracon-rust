//! The inline composer for adding and editing tasks: the quick-add name field, a description, and one row of
//! chips for the date, reminders and project, with cancel and save beside them, as in Todoist.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use ostracon_core::quickadd::{BlockValue, Draft, QuickAddResult};
use ostracon_core::{Due, Project, ProjectId, Reminder};

use super::overlays::DESCRIPTION_MAX_ROWS;
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
    description: Entity<TextareaState>,
    projects: Vec<Project>,
    /// Chosen with the chips; text in the name overrides the project and date.
    project: ProjectId,
    due: Option<Due>,
    /// The due date it opened with, which it goes back to for the next task.
    seed_due: Option<Due>,
    reminders: Vec<Reminder>,
    /// What the name says, refreshed as it changes.
    parsed: QuickAddResult,
    picker: Picker,
    /// What it held when opened or last reset, to tell whether anything would be lost by closing it.
    baseline: Baseline,
    _subscriptions: Vec<Subscription>,
}

#[derive(PartialEq)]
struct Baseline {
    draft: Draft,
    description: String,
    project: ProjectId,
    due: Option<Due>,
    reminders: Vec<Reminder>,
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
        let baseline = Baseline {
            draft: seed.draft.clone(),
            description: seed.description.clone(),
            project: seed.project,
            due: seed.due,
            reminders: seed.reminders.clone(),
        };
        let name = cx.new(|cx| QuickAddInput::new(seed.draft, projects.clone(), placeholder, cx));
        let description = cx.new(|cx| {
            // Enter saves, as from the name; Shift-Enter starts a new line.
            TextareaState::new(window, cx)
                .auto_grow(1, DESCRIPTION_MAX_ROWS)
                .submit_on_enter(true)
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
                if let InputEvent::PressEnter { shift: false, .. } = event {
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
            seed_due: seed.due,
            reminders: seed.reminders,
            parsed: QuickAddResult {
                name: String::new(),
                project_id: None,
                due: None,
                reminders: vec![],
                due_text: None,
            },
            picker: Picker::Closed,
            baseline,
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

    /// Whether closing it now would lose anything typed or picked.
    pub fn has_changes(&self, cx: &App) -> bool {
        self.current(cx) != self.baseline
    }

    fn current(&self, cx: &App) -> Baseline {
        Baseline {
            draft: self.draft(cx),
            description: self.description(cx),
            project: self.project,
            due: self.due,
            reminders: self.reminders.clone(),
        }
    }

    pub fn set_projects(&mut self, projects: Vec<Project>, cx: &mut Context<Self>) {
        self.projects = projects.clone();
        self.name.update(cx, |name, _| name.set_projects(projects));
        self.reparse(cx);
    }

    /// After adding: ready for the next task in the same project, due as the first was to begin with, with
    /// nothing else carried over.
    pub fn reset_for_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |name, cx| name.clear(cx));
        self.description
            .update(cx, |d, cx| d.set_value("", window, cx));
        self.due = self.seed_due;
        self.reminders.clear();
        self.picker = Picker::Closed;
        self.baseline = self.current(cx);
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

const CHIP_HEIGHT: f32 = 28.;

/// A chip's content: an icon and a label, the same whether the chip is a plain element or a button.
fn chip_content(icon: Icon, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(icon.size(px(14.)))
        .child(label.into())
}

fn chip(id: impl Into<ElementId>, palette: &theme::Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(palette.card_border)
        .text_size(px(13.))
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
            chip("date-chip", &palette)
                .text_color(color)
                .when(self.parsed.due.is_some(), |d| d.bg(palette.block_due))
                .child(chip_content(Icon::new(Lucide::Calendar), label))
                .on_click(cx.listener(|this, _, window, cx| this.open_date_picker(window, cx)))
        };
        let reminder_chip = chip("reminder-chip", &palette)
            .child(chip_content(
                Icon::new(Lucide::AlarmClock),
                match reminder_count {
                    0 => "Reminders".to_owned(),
                    1 => "1 reminder".to_owned(),
                    n => format!("{n} reminders"),
                },
            ))
            .on_click(cx.listener(|this, _, window, cx| this.open_reminders(window, cx)));

        let entity = cx.entity();
        let projects = self.projects.clone();
        let project_icon = match &project {
            Some(p) if !p.is_inbox => {
                Icon::new(Lucide::Hash).text_color(theme::project_color(p.color))
            }
            _ => Icon::new(Lucide::Inbox),
        };
        let project_chip = Button::new("project-chip")
            .ghost()
            .small()
            .h(px(CHIP_HEIGHT))
            .px_2()
            .border_1()
            .border_color(palette.card_border)
            .text_size(px(13.))
            .text_color(palette.muted)
            .when(self.parsed.project_id.is_some(), |b| {
                b.bg(palette.block_project)
            })
            .child(chip_content(
                project_icon,
                project
                    .as_ref()
                    .map_or("Inbox".to_owned(), |p| p.name.clone()),
            ))
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

        let cancel = Button::new("cancel")
            .ghost()
            .icon(Icon::new(Lucide::X))
            .size(px(CHIP_HEIGHT + 4.))
            .tooltip("Cancel")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Cancel)));
        let save = Button::new("save")
            .primary()
            .icon(Icon::new(Lucide::Check))
            .size(px(CHIP_HEIGHT + 4.))
            .tooltip(if editing { "Save" } else { "Add task" })
            .disabled(!can_save)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerEvent::Submit)));

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
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(palette.card_border)
            .bg(palette.card)
            .child(self.name.clone())
            // Popovers are absolutely positioned, so each hangs from a zero-height box in the flow below what it
            // belongs to; otherwise they'd land at the top of the card.
            .when_some(suggestions, |d, s| {
                d.child(
                    div()
                        .h_0()
                        .child(deferred(anchored().child(s)).with_priority(2)),
                )
            })
            // No padding of its own, so the description starts where the name does.
            .child(
                Textarea::new(&self.description)
                    .appearance(false)
                    .small()
                    .px_0()
                    .text_size(px(13.)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .mt_2()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1p5()
                            .child(date_chip)
                            .child(reminder_chip)
                            .child(project_chip),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .child(cancel)
                            .child(save),
                    ),
            )
            .when_some(picker, |d, picker| {
                d.child(
                    div().h_0().child(
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
                    ),
                )
            })
    }
}
