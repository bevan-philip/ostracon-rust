//! The task list: a flat, Todoist-style outline with hairline dividers that start at the checkbox, disclosure
//! chevrons hanging in the left margin, and the composer inline where it belongs.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use ostracon_core::TaskId;
use ostracon_core::tree::TaskRow;

use super::actions::{Complete, Delete, Edit, NewSubtask, OpenOverview};
use super::theme::{self, CHECKBOX, INDENT, META_TEXT, Palette, TASK_TEXT};
use super::workspace::{DraggedTasks, DropHover, Workspace};
use crate::format;
use crate::state::{Composer, DropTarget, ListItem};

/// The list's left edge to the checkbox: room for the chevrons hanging in the margin.
const GUTTER: f32 = 40.;
const MAX_WIDTH: f32 = 820.;

impl Workspace {
    pub fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = theme::palette(cx);
        let now = ZonedDateTime::now();
        let items = self.state.list_items();
        let show_completed = self.state.show_completed;
        let empty = self.state.rows().is_empty() && !self.state.composer.is_in_list();
        let row_count = self.state.rows().len();
        let drop = self.drop_target(cx);

        let mut rows: Vec<AnyElement> = Vec::new();
        // Each task's index in the outline, which the composer may stand in for.
        let mut index = 0;
        for item in items {
            match item {
                ListItem::Task(row) => {
                    rows.push(self.render_row(&row, index, drop, &now, &palette, cx));
                    index += 1;
                }
                ListItem::Composer { mode, depth } => {
                    if matches!(mode, Composer::Editing { .. }) {
                        index += 1;
                    }
                    if let Some(composer) = &self.composer {
                        rows.push(
                            div()
                                .pl(px(GUTTER + depth as f32 * INDENT - 4.))
                                .py_2()
                                .child(composer.clone())
                                .into_any_element(),
                        );
                    }
                }
            }
        }
        if let Some(target) = drop.filter(|t| t.before.is_none()) {
            rows.push(
                div()
                    .relative()
                    .child(drop_line(&palette, target.depth))
                    .into_any_element(),
            );
        }
        if !self.state.composer.is_in_list() {
            rows.push(
                div()
                    .id("add-task")
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<DraggedTasks>, _, cx| {
                            this.hover_gap(event, row_count, cx)
                        },
                    ))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .pl(px(GUTTER))
                    .py_2()
                    .cursor_pointer()
                    .text_size(px(TASK_TEXT))
                    .text_color(palette.muted)
                    .hover(|d| d.text_color(palette.accent))
                    .child(Icon::new(Lucide::Plus).small().text_color(palette.accent))
                    .child("Add task")
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.act(window, cx, |s| s.start_adding())
                        }),
                    )
                    .into_any_element(),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(palette.page)
            .child(
                div()
                    .id("task-list")
                    .on_drag_move(cx.listener(
                        |this, event: &DragMoveEvent<DraggedTasks>, _, cx| {
                            if !event.bounds.contains(&event.event.position)
                                && this.drop_hover.take().is_some()
                            {
                                cx.notify();
                            }
                        },
                    ))
                    .on_drop(cx.listener(|this, drag: &DraggedTasks, window, cx| {
                        let Some(hover) = this.drop_hover.take() else {
                            return;
                        };
                        let target = this.state.drop_target(&drag.ids, hover.gap, hover.depth);
                        this.act(window, cx, |s| s.drop_tasks(&drag.ids, target));
                    }))
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .max_w(px(MAX_WIDTH))
                            .mx_auto()
                            .w_full()
                            .px_6()
                            .pt(px(44.))
                            .pb(px(120.))
                            .child(self.render_header(&palette, show_completed, cx))
                            .children(self.render_banners(&palette, cx))
                            .children(rows)
                            .when(empty, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap_1()
                                        .mt(px(80.))
                                        .child(div().text_size(px(16.)).text_color(palette.text).child("All clear"))
                                        .child(
                                            div()
                                                .text_size(px(13.5))
                                                .text_color(palette.muted)
                                                .child("Press A or Q to add a task. Try \"Pay rent tomorrow 9am #Home\"."),
                                        ),
                                )
                            }),
                    ),
            )
    }

    /// Where the dragged tasks would land, while they are dragged over the list.
    fn drop_target(&self, cx: &App) -> Option<DropTarget> {
        let hover = self.drop_hover.as_ref().filter(|_| cx.has_active_drag())?;
        Some(self.state.drop_target(&hover.ids, hover.gap, hover.depth))
    }

    /// Notes that the dragged tasks are over the gap before row `gap`. Dragging sideways from where the drag
    /// began asks for one level deeper or shallower for every indent's width.
    fn hover_gap(
        &mut self,
        event: &DragMoveEvent<DraggedTasks>,
        gap: usize,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position;
        if !event.bounds.contains(&position) {
            return;
        }
        let (start, depth) = self.drag_origin.unwrap_or((position.x, 0));
        let shift = ((position.x - start) / px(INDENT)).round() as isize;
        let hover = DropHover {
            ids: event.drag(cx).ids.clone(),
            gap,
            depth: depth as isize + shift,
        };
        if self.drop_hover.as_ref() != Some(&hover) {
            self.drop_hover = Some(hover);
            cx.notify();
        }
    }

    fn render_header(
        &self,
        palette: &Palette,
        show_completed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let name = self
            .state
            .current_project()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        div()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(GUTTER))
            .mb_3()
            .child(
                div()
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.text)
                    .child(name),
            )
            .child(
                Button::new("show-completed")
                    .ghost()
                    .small()
                    .icon(Icon::new(if show_completed {
                        Lucide::EyeOff
                    } else {
                        Lucide::Eye
                    }))
                    .label(if show_completed {
                        "Hide completed"
                    } else {
                        "Show completed"
                    })
                    .tooltip("⇧⌘C")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.act(window, cx, |s| s.set_show_completed(!s.show_completed))
                    })),
            )
    }

    fn render_banners(&self, palette: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut banners = Vec::new();
        if let Some(error) = self.state.error.clone() {
            banners.push(
                banner(palette, palette.overdue.opacity(0.1), palette.overdue)
                    .child(div().flex_1().child(error))
                    .child(
                        Button::new("dismiss-error")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::X))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.error = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        if self.notifications_blocked() {
            banners.push(
                banner(palette, palette.warning_bg, palette.warning_text)
                    .child(Icon::new(Lucide::BellOff).small())
                    .child(div().flex_1().child(
                        "Notifications are off for Ostracon, so reminders won't appear. Turn them on in System Settings → Notifications.",
                    ))
                    .into_any_element(),
            );
        }
        banners
    }

    fn render_row(
        &self,
        row: &TaskRow,
        index: usize,
        drop: Option<DropTarget>,
        now: &ZonedDateTime,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task = &row.task;
        let id = task.id;
        let selected = self.state.selection.contains(id);
        let completed = task.is_completed();
        let indent = row.depth as f32 * INDENT;
        let depth = row.depth;
        let dragged = drop.is_some()
            && self
                .drop_hover
                .as_ref()
                .is_some_and(|h| h.ids.contains(&id));

        let chevron = row.has_children().then(|| {
            let collapsed = task.collapsed;
            div()
                .id(("chevron", id.0 as usize))
                .absolute()
                .left(px(-26.))
                .top(px(11.))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .text_color(palette.faint)
                .hover(|d| d.bg(palette.sidebar_hover).text_color(palette.text))
                .child(
                    Icon::new(if collapsed {
                        Lucide::ChevronRight
                    } else {
                        Lucide::ChevronDown
                    })
                    .small(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.act(window, cx, |s| s.set_collapsed(id, !collapsed));
                }))
        });

        let checkbox = div()
            .id(("check", id.0 as usize))
            .group("check")
            .flex_none()
            .mt(px(3.))
            .size(px(CHECKBOX))
            .rounded_full()
            .border(px(1.5))
            .border_color(if completed {
                palette.faint
            } else {
                palette.muted
            })
            .when(completed, |d| d.bg(palette.faint))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                div()
                    .text_color(if completed {
                        palette.page
                    } else {
                        transparent_black()
                    })
                    .when(!completed, |d| {
                        d.group_hover("check", move |s| s.text_color(palette.muted))
                    })
                    .child(Icon::new(Lucide::Check).xsmall()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.act(window, cx, |s| s.toggle_completed_of(&[id]));
            }));

        let mut meta: Vec<AnyElement> = Vec::new();
        if let Some(due) = task.due {
            let color = if completed {
                palette.faint
            } else {
                palette.urgency(format::urgency(due, now))
            };
            meta.push(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(color)
                    .child(Icon::new(Lucide::Calendar).xsmall())
                    .child(format::due_label(due, now))
                    .into_any_element(),
            );
        }
        if !task.reminders.is_empty() {
            meta.push(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(Icon::new(Lucide::Bell).xsmall())
                    .when(task.reminders.len() > 1, |d| {
                        d.child(task.reminders.len().to_string())
                    })
                    .into_any_element(),
            );
        }
        if row.subtask_count > 0 {
            meta.push(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(Icon::new(Lucide::GitBranch).xsmall())
                    .child(format!(
                        "{}/{}",
                        row.completed_subtask_count, row.subtask_count
                    ))
                    .into_any_element(),
            );
        }
        let hidden = row.hidden_completed_count();
        if hidden > 0 && !self.state.show_completed {
            meta.push(
                div()
                    .child(format!("{hidden} completed hidden"))
                    .into_any_element(),
            );
        }

        let description = task
            .description
            .lines()
            .next()
            .map(str::to_owned)
            .filter(|d| !d.is_empty());
        let drag = {
            let ids = self.state.acting_on(Some(id));
            let label: SharedString = if ids.len() > 1 {
                format!("{} tasks", ids.len()).into()
            } else {
                task.name.clone().into()
            };
            DraggedTasks { ids, label }
        };

        let row = div()
            .id(("task", id.0 as usize))
            .relative()
            .pl(px(GUTTER + indent))
            .cursor_pointer()
            .when(selected, |d| d.bg(palette.row_selected))
            .when(!selected, |d| d.hover(|d| d.bg(palette.row_hover)))
            .rounded_md()
            .when(dragged, |d| d.opacity(0.4))
            .when_some(drop.filter(|t| t.before == Some(id)), |d, target| {
                d.child(drop_line(palette, target.depth))
            })
            // The drop target receives this value, not the preview view, so it must carry the ids.
            .on_drag(drag, |drag: &DraggedTasks, _, _, cx| {
                cx.new(|_| drag.clone())
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, _| {
                    this.drag_origin = Some((event.position.x, depth));
                }),
            )
            // The lower half of a row is the gap below it.
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DraggedTasks>, _, cx| {
                    let lower = event.event.position.y > event.bounds.center().y;
                    this.hover_gap(event, index + usize::from(lower), cx)
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_row(id, event, window, cx)
            }))
            // A right click acts on the selection if it lands inside it, otherwise selects just this row, as in
            // Finder. It runs before the context menu is built.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    if !this.state.selection.contains(id) {
                        this.act(window, cx, |s| s.select_only(id));
                    }
                }),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .gap(px(10.))
                    .py(px(9.))
                    .border_b_1()
                    .border_color(palette.hairline)
                    .children(chevron)
                    .child(checkbox)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(TASK_TEXT))
                                    .line_height(px(24.))
                                    .text_color(if completed {
                                        palette.faint
                                    } else {
                                        palette.text
                                    })
                                    .when(completed, |d| d.line_through())
                                    .child(task.name.clone()),
                            )
                            .when_some(description, |d, text| {
                                d.child(
                                    div()
                                        .text_size(px(META_TEXT))
                                        .text_color(palette.muted)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(text),
                                )
                            })
                            .when(!meta.is_empty(), |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .text_size(px(META_TEXT))
                                        .text_color(palette.muted)
                                        .children(meta),
                                )
                            }),
                    ),
            );

        // The menu lives on a wrapper so the row itself can be registered for UI tests.
        div()
            .id(("task-menu", id.0 as usize))
            .child(row.test_support())
            .context_menu(self.task_menu(completed, cx.entity().downgrade()))
            .into_any_element()
    }

    /// The task context menu. Its items are the list's own actions, sent to the list, so they act on the
    /// selection (which the right click has just set) and show their shortcuts.
    fn task_menu(
        &self,
        completed: bool,
        workspace: WeakEntity<Workspace>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let list_focus = self.list_focus.clone();
        move |menu, window, cx| {
            let Some(workspace) = workspace.upgrade() else {
                return menu;
            };
            let (count, projects) = workspace.read_with(cx, |w, _| {
                (w.state.acting_on(None).len(), w.state.projects().to_vec())
            });
            let many = count > 1;
            let move_target = workspace.downgrade();
            menu.action_context(list_focus.clone())
                .menu_with_disabled("Open", Box::new(OpenOverview), many)
                .menu_with_disabled("Edit", Box::new(Edit), many)
                .menu_with_disabled("Add sub-task", Box::new(NewSubtask), many)
                .separator()
                .menu(
                    if completed { "Reopen" } else { "Complete" },
                    Box::new(Complete),
                )
                .submenu("Move to", window, cx, move |mut menu, _, _| {
                    for project in &projects {
                        let (pid, target) = (project.id, move_target.clone());
                        menu = menu.item(PopupMenuItem::new(project.name.clone()).on_click(
                            move |_, window, cx| {
                                if let Some(target) = target.upgrade() {
                                    target.update(cx, |this, cx| {
                                        this.act(window, cx, |s| {
                                            let ids = s.acting_on(None);
                                            s.move_to_project(&ids, pid);
                                        })
                                    });
                                }
                            },
                        ));
                    }
                    menu
                })
                .separator()
                .menu(
                    if many {
                        format!("Delete {count} tasks…")
                    } else {
                        "Delete…".to_owned()
                    },
                    Box::new(Delete),
                )
        }
    }

    /// ⌘-click toggles, ⇧-click extends, and a plain click opens the overview, as in Todoist. An open composer
    /// closes for it, first asking whether to discard anything unsaved in it.
    fn click_row(
        &mut self,
        id: TaskId,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        if modifiers.platform {
            self.act(window, cx, |s| s.toggle_selection(id));
        } else if modifiers.shift {
            self.act(window, cx, |s| s.extend_selection_to(id));
        } else if self.state.composer.is_in_list()
            && self
                .composer
                .as_ref()
                .is_some_and(|c| c.read(cx).has_changes(cx))
        {
            self.act(window, cx, |s| s.request_discard_to_open(id));
            // The dialog takes the keyboard, so Return discards and Esc goes back to the composer.
            window.focus(&self.modal_focus, cx);
        } else {
            self.act(window, cx, |s| {
                s.select_only(id);
                s.open_overview(id);
            });
        }
    }
}

/// Where dropped tasks would go: a line along the top of the row they would go above, `depth` levels in.
fn drop_line(palette: &Palette, depth: usize) -> Div {
    div()
        .absolute()
        .top(px(-1.))
        .left(px(GUTTER + depth as f32 * INDENT - 4.))
        .right_0()
        .h(px(2.))
        .rounded_full()
        .bg(palette.accent)
}

fn banner(palette: &Palette, background: Hsla, text: Hsla) -> Div {
    let _ = palette;
    div()
        .flex()
        .items_center()
        .gap_2()
        .ml(px(GUTTER))
        .mb_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(background)
        .text_color(text)
        .text_size(px(13.))
}
