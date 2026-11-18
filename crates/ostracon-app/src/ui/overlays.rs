//! Everything drawn above the list: the task overview, dialogs, settings and the toast.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use ostracon_core::backup::Backup;
use ostracon_core::{ProjectColor, TaskId};

use super::actions::{Confirm, Escape};
use super::theme::{self, Palette, TASK_TEXT};
use super::workspace::{OverviewPicker, Panel, Workspace};
use crate::format;
use crate::state::{Dialog, Undo};

impl Workspace {
    /// The modal layer, if anything is open, topmost first.
    pub fn render_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let palette = theme::palette(cx);
        let (content, context) = match (&self.panel, &self.state.dialog) {
            (Panel::ConfirmRestore(path), _) => (
                self.render_confirm_restore(path.clone(), &palette, cx),
                "Dialog",
            ),
            (Panel::Settings, _) => (self.render_settings(&palette, cx), "Settings"),
            (_, Dialog::ConfirmDeleteTasks(ids)) => (
                self.render_confirm_delete_tasks(ids.clone(), &palette, cx),
                "Dialog",
            ),
            (_, Dialog::ConfirmDeleteProject(id)) => {
                let name = self
                    .state
                    .project_by_id(*id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let count = self
                    .state
                    .tasks()
                    .iter()
                    .filter(|t| t.project_id == *id)
                    .count();
                (
                    self.render_confirm_delete_project(name, count, &palette, cx),
                    "Dialog",
                )
            }
            (_, Dialog::ProjectEditor(_)) => (self.render_project_editor(&palette, cx), "Dialog"),
            (_, Dialog::None) if self.overview.is_some() => {
                (self.render_overview(&palette, window, cx)?, "Overview")
            }
            _ => return None,
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .bg(palette.backdrop)
                .key_context(format!("Modal {context}").as_str())
                .track_focus(&self.modal_focus)
                .occlude()
                // A click on the backdrop closes the top layer, as Esc does. It calls the handler rather than
                // dispatching the action, which would need focus to be inside the modal.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.escape(&Escape, window, cx)),
                )
                .child(content)
                .into_any_element(),
        )
    }

    fn card(palette: &Palette, width: f32) -> Div {
        div()
            .flex()
            .flex_col()
            .w(px(width))
            .max_h(relative(0.85))
            .rounded_xl()
            .border_1()
            .border_color(palette.card_border)
            .bg(palette.card)
            .shadow_2xl()
            .text_color(palette.text)
            // Clicks inside the card don't reach the backdrop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn render_overview(
        &mut self,
        palette: &Palette,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let overview = self.overview.as_ref()?;
        let task = self.state.task(overview.task)?.clone();
        let now = ZonedDateTime::now();
        let project = self.state.project_by_id(task.project_id).cloned();
        let breadcrumb: Vec<String> = {
            let mut names: Vec<String> = self
                .state
                .ancestors(task.id)
                .iter()
                .rev()
                .filter_map(|id| self.state.task(*id))
                .map(|t| t.name.clone())
                .collect();
            names.insert(
                0,
                project.as_ref().map_or(String::new(), |p| p.name.clone()),
            );
            names
        };
        let subtasks: Vec<_> = self.state.subtasks(task.id).into_iter().cloned().collect();
        let picker: Option<AnyElement> = match &overview.picker {
            OverviewPicker::Closed => None,
            OverviewPicker::Date(p) => Some(p.clone().into_any_element()),
            OverviewPicker::Reminders(e) => Some(e.clone().into_any_element()),
        };
        let id = task.id;
        let completed = task.is_completed();

        let entity = cx.entity();
        let projects = self.state.projects().to_vec();
        let project_menu = Button::new("overview-project")
            .ghost()
            .small()
            .label(
                project
                    .as_ref()
                    .map_or("Inbox".to_owned(), |p| p.name.clone()),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for p in &projects {
                    let (pid, entity) = (p.id, entity.clone());
                    menu = menu.item(PopupMenuItem::new(p.name.clone()).on_click(
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.act(window, cx, |s| s.move_task_to(id, pid))
                            });
                        },
                    ));
                }
                menu
            });

        let due_label = task.due.map(|d| {
            (
                format::due_label(d, &now),
                palette.urgency(format::urgency(d, &now)),
            )
        });
        let reminders: Vec<String> = task
            .reminders
            .iter()
            .map(|r| format::reminder_label(*r, &now))
            .collect();

        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap_2()
            .p_5()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .id("overview-check")
                            .mt(px(8.))
                            .size(px(20.))
                            .flex_none()
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(palette.muted)
                            .when(completed, |d| d.bg(palette.faint))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.act(window, cx, |s| s.toggle_completed_of(&[id]))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(Input::new(&overview.name).appearance(false)),
                            )
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .child(Textarea::new(&overview.description).appearance(false)),
                            ),
                    ),
            )
            .child(div().h(px(1.)).bg(palette.hairline).my_2())
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.muted)
                    .child(if subtasks.is_empty() {
                        "Sub-tasks".to_owned()
                    } else {
                        format!(
                            "Sub-tasks {}/{}",
                            subtasks.iter().filter(|t| t.is_completed()).count(),
                            self.state
                                .tasks()
                                .iter()
                                .filter(|t| t.parent_id == Some(id))
                                .count()
                        )
                    }),
            )
            .children(subtasks.into_iter().map(|sub| {
                let sid = sub.id;
                let done = sub.is_completed();
                div()
                    .id(("subtask", sid.0 as usize))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .py_1p5()
                    .border_b_1()
                    .border_color(palette.hairline)
                    .cursor_pointer()
                    .hover(|d| d.bg(palette.row_hover))
                    .child(
                        div()
                            .id(("subtask-check", sid.0 as usize))
                            .size(px(16.))
                            .flex_none()
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(palette.muted)
                            .when(done, |d| d.bg(palette.faint))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.act(window, cx, |s| s.toggle_completed_of(&[sid]));
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(TASK_TEXT - 1.))
                            .text_color(if done { palette.faint } else { palette.text })
                            .when(done, |d| d.line_through())
                            .child(sub.name.clone()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.save_overview(id, window, cx);
                        this.act(window, cx, |s| s.open_overview(sid));
                    }))
            }))
            .child(
                div().text_size(px(13.5)).child(
                    Input::new(&overview.subtask)
                        .small()
                        .prefix(Icon::new(Lucide::Plus).xsmall()),
                ),
            );

        let side_label = |label: &'static str| {
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.muted)
                .child(label)
        };
        let side = div()
            .flex()
            .flex_col()
            .w(px(250.))
            .flex_none()
            .gap_1()
            .p_4()
            .bg(palette.sidebar)
            .border_l_1()
            .border_color(palette.hairline)
            .child(side_label("Project"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .size(px(9.))
                            .rounded_full()
                            .bg(project.map_or(palette.accent, |p| theme::project_color(p.color))),
                    )
                    .child(project_menu),
            )
            .child(div().h(px(1.)).bg(palette.hairline).my_2())
            .child(side_label("Date"))
            .child(
                Button::new("overview-date")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Calendar))
                    .label(
                        due_label
                            .as_ref()
                            .map_or("No date".to_owned(), |(l, _)| l.clone()),
                    )
                    .when_some(due_label.map(|(_, c)| c), |b, c| b.text_color(c))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_overview_picker(true, window, cx)
                    })),
            )
            .child(div().h(px(1.)).bg(palette.hairline).my_2())
            .child(side_label("Reminders"))
            .children(
                reminders
                    .into_iter()
                    .map(|r| div().px_2().text_size(px(13.)).child(r)),
            )
            .child(
                Button::new("overview-reminders")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Bell))
                    .label(if task.reminders.is_empty() {
                        "Add reminder"
                    } else {
                        "Edit reminders"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_overview_picker(false, window, cx)
                    })),
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
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(picker),
                        ),
                    )
                    .with_priority(3),
                )
            });

        let can_step_back = self.state.overview_neighbour(-1).is_some();
        let can_step_on = self.state.overview_neighbour(1).is_some();
        Some(
            Self::card(palette, 860.)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(palette.hairline)
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(palette.muted)
                                .child(breadcrumb.join(" / ")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new("overview-prev")
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(Lucide::ChevronUp))
                                        .disabled(!can_step_back)
                                        .tooltip("Previous task (↑)")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.step(-1, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("overview-next")
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(Lucide::ChevronDown))
                                        .disabled(!can_step_on)
                                        .tooltip("Next task (↓)")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.step(1, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("overview-close")
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(Lucide::X))
                                        .tooltip("Close (Esc)")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.save_overview(id, window, cx);
                                            this.act(window, cx, |s| s.close_overview());
                                        })),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(px(320.))
                        .child(main)
                        .child(side),
                )
                .into_any_element(),
        )
    }

    fn render_confirm_delete_tasks(
        &self,
        ids: Vec<TaskId>,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match ids.as_slice() {
            [one] => format!(
                "Delete \"{}\"?",
                self.state
                    .task(*one)
                    .map(|t| t.name.clone())
                    .unwrap_or_default()
            ),
            many => format!("Delete {} tasks?", many.len()),
        };
        let subtasks = self
            .state
            .tasks()
            .iter()
            .filter(|t| self.state.ancestors(t.id).iter().any(|a| ids.contains(a)))
            .count();
        let detail = match subtasks {
            0 => "This can't be undone.".to_owned(),
            1 => "Its sub-task will be deleted too. This can't be undone.".to_owned(),
            n => format!("Their {n} sub-tasks will be deleted too. This can't be undone."),
        };
        self.confirm_card(title, detail, "Delete", palette, cx)
    }

    fn render_confirm_delete_project(
        &self,
        name: String,
        count: usize,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = match count {
            0 => "This can't be undone.".to_owned(),
            1 => "Its task will be deleted too. This can't be undone.".to_owned(),
            n => format!("Its {n} tasks will be deleted too. This can't be undone."),
        };
        self.confirm_card(
            format!("Delete \"{name}\"?"),
            detail,
            "Delete project",
            palette,
            cx,
        )
    }

    fn render_confirm_restore(
        &self,
        path: std::path::PathBuf,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.confirm_card(
            format!("Restore \"{name}\"?"),
            "Your current projects and tasks are replaced with the backup's. They are saved first, as \"Ostracon before restore …\", so you can go back.".into(),
            "Restore",
            palette,
            cx,
        )
    }

    fn confirm_card(
        &self,
        title: String,
        detail: String,
        action: &'static str,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Self::card(palette, 420.)
            .p_5()
            .gap_2()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .text_color(palette.muted)
                    .child(detail),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .mt_3()
                    .child(
                        Button::new("dialog-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.escape(&Escape, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("dialog-confirm")
                            .small()
                            .danger()
                            .label(action)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm(&Confirm, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_project_editor(&mut self, palette: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let editing = matches!(&self.state.dialog, Dialog::ProjectEditor(d) if d.project.is_some());
        let swatches = ProjectColor::ALL.into_iter().map(|color| {
            let chosen = color == self.project_color;
            div()
                .id(color.key())
                .size(px(26.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::project_color(color))
                .cursor_pointer()
                .when(chosen, |d| d.border_2().border_color(palette.text))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(color.display_name())
                        .build(window, cx)
                })
                .when(chosen, |d| {
                    d.child(
                        Icon::new(Lucide::Check)
                            .xsmall()
                            .text_color(gpui_kit::white()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.project_color = color;
                    cx.notify();
                }))
        });
        Self::card(palette, 440.)
            .p_5()
            .gap_3()
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if editing {
                        "Edit project"
                    } else {
                        "Add project"
                    }),
            )
            .when_some(self.project_name.clone(), |d, input| {
                d.child(Input::new(&input))
            })
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(palette.muted)
                    .child(format!("Colour: {}", self.project_color.display_name())),
            )
            .child(div().flex().flex_wrap().gap_2().children(swatches))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .mt_2()
                    .child(
                        Button::new("project-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.act(window, cx, |s| s.cancel_dialog())
                            })),
                    )
                    .child(
                        Button::new("project-save")
                            .small()
                            .primary()
                            .label(if editing { "Save" } else { "Add" })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_project(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_settings(&self, palette: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let backup = &self.state.backup;
        let now = ZonedDateTime::now();
        let mut body = div().flex().flex_col().gap_2();
        if !backup.available() {
            body = body.child(div().text_size(px(13.5)).text_color(palette.muted).child(
                "Backups are off because OSTRACON_DATABASE points at another database, so a test database never overwrites your real backups.",
            ));
        } else {
            let folder = backup.folder.as_ref().map(|f| f.path.display().to_string());
            let last = backup
                .policy
                .last_backup
                .map(|at| format::moment_label(at, &now));
            body = body
                .child(div().text_size(px(13.5)).text_color(palette.muted).child(
                    "Choose a folder, such as one in OneDrive, and Ostracon copies your tasks there when they change (at most once a minute) and when it quits. It keeps one file a day for the last two days. Each copy is written elsewhere first and moved in, so the sync service never sees a half-written file.",
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .mt_1()
                        .child(Icon::new(Lucide::FolderOpen).small())
                        .child(div().flex_1().text_size(px(13.5)).overflow_hidden().text_ellipsis().child(folder.clone().unwrap_or_else(|| "Backups are off".into())))
                        .child(Button::new("choose-folder").small().label(if folder.is_some() { "Change…" } else { "Choose Folder…" }).on_click(cx.listener(|this, _, window, cx| this.choose_backup_folder(window, cx))))
                        .when(folder.is_some(), |d| d.child(Button::new("backups-off").small().ghost().label("Turn Off").on_click(cx.listener(|this, _, window, cx| this.act(window, cx, |s| s.set_backup_folder(None)))))),
                )
                .when(folder.is_some(), |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(13.))
                            .text_color(palette.muted)
                            .child(last.map_or("No backup yet".to_owned(), |l| format!("Last backup: {l}")))
                            .child(Button::new("back-up-now").small().ghost().label("Back Up Now").on_click(cx.listener(|this, _, window, cx| this.act(window, cx, |s| s.back_up_now())))),
                    )
                })
                .when_some(backup.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(palette.overdue).child(format!("The last backup failed: {e}"))));
            let restorable: Vec<Backup> = backup
                .folder
                .as_ref()
                .map(|f| f.restorable())
                .unwrap_or_default();
            if !restorable.is_empty() {
                body = body
                    .child(div().h(px(1.)).bg(palette.hairline).my_2())
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Restore from backup"),
                    )
                    .children(restorable.into_iter().enumerate().map(|(i, b)| {
                        let path = b.path.clone();
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(13.5))
                            .child(b.title())
                            .child(
                                Button::new(("restore", i))
                                    .small()
                                    .ghost()
                                    .label("Restore…")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.panel = Panel::ConfirmRestore(path.clone());
                                        cx.notify();
                                    })),
                            )
                    }));
            }
        }
        let database = self
            .state
            .database_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "In memory".into());
        Self::card(palette, 560.)
            .p_5()
            .gap_3()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::X))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.panel = Panel::None;
                                this.sync(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Backups"),
            )
            .child(body)
            .child(div().h(px(1.)).bg(palette.hairline).my_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(palette.faint)
                    .child(format!("Database: {database}")),
            )
            .into_any_element()
    }

    pub fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = theme::palette(cx);
        let toast = self.state.toast.clone()?;
        let id = toast.id;
        Some(
            div()
                .absolute()
                .bottom(px(24.))
                .left(px(theme::SIDEBAR_WIDTH + 24.))
                .flex()
                .items_center()
                .gap_3()
                .pl_4()
                .pr_2()
                .py_2()
                .rounded_lg()
                .bg(if palette.dark {
                    rgb(0x3a3a3a).into()
                } else {
                    Hsla::from(rgb(0x202020))
                })
                .text_color(gpui_kit::white())
                .text_size(px(13.5))
                .shadow_lg()
                .child(toast.message.clone())
                .when(matches!(toast.undo, Some(Undo::Reopen(_))), |d| {
                    d.child(
                        div()
                            .id("toast-undo")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(rgb(0xff8a7f))
                            .font_weight(FontWeight::SEMIBOLD)
                            .hover(|d| d.bg(rgba(0xffffff1a)))
                            .child("Undo")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.act(window, cx, |s| s.undo_toast())
                            })),
                    )
                })
                .child(
                    div()
                        .id("toast-close")
                        .p_1()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|d| d.bg(rgba(0xffffff1a)))
                        .child(Icon::new(Lucide::X).xsmall())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.act(window, cx, |s| s.dismiss_toast(id))
                        })),
                )
                .into_any_element(),
        )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let root = div()
            .key_context("Workspace")
            .relative()
            .flex()
            .size_full()
            .bg(palette.page)
            .text_color(palette.text)
            .font_family(".SystemUIFont");
        let root = self.with_actions(root, cx);
        let sidebar = self.render_sidebar(cx);
        let list = div()
            .key_context("TaskList")
            .track_focus(&self.list_focus)
            .flex()
            .flex_1()
            .h_full()
            .child(self.render_list(cx));
        let modal = self.render_modal(window, cx);
        let toast = self.render_toast(cx);
        root.child(sidebar)
            .child(list)
            .children(modal)
            .children(toast)
    }
}
