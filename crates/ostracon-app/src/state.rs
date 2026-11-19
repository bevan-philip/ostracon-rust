//! The app's state and the actions that change it, with no GPUI in sight, so it can be tested headless.
//!
//! Rules:
//! - Every write goes through [`AppState::perform`], which runs it as one transaction, reloads, and then
//!   reconciles. UI state changes only once a write has succeeded, so a rolled-back change never leaves the
//!   screen showing something that didn't happen.
//! - [`AppState::reconcile`] runs after every reload and fixes up everything that refers to projects or tasks:
//!   selections and editors pointing at tasks that are gone or hidden, overviews and dialogs whose target was
//!   deleted.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use jiff::{Timestamp, ZonedDateTime};
use ostracon_core::backup::{BackupFolder, BackupPolicy, timestamp};
use ostracon_core::dates::DateParser;
use ostracon_core::quickadd::{BlockValue, Draft, QuickAddResult};
use ostracon_core::repo::{self, NewTask, Snapshot};
use ostracon_core::schedule::{self, NOTIFICATION_LIMIT};
use ostracon_core::settings::Settings;
use ostracon_core::tree::{self, TaskRow};
use ostracon_core::{Due, Project, ProjectColor, ProjectId, Reminder, Repository, Task, TaskId};

/// Which tasks are selected. The anchor is where the keyboard acts and where a Shift range starts; it is
/// always one of `ids`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    Empty,
    Tasks {
        anchor: TaskId,
        ids: BTreeSet<TaskId>,
        /// The far end of a range grown with Shift, which moves while the anchor stays put.
        range_end: Option<TaskId>,
    },
}

impl Selection {
    pub fn single(id: TaskId) -> Self {
        Selection::Tasks {
            anchor: id,
            ids: BTreeSet::from([id]),
            range_end: None,
        }
    }

    pub fn anchor(&self) -> Option<TaskId> {
        match self {
            Selection::Empty => None,
            Selection::Tasks { anchor, .. } => Some(*anchor),
        }
    }

    pub fn contains(&self, id: TaskId) -> bool {
        matches!(self, Selection::Tasks { ids, .. } if ids.contains(&id))
    }

    pub fn len(&self) -> usize {
        match self {
            Selection::Empty => 0,
            Selection::Tasks { ids, .. } => ids.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ids(&self) -> BTreeSet<TaskId> {
        match self {
            Selection::Empty => BTreeSet::new(),
            Selection::Tasks { ids, .. } => ids.clone(),
        }
    }
}

/// The inline composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Composer {
    #[default]
    Closed,
    /// Adding a task at the end of the list, or at the end of `parent`'s subtasks.
    Adding { parent: Option<TaskId> },
    /// Adding a subtask to `parent` from its open overview, below its subtasks there.
    AddingInOverview { parent: TaskId },
    /// Editing a task in place.
    Editing { task: TaskId },
}

impl Composer {
    /// Whether the composer is open in the list, rather than closed or in the overview.
    pub fn is_in_list(self) -> bool {
        matches!(self, Composer::Adding { .. } | Composer::Editing { .. })
    }

    /// The task a subtask is being added to.
    pub fn parent(self) -> Option<TaskId> {
        match self {
            Composer::Adding { parent } => parent,
            Composer::AddingInOverview { parent } => Some(parent),
            Composer::Closed | Composer::Editing { .. } => None,
        }
    }
}

/// A modal dialog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Dialog {
    #[default]
    None,
    /// Creating (no id) or editing a project.
    ProjectEditor(ProjectDraft),
    ConfirmDeleteTasks(Vec<TaskId>),
    ConfirmDeleteProject(ProjectId),
    /// Discarding what's unsaved in the list's composer, to open the overview of the task clicked.
    ConfirmDiscard(TaskId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDraft {
    pub project: Option<ProjectId>,
    pub name: String,
    pub color: ProjectColor,
}

/// A row in the list: a task, or the composer where it belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListItem {
    Task(TaskRow),
    Composer { mode: Composer, depth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// Distinguishes toasts, so a timer only dismisses the one it was started for.
    pub id: u64,
    pub message: String,
    pub undo: Option<Undo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Reopen(Vec<TaskId>),
}

/// A system notification to schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNotification {
    pub identifier: String,
    pub task: TaskId,
    pub fire_at: Timestamp,
    pub title: String,
    pub body: String,
}

/// Backups: where they go and how the last one went.
#[derive(Debug, Clone, Default)]
pub struct BackupState {
    /// `None` while backups are off.
    pub folder: Option<BackupFolder>,
    pub policy: BackupPolicy,
    /// Why the last backup failed, cleared by the next one that succeeds.
    pub error: Option<String>,
    /// Where the folder choice is remembered; `None` when backups can't be turned on (a test database).
    settings_path: Option<PathBuf>,
}

impl BackupState {
    pub fn available(&self) -> bool {
        self.settings_path.is_some()
    }
}

/// A task moved to another project from the overview, and where it was in the list before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kept {
    task: TaskId,
    parent: Option<TaskId>,
    position: i64,
}

pub struct AppState {
    repo: Repository,
    snapshot: Snapshot,
    rows: Vec<TaskRow>,
    clock: Box<dyn Fn() -> ZonedDateTime>,

