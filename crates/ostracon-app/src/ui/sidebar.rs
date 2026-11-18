//! The sidebar: the Inbox and projects, with coloured icons and counts, drawn large so they are easy to click
//! and to drop tasks onto.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ostracon_core::Project;

use super::theme::{self, SIDEBAR_WIDTH};
use super::workspace::{DraggedTasks, Panel, Workspace};

impl Workspace {
    pub fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = theme::palette(cx);
        let projects = self.state.projects().to_vec();
        let (inbox, user): (Vec<Project>, Vec<Project>) =
            projects.into_iter().partition(|p| p.is_inbox);
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(palette.sidebar)
            .border_r_1()
            .border_color(palette.hairline)
            .child(
                div()
                    .id("sidebar")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_0p5()
                    .px_3()
                    .pt(px(44.))
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_scroll)
                    .children(inbox.iter().map(|p| self.render_project_row(p, 0, cx)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mt_5()
                            .mb_1()
                            .px_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.muted)
                                    .child("My Projects"),
                            )
                            .child(
                                Button::new("add-project")
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(Lucide::Plus))
                                    .tooltip("New project (⇧⌘N)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.act(window, cx, |s| s.start_new_project());
                                    })),
                            ),
                    )
                    .children(
                        user.iter()
                            .enumerate()
                            .map(|(i, p)| self.render_project_row(p, i + 1, cx)),
                    )
                    .when(user.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(px(13.))
                                .text_color(palette.faint)
                                .child("No projects yet"),
                        )
                    }),
            )
            .child(
                div()
                    .p_3()
                    .border_t_1()
                    .border_color(palette.hairline)
                    .child(
                        Button::new("settings")
                            .ghost()
                            .small()
                            .icon(Icon::new(Lucide::Settings))
                            .label("Settings")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.panel = Panel::Settings;
                                window.focus(&this.modal_focus, cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_project_row(
        &self,
        project: &Project,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = theme::palette(cx);
        let selected = project.id == self.state.project;
        let count = self.state.open_count(project.id);
        let id = project.id;
        let color = theme::project_color(project.color);
        let icon = if project.is_inbox {
            Icon::new(Lucide::Inbox)
        } else {
            Icon::new(Lucide::Hash)
        };
        let row = div()
            .id(("project", id.0 as usize))
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(38.))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(14.5))
            .text_color(palette.text)
            .when(selected, |d| {
                d.bg(palette.sidebar_selected)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!selected, |d| d.hover(|d| d.bg(palette.sidebar_hover)))
            .drag_over::<DraggedTasks>(move |style, _, _, _| {
                style
                    .bg(palette.sidebar_selected)
                    .border_1()
                    .border_color(color)
            })
            .on_drop(cx.listener(move |this, drag: &DraggedTasks, window, cx| {
                let ids = drag.ids.clone();
                this.act(window, cx, |s| {
                    s.move_to_project(&ids, id);
                });
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.act(window, cx, |s| s.select_project(id))
            }))
            .child(icon.text_color(color).small())
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(project.name.clone()),
            )
            .when(count > 0, |d| {
                d.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(palette.faint)
                        .child(count.to_string()),
                )
            })
            .when(index < 9, |d| {
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(format!("⌘{}", index + 1))
                        .build(window, cx)
                })
            });
        if project.is_inbox {
            return row.test_support().into_any_element();
        }
        let entity = cx.entity();
        // The menu lives on a wrapper so the row itself can be registered for UI tests.
        div()
            .id(("project-menu", id.0 as usize))
            .child(row.test_support())
            .context_menu(move |menu, _, _| {
                let (edit, delete) = (entity.clone(), entity.clone());
                menu.item(
                    PopupMenuItem::new("Edit project…").on_click(move |_, window, cx| {
                        edit.update(cx, |this, cx| {
                            this.act(window, cx, |s| s.start_editing_project(id))
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("Delete project…").on_click(move |_, window, cx| {
                        delete.update(cx, |this, cx| {
                            this.act(window, cx, |s| s.request_delete_project(id))
                        });
                    }),
                )
            })
            .into_any_element()
    }
}
