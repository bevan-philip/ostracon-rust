//! The window's root view. It owns the [`AppState`] and turns its state types into views: after every change
//! [`Workspace::sync`] creates or drops the composer, overview and dialog inputs so they match the state.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;
use ostracon_core::quickadd::Draft;
use ostracon_core::{ProjectColor, TaskId};

use super::actions::*;
use super::composer::{ComposerEvent, ComposerSeed, ComposerView};
use super::pickers::{DatePicker, PickerEvent, ReminderEditor};
use crate::platform::{NotificationResponse, Notifier, Permission};
use crate::state::{AppState, Composer, Dialog};

/// Tasks being dragged onto a project in the sidebar.
#[derive(Clone)]
pub struct DraggedTasks {
    pub ids: Vec<TaskId>,
    pub label: SharedString,
}

impl Render for DraggedTasks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = super::theme::palette(cx);
        div()
            .px_3()
            .py_1p5()
            .rounded_md()
            .bg(palette.card)
            .border_1()
            .border_color(palette.card_border)
            .shadow_lg()
            .text_size(px(13.5))
            .text_color(palette.text)
            .child(self.label.clone())
    }
}

/// The open task overview's fields.
pub struct OverviewInputs {
    pub task: TaskId,
    pub name: Entity<InputState>,
    pub description: Entity<TextareaState>,
    pub subtask: Entity<InputState>,
    pub picker: OverviewPicker,
    pub _subscriptions: Vec<Subscription>,
}

pub enum OverviewPicker {
    Closed,
    Date(Entity<DatePicker>),
    Reminders(Entity<ReminderEditor>),
}

/// Overlays that aren't about tasks or projects, so they live here rather than in [`AppState`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    None,
    Settings,
    /// Waiting to confirm restoring this backup.
    ConfirmRestore(PathBuf),
}