    pub project: ProjectId,
    pub show_completed: bool,
    pub selection: Selection,
    pub composer: Composer,
    /// The task whose overview is open.
    pub overview: Option<TaskId>,
    /// Tasks moved to another project from the overview, as in Todoist. They and their subtasks keep their
    /// place in the list until the overview closes, so the list doesn't shift under it and stepping away and
    /// back finds them where they were.
    kept: Vec<Kept>,
    pub dialog: Dialog,
    pub toast: Option<Toast>,
    /// The last thing that went wrong, for the error banner.
    pub error: Option<String>,
    pub backup: BackupState,
    next_toast: u64,
}

impl AppState {
    /// State over `repo`. Backups are configurable only with a `settings_path` to remember the folder in.
    pub fn new(repo: Repository, settings_path: Option<PathBuf>) -> Self {
        AppState::with_clock(repo, settings_path, ZonedDateTime::now)
    }

    pub fn with_clock(
        repo: Repository,
        settings_path: Option<PathBuf>,
        clock: impl Fn() -> ZonedDateTime + 'static,
    ) -> Self {
        let mut backup = BackupState {
            settings_path,
            ..Default::default()
        };
        if let Some(path) = &backup.settings_path
            && let Some(folder) = Settings::load(path).backup_folder
        {
            let folder = BackupFolder::new(folder);
            backup.policy.last_backup =
                folder.backups().first().and_then(|b| timestamp(b.modified));
            backup.folder = Some(folder);
        }
        let mut state = AppState {
            repo,
            snapshot: Snapshot::default(),
            rows: Vec::new(),
            clock: Box::new(clock),
            project: ProjectId(0),
            show_completed: false,
            selection: Selection::Empty,
            composer: Composer::Closed,
            overview: None,
            kept: Vec::new(),
            dialog: Dialog::None,
            toast: None,
            error: None,
            backup,
            next_toast: 0,
        };
        state.reload();
        state
    }

    pub fn now(&self) -> ZonedDateTime {
        (self.clock)()
    }

    pub fn repository(&self) -> &Repository {
        &self.repo
    }

    // Reading

    pub fn projects(&self) -> &[Project] {
        &self.snapshot.projects
    }

    pub fn tasks(&self) -> &[Task] {
        &self.snapshot.tasks
    }