pub struct Workspace {
    pub state: AppState,
    pub notifier: Option<Notifier>,
    pub list_focus: FocusHandle,
    pub modal_focus: FocusHandle,
    pub composer: Option<Entity<ComposerView>>,
    pub overview: Option<OverviewInputs>,
    /// The project editor's name field, while it is open.
    pub project_name: Option<Entity<InputState>>,
    pub project_color: ProjectColor,
    pub panel: Panel,
    pub list_scroll: ScrollHandle,
    /// A single click opens the overview shortly after, unless a double click (edit) arrives first.
    pub pending_open: Option<Task<()>>,
    pub toast_timer: Option<(u64, Task<()>)>,
    pub sidebar_scroll: ScrollHandle,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        state: AppState,
        notifier: Option<Notifier>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list_focus = cx.focus_handle();
        let modal_focus = cx.focus_handle();
        let mut workspace = Workspace {
            state,
            notifier,
            list_focus,
            modal_focus,
            composer: None,
            overview: None,
            project_name: None,
            project_color: ProjectColor::Charcoal,
            panel: Panel::None,
            list_scroll: ScrollHandle::new(),
            pending_open: None,
            toast_timer: None,
            sidebar_scroll: ScrollHandle::new(),
            _tasks: Vec::new(),
            _subscriptions: Vec::new(),
        };
        // Pick up changes from the CLI or MCP server, take backups, and hear back from notifications.
        workspace
            ._tasks
            .push(cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    if this
                        .update_in(cx, |this, window, cx| this.tick(window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        // Notification permission can change in System Settings while the app is in the background.
        workspace
            ._subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    if let Some(notifier) = &this.notifier {
                        notifier.refresh_permission();
                    }
                    cx.notify();
                }
            }));
        workspace._subscriptions.push(cx.on_app_quit(|this, _| {
            this.state.back_up_if_changed(false);
            async {}
        }));
        window.focus(&workspace.list_focus, cx);
        workspace.sync(window, cx);
        workspace
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = self.state.reload_if_changed_elsewhere();
        let responses = self
            .notifier
            .as_ref()
            .map(Notifier::take_responses)
            .unwrap_or_default();
        for response in responses {
            match response {
                NotificationResponse::Open(id) => {
                    self.state.reveal(id);
                    cx.activate(true);
                }
                NotificationResponse::Complete(id) => self.state.complete_from_notification(id),
            }
            changed = true;
        }
        self.state.back_up_if_changed(true);
        if changed {
            self.sync(window, cx);
        }
    }

    pub fn notifications_blocked(&self) -> bool {
        self.notifier
            .as_ref()
            .is_some_and(|n| n.permission() == Permission::Denied)
    }

    /// Brings the views in line with the state after it changed.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_composer(window, cx);
        self.sync_overview(window, cx);
        self.sync_project_editor(window, cx);
        if let Some(notifier) = &mut self.notifier {
            notifier.schedule(&self.state.notifications());
        }
        self.sync_toast(window, cx);
        self.scroll_to_selection();
        let modal = self.overview.is_some()
            || self.state.dialog != Dialog::None
            || self.panel != Panel::None;
        let composing = self.composer.is_some();
        if !modal && !composing && !self.list_focus.contains_focused(window, cx) {
            window.focus(&self.list_focus, cx);
        }
        cx.notify();
    }

    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.state.composer;
        let current = self.composer.as_ref().map(|c| c.read(cx).mode);
        if current == Some(wanted) {
            let projects = self.state.projects().to_vec();
            if let Some(composer) = &self.composer {
                composer.update(cx, |c, cx| c.set_projects(projects, cx));
            }
            return;
        }
        self.composer = None;
        let seed = match wanted {
            Composer::Closed => return,
            Composer::Adding { parent } => ComposerSeed {
                mode: wanted,
                draft: Draft::new(),
                description: String::new(),
                project: parent
                    .and_then(|p| self.state.task(p))
                    .map_or(self.state.project, |t| t.project_id),
                due: None,
                reminders: Vec::new(),
            },
            Composer::Editing { task } => {
                let Some(task) = self.state.task(task) else {
                    return;
                };
                ComposerSeed {
                    mode: wanted,
                    // An existing name is literal: it is never re-read for dates or projects.
                    draft: Draft::from_text(&task.name, true),
                    description: task.description.clone(),
                    project: task.project_id,
                    due: task.due,
                    reminders: task.reminders.clone(),
                }
            }
        };
        let projects = self.state.projects().to_vec();
        let composer = cx.new(|cx| ComposerView::new(seed, projects, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &composer,
            window,
            |this, composer, event, window, cx| match event {
                ComposerEvent::Submit => this.submit_composer(composer.clone(), window, cx),
                ComposerEvent::Cancel => {
                    this.state.close_composer();
                    this.sync(window, cx);
                }
            },
        ));
        composer.update(cx, |c, cx| c.focus(window, cx));
        self.composer = Some(composer);
    }

    fn submit_composer(
        &mut self,
        composer: Entity<ComposerView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (draft, description, project, due, reminders) = {
            let c = composer.read(cx);
            (
                c.draft(cx),
                c.description(cx),
                c.project(),
                c.due(),
                c.reminders().to_vec(),
            )
        };
        let adding = matches!(self.state.composer, Composer::Adding { .. });
        if self
            .state
            .submit_composer(&draft, &description, project, due, &reminders)
            && adding
        {
            // Return saves and opens a fresh composer for the next task.
            composer.update(cx, |c, cx| c.reset_for_next(window, cx));
        }
        self.sync(window, cx);
    }

    fn sync_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.state.overview;
        if self.overview.as_ref().map(|o| o.task) == wanted {
            return;
        }
        self.overview = None;
        let Some(task) = wanted.and_then(|id| self.state.task(id)).cloned() else {
            return;
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(task.name.clone()));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 12)
                .placeholder("Description")
                .default_value(task.description.clone())
        });
        let subtask = cx.new(|cx| InputState::new(window, cx).placeholder("Add sub-task"));
        let id = task.id;
        let subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        this.save_overview(id, window, cx)
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(&description, window, move |this, _, event, window, cx| {
                if let InputEvent::Blur = event {
                    this.save_overview(id, window, cx);
                }
            }),
            cx.subscribe_in(&subtask, window, move |this, input, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = input.read(cx).value().to_string();
                    if this.state.add_subtask(id, &text) {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                    }
                    this.sync(window, cx);
                }
            }),
        ];
        window.focus(&self.modal_focus, cx);
        self.overview = Some(OverviewInputs {
            task: id,
            name,
            description,
            subtask,
            picker: OverviewPicker::Closed,
            _subscriptions: subscriptions,
        });
    }

    pub fn save_overview(&mut self, id: TaskId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(overview) = self.overview.as_ref().filter(|o| o.task == id) else {
            return;
        };
        let name = overview.name.read(cx).value().to_string();
        let description = overview.description.read(cx).value().to_string();
        self.state.save_task(id, &name, &description);
        self.sync(window, cx);
    }

    pub fn open_overview_picker(
        &mut self,
        date: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(overview) = &mut self.overview else {
            return;
        };
        let Some(task) = self.state.task(overview.task).cloned() else {
            return;
        };
        let already = matches!(
            (&overview.picker, date),
            (OverviewPicker::Date(_), true) | (OverviewPicker::Reminders(_), false)
        );
        overview.picker = OverviewPicker::Closed;
        if already {
            cx.notify();
            return;
        }
        let id = task.id;
        if date {
            let picker = cx.new(|cx| DatePicker::new(task.due, window, cx));
            overview._subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event: &PickerEvent<Option<_>>, window, cx| {
                    let PickerEvent::Chosen(due) = event;
                    this.state.set_due_of_only(id, *due);
                    if let Some(o) = &mut this.overview {
                        o.picker = OverviewPicker::Closed;
                    }
                    this.sync(window, cx);
                },
            ));
            picker.update(cx, |p, cx| p.focus(window, cx));
            overview.picker = OverviewPicker::Date(picker);
        } else {
            let editor =
                cx.new(|cx| ReminderEditor::new(task.reminders.clone(), task.due, window, cx));
            overview._subscriptions.push(cx.subscribe_in(
                &editor,
                window,
                move |this, _, event: &PickerEvent<Vec<_>>, window, cx| {
                    let PickerEvent::Chosen(reminders) = event;
                    this.state.set_reminders(id, reminders);
                    this.sync(window, cx);
                },
            ));
            editor.update(cx, |e, cx| e.focus(window, cx));
            overview.picker = OverviewPicker::Reminders(editor);
        }
        cx.notify();
    }

    fn sync_project_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.state.dialog {
            Dialog::ProjectEditor(draft) => {
                if self.project_name.is_none() {
                    let name = draft.name.clone();
                    self.project_color = draft.color;
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder("Project name")
                            .default_value(name)
                    });
                    self._subscriptions.push(cx.subscribe_in(
                        &input,
                        window,
                        |this, _, event, window, cx| {
                            if let InputEvent::PressEnter { .. } = event {
                                this.save_project(window, cx);
                            }
                        },
                    ));
                    input.update(cx, |i, cx| i.focus(window, cx));
                    self.project_name = Some(input);
                }
            }
            Dialog::None => self.project_name = None,
            _ => {
                self.project_name = None;
                window.focus(&self.modal_focus, cx);
            }
        }
    }

    pub fn save_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Dialog::ProjectEditor(draft) = &self.state.dialog else {
            return;
        };
        let mut draft = draft.clone();
        draft.name = self
            .project_name
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        draft.color = self.project_color;
        self.state.save_project(&draft);
        self.sync(window, cx);
    }

    fn sync_toast(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.state.toast.as_ref().map(|t| t.id) else {
            self.toast_timer = None;
            return;
        };
        if self
            .toast_timer
            .as_ref()
            .is_some_and(|(timer_id, _)| *timer_id == id)
        {
            return;
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.state.dismiss_toast(id);
                this.sync(window, cx);
            });
        });
        self.toast_timer = Some((id, task));
    }

    fn scroll_to_selection(&self) {
        if let Some(index) = self
            .state
            .selection
            .anchor()
            .and_then(|a| self.state.rows().iter().position(|r| r.id() == a))
        {
            self.list_scroll.scroll_to_item(index);
        }
    }

    /// Runs `change` on the state, then syncs.
    pub fn act(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut AppState),
    ) {
        change(&mut self.state);
        self.sync(window, cx);
    }

    pub fn choose_backup_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Back Up Here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.act(window, cx, |s| s.set_backup_folder(Some(folder)))
                });
            }
        })
        .detach();
    }

    // Actions

    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn new_task(&mut self, _: &NewTask, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::None;
        self.act(window, cx, AppState::start_adding);
    }

    fn new_subtask(&mut self, _: &NewSubtask, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.state.overview.or(self.state.selection.anchor());
        self.act(window, cx, |s| s.start_adding_subtask(parent));
    }

    fn new_project(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, AppState::start_new_project);
    }

    fn toggle_show_completed(
        &mut self,
        _: &ToggleShowCompleted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.act(window, cx, |s| s.set_show_completed(!s.show_completed));
    }

    fn open_overview(&mut self, _: &OpenOverview, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.selection.anchor() {
            self.act(window, cx, |s| s.open_overview(id));
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::Settings;
        window.focus(&self.modal_focus, cx);
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.toast.as_ref().is_some_and(|t| t.undo.is_some()) {
            self.act(window, cx, AppState::undo_toast);
        }
    }

    fn select_project(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.select_project_at(index));
    }

    fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        self.step(1, window, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.step(-1, window, cx);
    }

    /// ↑/↓: in the list, move the selection; in the overview, step to the neighbouring task.
    pub fn step(&mut self, offset: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.overview.is_some() {
            if let Some(id) = self.state.overview_neighbour(offset) {
                if let Some(open) = self.state.overview {
                    self.save_overview(open, window, cx);
                }
                self.act(window, cx, |s| s.open_overview(id));
            }
        } else {
            self.act(window, cx, |s| s.move_selection(offset));
        }
    }

    fn extend_next(&mut self, _: &ExtendNext, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.extend_selection_by(1));
    }

    fn extend_previous(&mut self, _: &ExtendPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.extend_selection_by(-1));
    }

    fn collapse(&mut self, _: &CollapseOrParent, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, AppState::collapse_or_select_parent);
    }

    fn expand(&mut self, _: &Expand, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, AppState::expand_selected);
    }

    fn complete(&mut self, _: &Complete, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.toggle_completed(None));
    }

    fn edit(&mut self, _: &Edit, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.start_editing(None));
    }

    fn indent(&mut self, _: &Indent, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.indent(None));
    }

    fn outdent(&mut self, _: &Outdent, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.outdent(None));
    }

    fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.move_by(None, -1));
    }

    fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.move_by(None, 1));
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.act(window, cx, |s| s.request_delete(None));
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if let Panel::ConfirmRestore(path) = &self.panel {
            let path = path.clone();
            self.panel = Panel::Settings;
            self.act(window, cx, |s| s.restore(&path));
            return;
        }
        match self.state.dialog {
            Dialog::ConfirmDeleteTasks(_) | Dialog::ConfirmDeleteProject(_) => {
                self.act(window, cx, AppState::confirm_dialog)
            }
            Dialog::ProjectEditor(_) => self.save_project(window, cx),
            Dialog::None => {}
        }
    }

    /// Esc closes the topmost layer: a picker, a confirmation, settings, a dialog, the overview, then the
    /// selection.
    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overview) = &mut self.overview
            && !matches!(overview.picker, OverviewPicker::Closed)
        {
            overview.picker = OverviewPicker::Closed;
            cx.notify();
            return;
        }
        match &self.panel {
            Panel::ConfirmRestore(_) => {
                self.panel = Panel::Settings;
                cx.notify();
                return;
            }
            Panel::Settings => {
                self.panel = Panel::None;
                self.sync(window, cx);
                return;
            }
            Panel::None => {}
        }
        if self.state.dialog != Dialog::None {
            self.act(window, cx, AppState::cancel_dialog);
        } else if let Some(id) = self.state.overview {
            self.save_overview(id, window, cx);
            self.act(window, cx, AppState::close_overview);
        } else {
            self.act(window, cx, AppState::clear_selection);
        }
    }

    /// Hooks every action up to the root element.
    pub fn with_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::new_task))
            .on_action(cx.listener(Self::new_subtask))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::toggle_show_completed))
            .on_action(cx.listener(Self::open_overview))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(|this, _: &Project1, w, cx| this.select_project(0, w, cx)))
            .on_action(cx.listener(|this, _: &Project2, w, cx| this.select_project(1, w, cx)))
            .on_action(cx.listener(|this, _: &Project3, w, cx| this.select_project(2, w, cx)))
            .on_action(cx.listener(|this, _: &Project4, w, cx| this.select_project(3, w, cx)))
            .on_action(cx.listener(|this, _: &Project5, w, cx| this.select_project(4, w, cx)))
            .on_action(cx.listener(|this, _: &Project6, w, cx| this.select_project(5, w, cx)))
            .on_action(cx.listener(|this, _: &Project7, w, cx| this.select_project(6, w, cx)))
            .on_action(cx.listener(|this, _: &Project8, w, cx| this.select_project(7, w, cx)))
            .on_action(cx.listener(|this, _: &Project9, w, cx| this.select_project(8, w, cx)))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::extend_next))
            .on_action(cx.listener(Self::extend_previous))
            .on_action(cx.listener(Self::collapse))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::complete))
            .on_action(cx.listener(Self::edit))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::escape))
    }
}