    pub fn project_by_id(&self, id: ProjectId) -> Option<&Project> {
        self.snapshot.project(id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.snapshot.task(id)
    }

    pub fn current_project(&self) -> Option<&Project> {
        self.project_by_id(self.project)
    }

    pub fn open_count(&self, project: ProjectId) -> usize {
        tree::open_count(&self.snapshot.tasks, project)
    }

    /// The current project's outline.
    pub fn rows(&self) -> &[TaskRow] {
        &self.rows
    }

    fn row_index(&self, id: TaskId) -> Option<usize> {
        self.rows.iter().position(|r| r.id() == id)
    }

    pub fn is_in_list(&self, id: TaskId) -> bool {
        self.row_index(id).is_some()
    }

    /// The rows with the composer spliced in where it belongs.
    pub fn list_items(&self) -> Vec<ListItem> {
        let mut items: Vec<ListItem> = self.rows.iter().cloned().map(ListItem::Task).collect();
        match self.composer {
            Composer::Closed | Composer::AddingInOverview { .. } => {}
            Composer::Adding { parent: None } => items.push(ListItem::Composer {
                mode: self.composer,
                depth: 0,
            }),
            Composer::Adding {
                parent: Some(parent),
            } => {
                if let Some(index) = self.row_index(parent) {
                    let depth = self.rows[index].depth;
                    let end = self.rows[index + 1..]
                        .iter()
                        .position(|r| r.depth <= depth)
                        .map_or(self.rows.len(), |offset| index + 1 + offset);
                    items.insert(
                        end,
                        ListItem::Composer {
                            mode: self.composer,
                            depth: depth + 1,
                        },
                    );
                }
            }
            Composer::Editing { task } => {
                if let Some(index) = self.row_index(task) {
                    items[index] = ListItem::Composer {
                        mode: self.composer,
                        depth: self.rows[index].depth,
                    };
                }
            }
        }
        items
    }

    /// A task's direct subtasks, leaving out completed ones unless they're being shown.
    pub fn subtasks(&self, id: TaskId) -> Vec<&Task> {
        tree::children(&self.snapshot.tasks, id)
            .into_iter()
            .filter(|t| self.show_completed || !t.is_completed())
            .collect()
    }

    pub fn ancestors(&self, id: TaskId) -> Vec<TaskId> {
        tree::ancestors(&self.snapshot.tasks, id)
    }

    /// The tasks a command on `id` acts on, in outline order: the whole selection when `id` is part of it (or
    /// is `None`), otherwise just that task. Subtasks of included tasks are left out, as their parent takes
    /// them along.
    pub fn acting_on(&self, id: Option<TaskId>) -> Vec<TaskId> {
        if let Some(id) = id.filter(|&id| !self.selection.contains(id)) {
            return vec![id];
        }
        let ids: Vec<TaskId> = self
            .rows
            .iter()
            .map(TaskRow::id)
            .filter(|&id| self.selection.contains(id))
            .collect();
        self.topmost(&ids)
    }

    fn topmost(&self, ids: &[TaskId]) -> Vec<TaskId> {
        ids.iter()
            .copied()
            .filter(|&id| !self.ancestors(id).iter().any(|a| ids.contains(a)))
            .collect()
    }

    // Navigation

    pub fn select_project(&mut self, id: ProjectId) {
        if id == self.project || self.project_by_id(id).is_none() {
            return;
        }
        self.project = id;
        self.selection = Selection::Empty;
        self.composer = Composer::Closed;
        self.overview = None;
        self.refresh_rows();
    }

    /// ⌘1…9: the Inbox, then projects in sidebar order.
    pub fn select_project_at(&mut self, index: usize) {
        if let Some(project) = self.snapshot.projects.get(index) {
            self.select_project(project.id);
        }
    }

    pub fn set_show_completed(&mut self, show: bool) {
        self.show_completed = show;
        self.reconcile();
    }

    pub fn select_only(&mut self, id: TaskId) {
        if self.is_in_list(id) {
            self.selection = Selection::single(id);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = Selection::Empty;
    }

    pub fn move_selection(&mut self, offset: isize) {
        if self.rows.is_empty() {
            return;
        }
        let next = match self.selection.anchor().and_then(|a| self.row_index(a)) {
            None if offset > 0 => 0,
            None => self.rows.len() - 1,
            Some(current) => current
                .saturating_add_signed(offset)
                .min(self.rows.len() - 1),
        };
        self.selection = Selection::single(self.rows[next].id());
    }

    /// ⌘-click: adds a task to the selection, or takes it out. Either way a Shift range being grown ends.
    pub fn toggle_selection(&mut self, id: TaskId) {
        let mut ids = self.selection.ids();
        let mut anchor = self.selection.anchor();
        if ids.insert(id) {
            anchor = Some(id);
        } else {
            ids.remove(&id);
            if anchor == Some(id) {
                // Hand the keyboard to the nearest task still selected.
                let index = self.row_index(id).unwrap_or(0);
                let after = self.rows[index..].iter().find(|r| ids.contains(&r.id()));
                let before = self.rows[..index]
                    .iter()
                    .rev()
                    .find(|r| ids.contains(&r.id()));
                anchor = after.or(before).map(TaskRow::id);
            }
        }
        self.selection = match anchor {
            Some(anchor) if !ids.is_empty() => Selection::Tasks {
                anchor,
                ids,
                range_end: None,
            },
            _ => Selection::Empty,
        };
    }

    /// ⇧-click: selects every task from the anchor to `id`.
    pub fn extend_selection_to(&mut self, id: TaskId) {
        let (Some(anchor), Some(end)) = (self.selection.anchor(), self.row_index(id)) else {
            self.select_only(id);
            return;
        };
        let Some(start) = self.row_index(anchor) else {
            self.select_only(id);
            return;
        };
        let ids = self.rows[start.min(end)..=start.max(end)]
            .iter()
            .map(TaskRow::id)
            .collect();
        self.selection = Selection::Tasks {
            anchor,
            ids,
            range_end: Some(id),
        };
    }

    /// ⇧-arrow: grows or shrinks the selected range by a row.
    pub fn extend_selection_by(&mut self, offset: isize) {
        let end = match &self.selection {
            Selection::Empty => None,
            Selection::Tasks {
                anchor, range_end, ..
            } => Some(range_end.unwrap_or(*anchor)),
        };
        let Some(current) = end.and_then(|e| self.row_index(e)) else {
            self.move_selection(offset);
            return;
        };
        let next = current
            .saturating_add_signed(offset)
            .min(self.rows.len() - 1);
        self.extend_selection_to(self.rows[next].id());
    }

    // Composer

    pub fn start_adding(&mut self) {
        self.close_overview();
        self.composer = Composer::Adding { parent: None };
    }

    pub fn start_adding_subtask(&mut self, to: Option<TaskId>) {
        let Some(parent) = to
            .or(self.selection.anchor())
            .filter(|&id| self.is_in_list(id))
        else {
            return;
        };
        if self.task(parent).is_some_and(|t| t.collapsed)
            && !self.perform(|repo| repo.set_collapsed(parent, false))
        {
            return;
        }
        // A task kept in the list for the overview leaves it as the overview closes.
        self.close_overview();
        if !self.is_in_list(parent) {
            return;
        }
        self.composer = Composer::Adding {
            parent: Some(parent),
        };
    }

    /// Opens the composer in the overview, to add a subtask to its task.
    pub fn start_adding_subtask_in_overview(&mut self) {
        if let Some(parent) = self.overview {
            self.composer = Composer::AddingInOverview { parent };
        }
    }

    pub fn start_editing(&mut self, id: Option<TaskId>) {
        let Some(id) = id
            .or(self.selection.anchor())
            .filter(|&id| self.is_in_list(id))
        else {
            return;
        };
        self.close_overview();
        if !self.is_in_list(id) {
            return;
        }
        self.selection = Selection::single(id);
        self.composer = Composer::Editing { task: id };
    }

    pub fn close_composer(&mut self) {
        self.composer = Composer::Closed;
    }

    pub fn date_parser(&self) -> DateParser {
        DateParser::new(self.now())
    }

    /// What the draft would save, counting words not yet turned into blocks. A block for a project deleted
    /// since it was typed counts as its words.
    pub fn composer_result(&self, draft: &Draft) -> QuickAddResult {
        let dates = self.date_parser();
        let mut draft = draft.clone();
        draft.convert(&self.snapshot.projects, Some(&dates), true);
        draft.revert_blocks(
            |value| matches!(value, BlockValue::Project(id) if self.project_by_id(*id).is_none()),
        );
        draft.result(&dates)
    }

    /// Saves the composer. A project block overrides `project`, a date block overrides `due` (as picked with
    /// the chip), and reminder blocks add to `reminders`. Returns whether it saved.
    pub fn submit_composer(
        &mut self,
        draft: &Draft,
        description: &str,
        project: ProjectId,
        due: Option<Due>,
        reminders: &[Reminder],
    ) -> bool {
        let parsed = self.composer_result(draft);
        if parsed.name.is_empty() {
            return false;
        }
        let target = parsed.project_id.unwrap_or(project);
        let due = parsed.due.or(due);
        let mut reminders = reminders.to_vec();
        reminders.extend(parsed.reminders);
        let description = description.trim().to_owned();
        match self.composer {
            Composer::Closed => false,
            Composer::Adding { .. } | Composer::AddingInOverview { .. } => {
                // A subtask stays with its parent unless the text names another project.
                let parent = self
                    .composer
                    .parent()
                    .and_then(|p| self.task(p))
                    .filter(|t| t.project_id == target)
                    .map(|t| (t.id, t.collapsed));
                let new = NewTask {
                    name: parsed.name,
                    description,
                    project_id: Some(target),
                    parent_id: parent.map(|(id, _)| id),
                    due,
                    reminders,
                };
                let added = self.perform(|repo| {
                    // The new subtask shows in the list, as in Todoist.
                    if let Some((id, true)) = parent {
                        repo.set_collapsed(id, false)?;
                    }
                    repo.add_task(new).map(drop)
                });
                if !added {
                    return false;
                }
                if target != self.project {
                    let name = self
                        .project_by_id(target)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.show_toast(format!("Task added to {name}"), None);
                }
                true
            }
            Composer::Editing { task } => {
                let saved = self.perform(|repo| {
                    repo.update_task(task, &parsed.name, &description)?;
                    repo.set_due(task, due)?;
                    repo.set_reminders(task, &reminders)?;
                    repo.move_to_project(task, target)
                });
                if !saved {
                    return false;
                }
                self.composer = Composer::Closed;
                if target != self.project {
                    let name = self
                        .project_by_id(target)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.selection = Selection::Empty;
                    self.show_toast(format!("Task moved to {name}"), None);
                }
                true
            }
        }
    }

    // Task overview

    pub fn open_overview(&mut self, id: TaskId) {
        if self.task(id).is_none() {
            return;
        }
        self.composer = Composer::Closed;
        // A subtask hidden in the list would hand the selection to its parent; leave the selection be.
        if self.is_in_list(id) {
            self.selection = Selection::single(id);
        }
        self.overview = Some(id);
    }

    /// Closes the overview. Tasks it kept in the list leave it now, handing on the selection as a move would.
    pub fn close_overview(&mut self) {
        self.overview = None;
        if matches!(self.composer, Composer::AddingInOverview { .. }) {
            self.composer = Composer::Closed;
        }
        if self.kept.is_empty() {
            return;
        }
        let gone: Vec<TaskId> = self
            .kept
            .iter()
            .map(|k| k.task)
            .filter(|&id| self.task(id).is_some_and(|t| t.project_id != self.project))
            .collect();
        let fallback = self.selection_after_removing(&gone);
        self.kept.clear();
        if let Some(fallback) = fallback {
            self.selection = fallback.map_or(Selection::Empty, Selection::single);
        }
        self.reconcile();
    }

    /// The task `offset` rows from the open one, which the overview's arrows step to.
    pub fn overview_neighbour(&self, offset: isize) -> Option<TaskId> {
        let index = self.row_index(self.overview?)?;
        self.rows
            .get(index.checked_add_signed(offset)?)
            .map(TaskRow::id)
    }

    /// Saves the overview's name and description. As in the composer, a project, date or reminder written in
    /// the name is set and taken out of it: a project moves the task (keeping it in the list), a date replaces
    /// the due date and reminders add to the task's. An empty name keeps the old one. Returns whether it saved.
    pub fn save_overview(&mut self, id: TaskId, name: &Draft, description: &str) -> bool {
        let parsed = self.composer_result(name);
        let Some(task) = self.task(id).cloned() else {
            return false;
        };
        let name = Some(parsed.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(&task.name);
        let description = description.trim();
        let mut reminders = task.reminders.clone();
        reminders.extend(parsed.reminders.iter().copied());
        let text_changed = name != task.name || description != task.description;
        let due_changed = parsed.due.is_some_and(|due| task.due != Some(due));
        let reminders_changed = !parsed.reminders.is_empty();
        if (text_changed || due_changed || reminders_changed)
            && !self.perform(|repo| {
                if text_changed {
                    repo.update_task(id, name, description)?;
                }
                if due_changed {
                    repo.set_due(id, parsed.due)?;
                }
                if reminders_changed {
                    repo.set_reminders(id, &reminders)?;
                }
                Ok(())
            })
        {
            return false;
        }
        match parsed.project_id.filter(|&p| p != task.project_id) {
            Some(project) => self.move_task_to(id, project),
            None => true,
        }
    }

    /// Reschedules this one task, unlike [`AppState::set_due`], which acts on the selection it belongs to.
    pub fn set_due_of_only(&mut self, id: TaskId, due: Option<Due>) {
        self.perform(|repo| repo.set_due(id, due));
    }

    pub fn set_reminders(&mut self, id: TaskId, reminders: &[Reminder]) {
        self.perform(|repo| repo.set_reminders(id, reminders));
    }

    /// Moves the overview's task. It keeps its place in the list until the overview closes.
    pub fn move_task_to(&mut self, id: TaskId, project: ProjectId) -> bool {
        self.move_tasks(&[id], project, self.overview.is_some())
    }

    // Task actions

    /// Completes the task, or the selection it belongs to; reopens them if they are all done already.
    pub fn toggle_completed(&mut self, id: Option<TaskId>) {
        let ids = self.acting_on(id);
        self.toggle_completed_of(&ids);
    }

    pub fn toggle_completed_of(&mut self, ids: &[TaskId]) {
        let tasks: Vec<&Task> = ids.iter().filter_map(|&id| self.task(id)).collect();
        if tasks.is_empty() {
            return;
        }
        let completing = tasks.iter().any(|t| !t.is_completed());
        let changed: Vec<TaskId> = tasks
            .iter()
            .filter(|t| t.is_completed() != completing)
            .map(|t| t.id)
            .collect();
        // Undo reopens exactly what completing closed: the tasks and their subtasks that were open.
        let reopen: Vec<TaskId> = self
            .snapshot
            .tasks
            .iter()
            .filter(|t| !t.is_completed())
            .filter(|t| {
                changed.contains(&t.id) || self.ancestors(t.id).iter().any(|a| changed.contains(a))
            })
            .map(|t| t.id)
            .collect();
        let fallback = if completing && !self.show_completed {
            self.selection_after_removing(&changed)
        } else {
            None
        };
        if !self.perform(|repo| {
            changed
                .iter()
                .try_for_each(|&id| repo.set_completed(id, completing))
        }) {
            return;
        }
        if let Some(fallback) = fallback {
            self.selection = fallback.map_or(Selection::Empty, Selection::single);
            self.reconcile();
        }
        if completing {
            let message = match changed.len() {
                1 => "1 task completed".to_owned(),
                n => format!("{n} tasks completed"),
            };
            self.show_toast(message, Some(Undo::Reopen(reopen)));
        }
    }

    /// Reschedules a task, or the selection it belongs to, keeping each one's time of day unless `due` brings
    /// its own.
    pub fn set_due(&mut self, id: Option<TaskId>, due: Option<Due>) {
        let changes: Vec<(TaskId, Option<Due>)> = self
            .acting_on(id)
            .into_iter()
            .map(|id| {
                let time = self.task(id).and_then(|t| t.due).and_then(|d| d.time);
                (
                    id,
                    due.map(|d| if d.has_time() { d } else { d.with_time(time) }),
                )
            })
            .collect();
        self.perform(|repo| {
            changes
                .iter()
                .try_for_each(|&(id, due)| repo.set_due(id, due))
        });
    }

    /// Shows a task, as when its notification is clicked: switches to its project and expands its ancestors.
    pub fn reveal(&mut self, id: TaskId) {
        let Some(task) = self.task(id).cloned() else {
            return;
        };
        self.select_project(task.project_id);
        let collapsed: Vec<TaskId> = self
            .ancestors(id)
            .into_iter()
            .filter(|&a| self.task(a).is_some_and(|t| t.collapsed))
            .collect();
        if !collapsed.is_empty()
            && !self.perform(|repo| {
                collapsed
                    .iter()
                    .try_for_each(|&a| repo.set_collapsed(a, false))
            })
        {
            return;
        }
        if task.is_completed() {
            self.set_show_completed(true);
        }
        self.select_only(id);
    }

    /// A notification's Complete button.
    pub fn complete_from_notification(&mut self, id: TaskId) {
        if self.task(id).is_some_and(|t| !t.is_completed()) {
            let fallback = self.selection_after_removing(&[id]);
            if self.perform(|repo| repo.set_completed(id, true))
                && !self.show_completed
                && let Some(fallback) = fallback
            {
                self.selection = fallback.map_or(Selection::Empty, Selection::single);
                self.reconcile();
            }
        }
    }

    pub fn request_delete(&mut self, id: Option<TaskId>) {
        let ids = self.acting_on(id);
        if !ids.is_empty() {
            self.dialog = Dialog::ConfirmDeleteTasks(ids);
        }
    }

    pub fn indent(&mut self, id: Option<TaskId>) {
        if let Some(id) = id.or(self.selection.anchor()) {
            self.perform(|repo| repo.indent(id).map(drop));
        }
    }

    pub fn outdent(&mut self, id: Option<TaskId>) {
        if let Some(id) = id.or(self.selection.anchor()) {
            self.perform(|repo| repo.outdent(id).map(drop));
        }
    }

    pub fn move_by(&mut self, id: Option<TaskId>, offset: isize) {
        if let Some(id) = id.or(self.selection.anchor()) {
            self.perform(|repo| repo.move_by(id, offset).map(drop));
        }
    }

    /// Moves tasks, with their subtasks, to the end of another project, keeping their order. Returns `false`
    /// if they were all already there.
    pub fn move_to_project(&mut self, ids: &[TaskId], project: ProjectId) -> bool {
        self.move_tasks(ids, project, false)
    }

    /// [`AppState::move_to_project`], optionally keeping the tasks in the list for the open overview.
    fn move_tasks(&mut self, ids: &[TaskId], project: ProjectId, keep: bool) -> bool {
        let ids: Vec<TaskId> = self
            .topmost(ids)
            .into_iter()
            .filter(|&id| self.task(id).is_some_and(|t| t.project_id != project))
            .collect();
        let Some(name) = self.project_by_id(project).map(|p| p.name.clone()) else {
            return false;
        };
        if ids.is_empty() {
            return false;
        }
        let fallback = (!keep && project != self.project)
            .then(|| self.selection_after_removing(&ids))
            .flatten();
        // Recorded before the write, as the reload after it would otherwise close the overview; a failed write
        // leaves the tasks where these say they are, and they are dropped again.
        let kept_before = self.kept.len();
        if keep {
            let new: Vec<Kept> = ids
                .iter()
                .filter(|&&id| !self.kept.iter().any(|k| k.task == id))
                .filter_map(|&id| self.task(id))
                .filter(|t| self.listed_project(t) == self.project)
                .map(|t| Kept {
                    task: t.id,
                    parent: t.parent_id,
                    position: t.position,
                })
                .collect();
            self.kept.extend(new);
        }
        if !self.perform(|repo| {
            ids.iter()
                .try_for_each(|&id| repo.move_to_project(id, project))
        }) {
            self.kept.truncate(kept_before);
            self.refresh_rows();
            return false;
        }
        if let Some(fallback) = fallback {
            self.selection = fallback.map_or(Selection::Empty, Selection::single);
            self.reconcile();
        }
        let what = if ids.len() == 1 {
            "Task".to_owned()
        } else {
            format!("{} tasks", ids.len())
        };
        self.show_toast(format!("{what} moved to {name}"), None);
        true
    }

    pub fn set_collapsed(&mut self, id: TaskId, collapsed: bool) {
        self.perform(|repo| repo.set_collapsed(id, collapsed));
    }

    /// ←: collapse the selected task, or go to its parent if it is collapsed already or a leaf.
    pub fn collapse_or_select_parent(&mut self) {
        let Some(row) = self
            .selection
            .anchor()
            .and_then(|a| self.row_index(a))
            .map(|i| self.rows[i].clone())
        else {
            return;
        };
        if row.has_children() && !row.task.collapsed {
            self.set_collapsed(row.id(), true);
        } else if let Some(parent) = row.task.parent_id {
            self.select_only(parent);
        }
    }

    /// →: expand the selected task.
    pub fn expand_selected(&mut self) {
        if let Some(task) = self
            .selection
            .anchor()
            .and_then(|a| self.task(a))
            .filter(|t| t.collapsed)
        {
            let id = task.id;
            self.set_collapsed(id, false);
        }
    }

    // Toasts

    fn show_toast(&mut self, message: String, undo: Option<Undo>) {
        self.next_toast += 1;
        self.toast = Some(Toast {
            id: self.next_toast,
            message,
            undo,
        });
    }

    pub fn undo_toast(&mut self) {
        let Some(toast) = self.toast.take() else {
            return;
        };
        match toast.undo {
            Some(Undo::Reopen(ids)) => {
                self.perform(|repo| ids.iter().try_for_each(|&id| repo.set_completed(id, false)));
            }
            None => {}
        }
    }

    pub fn dismiss_toast(&mut self, id: u64) {
        if self.toast.as_ref().is_some_and(|t| t.id == id) {
            self.toast = None;
        }
    }

    // Dialogs and projects

    pub fn start_new_project(&mut self) {
        self.dialog = Dialog::ProjectEditor(ProjectDraft {
            project: None,
            name: String::new(),
            color: ProjectColor::Charcoal,
        });
    }

    pub fn start_editing_project(&mut self, id: ProjectId) {
        if let Some(project) = self.project_by_id(id).filter(|p| !p.is_inbox) {
            self.dialog = Dialog::ProjectEditor(ProjectDraft {
                project: Some(id),
                name: project.name.clone(),
                color: project.color,
            });
        }
    }

    pub fn request_delete_project(&mut self, id: ProjectId) {
        if self.project_by_id(id).is_some_and(|p| !p.is_inbox) {
            self.dialog = Dialog::ConfirmDeleteProject(id);
        }
    }

    /// A click on a task while the list's composer holds unsaved changes: asks before throwing them away.
    pub fn request_discard_to_open(&mut self, id: TaskId) {
        if self.composer.is_in_list() && self.is_in_list(id) {
            self.dialog = Dialog::ConfirmDiscard(id);
        }
    }

    pub fn cancel_dialog(&mut self) {
        self.dialog = Dialog::None;
    }

    /// Saves the project editor. Returns whether it saved; an empty name keeps the dialog open.
    pub fn save_project(&mut self, draft: &ProjectDraft) -> bool {
        if draft.name.trim().is_empty() {
            return false;
        }
        match draft.project {
            Some(id) => {
                if !self.perform(|repo| repo.update_project(id, &draft.name, draft.color)) {
                    return false;
                }
            }
            None => {
                let mut created = None;
                if !self.perform(|repo| {
                    created = Some(repo.create_project(&draft.name, draft.color)?.id);
                    Ok(())
                }) {
                    return false;
                }
                if let Some(id) = created {
                    self.select_project(id);
                }
            }
        }
        self.dialog = Dialog::None;
        true
    }

    /// Confirms the open deletion or discard dialog.
    pub fn confirm_dialog(&mut self) {
        match std::mem::take(&mut self.dialog) {
            Dialog::ConfirmDeleteTasks(ids) => {
                let fallback = self.selection_after_removing(&ids);
                if self.perform(|repo| ids.iter().try_for_each(|&id| repo.delete_task(id)))
                    && let Some(fallback) = fallback
                {
                    self.selection = fallback.map_or(Selection::Empty, Selection::single);
                    self.reconcile();
                }
            }
            Dialog::ConfirmDeleteProject(id) => {
                self.perform(|repo| repo.delete_project(id).map(drop));
            }
            Dialog::ConfirmDiscard(id) => {
                self.select_only(id);
                self.open_overview(id);
            }
            other => self.dialog = other,
        }
    }

    // Backups

    pub fn database_path(&self) -> Option<&Path> {
        self.repo.path()
    }

    /// Starts backing up to `folder`, with a backup straight away, or stops when it is `None`.
    pub fn set_backup_folder(&mut self, folder: Option<PathBuf>) {
        let Some(settings_path) = self.backup.settings_path.clone() else {
            return;
        };
        let mut settings = Settings::load(&settings_path);
        settings.backup_folder = folder.clone();
        if let Err(e) = settings.save(&settings_path) {
            self.error = Some(format!("Couldn't save settings: {e}"));
            return;
        }
        self.backup.folder = folder.map(BackupFolder::new);
        self.backup.policy = BackupPolicy {
            last_attempt: None,
            last_backup: self
                .backup
                .folder
                .as_ref()
                .and_then(|f| f.backups().first().and_then(|b| timestamp(b.modified))),
        };
        self.backup.error = None;
        self.back_up_now();
    }

    pub fn back_up_now(&mut self) {
        let Some(folder) = self.backup.folder.clone() else {
            return;
        };
        let now = self.now();
        self.backup.policy.last_attempt = Some(now.timestamp());
        match folder.back_up(&self.repo, &now) {
            Ok(_) => {
                self.backup.policy.last_backup = Some(now.timestamp());
                self.backup.error = None;
            }
            Err(e) => self.backup.error = Some(e.to_string()),
        }
    }

    /// Backs up if anything (here, or in the CLI or MCP server) has changed the database since the last
    /// backup. Called every second while running; `throttled` waits a minute between attempts.
    pub fn back_up_if_changed(&mut self, throttled: bool) {
        if self.backup.folder.is_none() {
            return;
        }
        let modified = self.repo.last_modified().and_then(timestamp);
        if self
            .backup
            .policy
            .is_due(self.now().timestamp(), modified, throttled)
        {
            self.back_up_now();
        }
    }

    /// Replaces everything with a backup's contents, first saving the current data beside the backups (or the
    /// database, if backups are off) so the restore can itself be undone.
    pub fn restore(&mut self, backup: &Path) {
        let now = self.now();
        let safety = self.backup.folder.clone().or_else(|| {
            self.repo
                .path()
                .and_then(Path::parent)
                .map(BackupFolder::new)
        });
        let result = (|| -> repo::Result<()> {
            if let Some(folder) = safety {
                folder.back_up_before(&self.repo, "restore", &now)?;
            }
            self.repo.restore_from(backup)
        })();
        self.reload();
        if let Err(e) = result {
            self.error = Some(e.to_string());
            return;
        }
        if let Some(inbox) = self.snapshot.inbox() {
            self.project = inbox.id;
        }
        self.selection = Selection::Empty;
        self.composer = Composer::Closed;
        self.overview = None;
        self.refresh_rows();
        let name = backup
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.show_toast(format!("Restored {name}"), None);
    }

    // Notifications

    /// The notifications to have scheduled now: each reminder, and the due time of timed tasks.
    pub fn notifications(&self) -> Vec<PendingNotification> {
        let now = self.now();
        schedule::upcoming(
            &self.snapshot.tasks,
            now.timestamp(),
            now.time_zone(),
            NOTIFICATION_LIMIT,
        )
        .into_iter()
        .filter_map(|scheduled| {
            let task = self.task(scheduled.task_id)?;
            let mut details = Vec::new();
            if let Some(due) = task.due {
                let at = ZonedDateTime::new(scheduled.fire_at, now.time_zone().clone());
                details.push(format!("Due {}", crate::format::due_label(due, &at)));
            }
            if let Some(project) = self.project_by_id(task.project_id) {
                details.push(project.name.clone());
            }
            Some(PendingNotification {
                identifier: scheduled.identifier(),
                task: task.id,
                fire_at: scheduled.fire_at,
                title: task.name.clone(),
                body: details.join(" · "),
            })
        })
        .collect()
    }

    // Reloading

    /// Picks up changes another process, such as the CLI, has written. Returns whether anything changed.
    pub fn reload_if_changed_elsewhere(&mut self) -> bool {
        match self.repo.has_external_changes() {
            Ok(true) => {
                self.reload();
                true
            }
            Ok(false) => false,
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    pub fn reload(&mut self) {
        match self.repo.snapshot() {
            Ok(snapshot) => self.snapshot = snapshot,
            Err(e) => self.error = Some(e.to_string()),
        }
        self.reconcile();
    }

    /// Brings everything that refers to a project or task back in line with what exists and is shown:
    ///
    /// - The project on screen exists, or the Inbox is shown instead.
    /// - Selected tasks are rows in the list; one that left it hands its place to the collapsed parent hiding
    ///   it, as in Finder, or leaves the selection.
    /// - The composer edits, or adds a subtask to, a task that is a row, or adds one to the overview's task.
    /// - The overview and dialogs refer to projects and tasks that still exist, and a discard dialog to a task
    ///   that is a row while the composer is still open in the list.
    pub fn reconcile(&mut self) {
        if self.current_project().is_none()
            && let Some(inbox) = self.snapshot.inbox()
        {
            self.project = inbox.id;
            self.selection = Selection::Empty;
            self.composer = Composer::Closed;
        }
        self.refresh_rows();

        let shown_instead = |state: &AppState, id: TaskId| -> Option<TaskId> {
            if state.is_in_list(id) {
                Some(id)
            } else {
                state
                    .ancestors(id)
                    .into_iter()
                    .find(|&a| state.is_in_list(a))
            }
        };
        if let Selection::Tasks {
            anchor,
            ids,
            range_end,
        } = &self.selection
        {
            let new_ids: BTreeSet<TaskId> = ids
                .iter()
                .filter_map(|&id| shown_instead(self, id))
                .collect();
            let new_end = range_end.and_then(|e| shown_instead(self, e));
            let new_anchor = shown_instead(self, *anchor).or_else(|| {
                self.rows
                    .iter()
                    .map(TaskRow::id)
                    .find(|id| new_ids.contains(id))
            });
            self.selection = match new_anchor {
                Some(anchor) => {
                    let mut ids = new_ids;
                    ids.insert(anchor);
                    Selection::Tasks {
                        anchor,
                        ids,
                        range_end: new_end,
                    }
                }
                None => Selection::Empty,
            };
        }

        match self.composer {
            Composer::Editing { task } | Composer::Adding { parent: Some(task) }
                if !self.is_in_list(task) =>
            {
                // Open, it would be hidden yet hold the keyboard, or add a subtask no one can see.
                self.composer = Composer::Closed;
            }
            Composer::AddingInOverview { parent } if self.overview != Some(parent) => {
                self.composer = Composer::Closed;
            }
            _ => {}
        }

        self.kept.retain(|k| self.snapshot.task(k.task).is_some());
        if self.overview.is_some_and(|id| {
            self.task(id)
                .is_none_or(|t| self.listed_project(t) != self.project)
        }) {
            self.close_overview();
        }

        let dialog_is_stale = match &self.dialog {
            Dialog::None => false,
            Dialog::ProjectEditor(draft) => draft
                .project
                .is_some_and(|id| self.project_by_id(id).is_none()),
            Dialog::ConfirmDeleteProject(id) => self.project_by_id(*id).is_none(),
            Dialog::ConfirmDiscard(id) => !self.composer.is_in_list() || !self.is_in_list(*id),
            Dialog::ConfirmDeleteTasks(ids) => {
                let remaining: Vec<TaskId> = ids
                    .iter()
                    .copied()
                    .filter(|&id| self.task(id).is_some())
                    .collect();
                if remaining.is_empty() {
                    true
                } else {
                    self.dialog = Dialog::ConfirmDeleteTasks(remaining);
                    false
                }
            }
        };
        if dialog_is_stale {
            self.dialog = Dialog::None;
        }
    }

    fn refresh_rows(&mut self) {
        if self.overview.is_none() {
            self.kept.clear();
        }
        self.rows = if self.kept.is_empty() {
            tree::rows(&self.snapshot.tasks, self.project, self.show_completed)
        } else {
            let parents: HashMap<TaskId, Option<TaskId>> = self
                .snapshot
                .tasks
                .iter()
                .map(|t| (t.id, t.parent_id))
                .collect();
            let is_kept = |id: TaskId| self.kept.iter().any(|k| k.task == id);
            let in_kept_subtree = |mut id: TaskId| {
                // Bounded, in case a corrupt database has a cycle.
                for _ in 0..=parents.len() {
                    if is_kept(id) {
                        return true;
                    }
                    match parents.get(&id).copied().flatten() {
                        Some(parent) => id = parent,
                        None => return false,
                    }
                }
                false
            };
            let tasks: Vec<Task> = self
                .snapshot
                .tasks
                .iter()
                .map(|task| {
                    let mut task = task.clone();
                    if let Some(kept) = self.kept.iter().find(|k| k.task == task.id) {
                        task.parent_id = kept.parent;
                        task.position = kept.position;
                    }
                    if in_kept_subtree(task.id) {
                        task.project_id = self.project;
                    }
                    task
                })
                .collect();
            tree::rows(&tasks, self.project, self.show_completed)
        };
    }

    /// The project whose list shows `task`: the current one for a task the overview keeps, or a subtask of
    /// one, and otherwise its own.
    fn listed_project(&self, task: &Task) -> ProjectId {
        let kept = |id: TaskId| self.kept.iter().any(|k| k.task == id);
        if kept(task.id) || self.ancestors(task.id).into_iter().any(kept) {
            self.project
        } else {
            task.project_id
        }
    }

    /// Runs `change` as one transaction, then reloads. On failure shows why, and none of it was kept. Update
    /// the state's own fields only after this returns `true`.
    fn perform(&mut self, change: impl FnOnce(&Repository) -> repo::Result<()>) -> bool {
        let result = self.repo.transaction(|| change(&self.repo));
        // Our own writes don't bump data_version, so the next poll won't reload for them again.
        let _ = self.repo.has_external_changes();
        self.reload();
        match result {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    /// Before tasks disappear from the list: if any of them (or their subtasks) are selected, where the
    /// selection should go — the first row left after the anchor, or the last before it. `None` when the
    /// selection isn't affected.
    fn selection_after_removing(&self, ids: &[TaskId]) -> Option<Option<TaskId>> {
        let mut removed = BTreeSet::new();
        let mut removing_below: Option<usize> = None;
        for row in &self.rows {
            if removing_below.is_some_and(|depth| row.depth > depth) {
                removed.insert(row.id());
                continue;
            }
            removing_below = None;
            if ids.contains(&row.id()) {
                removed.insert(row.id());
                removing_below = Some(row.depth);
            }
        }
        if !self.selection.ids().iter().any(|id| removed.contains(id)) {
            return None;
        }
        let index = self.selection.anchor().and_then(|a| self.row_index(a))?;
        let after = self.rows[index..]
            .iter()
            .find(|r| !removed.contains(&r.id()));
        let before = self.rows[..index]
            .iter()
            .rev()
            .find(|r| !removed.contains(&r.id()));
        Some(after.or(before).map(TaskRow::id))
    }
}
