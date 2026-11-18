//! All persistence for projects and tasks, in one SQLite file shared by the app, the CLI and the MCP server.
//!
//! - WAL mode, so readers never block the writer, and a busy timeout so concurrent writers wait for each other
//!   instead of failing.
//! - Every change runs in a transaction. Transactions nest: the outermost is `BEGIN IMMEDIATE`, inner ones are
//!   savepoints, so a multi-step change is all-or-nothing however it is composed.
//! - [`Repository::has_external_changes`] reports commits made by other connections.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};

use crate::model::{Due, Project, ProjectColor, ProjectId, Reminder, Task, TaskId};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("couldn't read or write a file: {0}")]
    Io(#[from] std::io::Error),
    #[error("a name can't be empty")]
    EmptyName,
    #[error("no task with ID {0}")]
    NoSuchTask(TaskId),
    #[error("no project with ID {0}")]
    NoSuchProject(ProjectId),
    #[error("the Inbox can't be renamed, recoloured or deleted")]
    InboxIsFixed,
    #[error("task {0} is in a different project, so it can't be the parent")]
    ParentInOtherProject(TaskId),
    #[error("{0} isn't an Ostracon database")]
    NotADatabase(PathBuf),
    #[error("{0} was written by a newer version of Ostracon")]
    TooNew(PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Ordered schema migrations, tracked with `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE projects (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        color TEXT NOT NULL,
        position INTEGER NOT NULL,
        is_inbox INTEGER NOT NULL DEFAULT 0
    );
    CREATE UNIQUE INDEX projects_one_inbox ON projects(is_inbox) WHERE is_inbox = 1;
    CREATE TABLE tasks (
        id INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        parent_id INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        position INTEGER NOT NULL,
        completed_at INTEGER,
        collapsed INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        due TEXT
    );
    CREATE INDEX tasks_project ON tasks(project_id);
    CREATE INDEX tasks_parent ON tasks(parent_id);
    CREATE TABLE reminders (
        id INTEGER PRIMARY KEY,
        task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        fire_at INTEGER,
        minutes_before INTEGER,
        CHECK ((fire_at IS NULL) <> (minutes_before IS NULL))
    );
    CREATE INDEX reminders_task ON reminders(task_id);
"#];

const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Every project and task at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Inbox first, then sidebar order.
    pub projects: Vec<Project>,
    /// In sibling order.
    pub tasks: Vec<Task>,
}

impl Snapshot {
    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn inbox(&self) -> Option<&Project> {
        self.projects.iter().find(|p| p.is_inbox)
    }
}

/// What to create.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub name: String,
    pub description: String,
    pub project_id: Option<ProjectId>,
    pub parent_id: Option<TaskId>,
    pub due: Option<Due>,
    pub reminders: Vec<Reminder>,
}

pub struct Repository {
    conn: Connection,
    path: Option<PathBuf>,
    depth: Cell<u32>,
    data_version: Cell<i64>,
}

impl Repository {
    /// Opens (creating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Repository::with_connection(conn, Some(path.to_owned()))
    }

    /// Opens the database at [`crate::paths::database_location`].
    pub fn open_default() -> Result<Self> {
        Repository::open(crate::paths::database_location().path)
    }

    /// An empty database in memory, for tests.
    pub fn in_memory() -> Result<Self> {
        Repository::with_connection(Connection::open_in_memory()?, None)
    }

    fn with_connection(conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(10))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let repo = Repository {
            conn,
            path,
            depth: Cell::new(0),
            data_version: Cell::new(0),
        };
        migrate(&repo.conn)?;
        repo.transaction(|| repo.ensure_inbox())?;
        repo.data_version.set(repo.read_data_version()?);
        Ok(repo)
    }

    /// The database file, or `None` in memory.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Runs `change` atomically: everything it does is kept, or, if it fails, none of it is.
    ///
    /// Works with any error type that a repository error converts into, so callers such as the tools can roll
    /// back on their own errors.
    pub fn transaction<T, E: From<Error>>(
        &self,
        change: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let depth = self.depth.get();
        let begin = if depth == 0 {
            "BEGIN IMMEDIATE".to_owned()
        } else {
            format!("SAVEPOINT nested_{depth}")
        };
        self.conn
            .execute_batch(&begin)
            .map_err(|e| E::from(e.into()))?;
        self.depth.set(depth + 1);
        let result = change();
        self.depth.set(depth);
        let finish = |sql: String| self.conn.execute_batch(&sql);
        match result {
            Ok(value) => {
                let end = if depth == 0 {
                    "COMMIT".to_owned()
                } else {
                    format!("RELEASE nested_{depth}")
                };
                if let Err(error) = finish(end) {
                    if depth == 0 {
                        let _ = finish("ROLLBACK".to_owned());
                    }
                    return Err(Error::from(error).into());
                }
                Ok(value)
            }
            Err(error) => {
                let undo = if depth == 0 {
                    "ROLLBACK".to_owned()
                } else {
                    format!("ROLLBACK TO nested_{depth}; RELEASE nested_{depth}")
                };
                let _ = finish(undo);
                Err(error)
            }
        }
    }

    /// Reads with one consistent view of the database.
    fn read<T>(&self, query: impl FnOnce() -> Result<T>) -> Result<T> {
        if self.depth.get() > 0 {
            return query();
        }
        self.conn.execute_batch("BEGIN DEFERRED")?;
        self.depth.set(1);
        let result = query();
        self.depth.set(0);
        let _ = self.conn.execute_batch("COMMIT");
        result
    }

    /// Whether another connection, such as the CLI's, has committed since this was last asked.
    pub fn has_external_changes(&self) -> Result<bool> {
        let version = self.read_data_version()?;
        Ok(self.data_version.replace(version) != version)
    }

    fn read_data_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?)
    }

    /// When anything last wrote to the database file (commits land in the WAL until it is checkpointed).
    pub fn last_modified(&self) -> Option<std::time::SystemTime> {
        let path = self.path.as_ref()?;
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        [path.as_path(), wal.as_path()]
            .into_iter()
            .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .max()
    }

    // Reading

    pub fn snapshot(&self) -> Result<Snapshot> {
        self.read(|| {
            Ok(Snapshot {
                projects: self.projects()?,
                tasks: self.tasks()?,
            })
        })
    }

    /// All projects, Inbox first, then in sidebar order.
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT id, name, color, position, is_inbox FROM projects ORDER BY is_inbox DESC, position, id")?;
        let rows = statement.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn project(&self, id: ProjectId) -> Result<Project> {
        self.conn
            .query_row(
                "SELECT id, name, color, position, is_inbox FROM projects WHERE id = ?",
                [id.0],
                project_from_row,
            )
            .optional()?
            .ok_or(Error::NoSuchProject(id))
    }

    pub fn inbox(&self) -> Result<Project> {
        Ok(self.conn.query_row(
            "SELECT id, name, color, position, is_inbox FROM projects WHERE is_inbox = 1",
            [],
            project_from_row,
        )?)
    }

    /// Every task, in sibling order.
    pub fn tasks(&self) -> Result<Vec<Task>> {
        self.read(|| {
            let mut reminders = self.reminders_by_task()?;
            let mut statement = self
                .conn
                .prepare_cached(&format!("{TASK_COLUMNS} ORDER BY position, id"))?;
            let rows = statement.query_map([], task_from_row)?;
            let mut tasks: Vec<Task> = rows.collect::<rusqlite::Result<_>>()?;
            for task in &mut tasks {
                task.reminders = reminders.remove(&task.id).unwrap_or_default();
            }
            Ok(tasks)
        })
    }

    pub fn task(&self, id: TaskId) -> Result<Task> {
        self.read(|| {
            let mut task = self
                .conn
                .query_row(
                    &format!("{TASK_COLUMNS} WHERE id = ?"),
                    [id.0],
                    task_from_row,
                )
                .optional()?
                .ok_or(Error::NoSuchTask(id))?;
            task.reminders = self.reminders_of(id)?;
            Ok(task)
        })
    }

    // Projects

    pub fn create_project(&self, name: &str, color: ProjectColor) -> Result<Project> {
        let name = validated(name)?;
        self.transaction(|| {
            let position: i64 = self.conn.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM projects WHERE is_inbox = 0",
                [],
                |row| row.get(0),
            )?;
            self.conn.execute(
                "INSERT INTO projects (name, color, position) VALUES (?, ?, ?)",
                params![name, color.key(), position],
            )?;
            self.project(ProjectId(self.conn.last_insert_rowid()))
        })
    }

    pub fn update_project(&self, id: ProjectId, name: &str, color: ProjectColor) -> Result<()> {
        let name = validated(name)?;
        self.transaction(|| {
            if self.project(id)?.is_inbox {
                return Err(Error::InboxIsFixed);
            }
            self.conn.execute(
                "UPDATE projects SET name = ?, color = ? WHERE id = ?",
                params![name, color.key(), id.0],
            )?;
            Ok(())
        })
    }

    /// Deletes a project and all of its tasks. Returns how many tasks went with it.
    pub fn delete_project(&self, id: ProjectId) -> Result<usize> {
        self.transaction(|| {
            if self.project(id)?.is_inbox {
                return Err(Error::InboxIsFixed);
            }
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?",
                [id.0],
                |row| row.get(0),
            )?;
            self.conn
                .execute("DELETE FROM projects WHERE id = ?", [id.0])?;
            Ok(count as usize)
        })
    }

    // Tasks

    /// Adds a task at the end of its siblings. With a parent and no project, it goes in the parent's project;
    /// with neither, in the Inbox.
    pub fn add_task(&self, new: NewTask) -> Result<Task> {
        let name = validated(&new.name)?;
        self.transaction(|| {
            let parent = new.parent_id.map(|id| self.task(id)).transpose()?;
            let project_id = match (new.project_id, &parent) {
                (Some(id), _) => self.project(id)?.id,
                (None, Some(parent)) => parent.project_id,
                (None, None) => self.inbox()?.id,
            };
            if let Some(parent) = &parent
                && parent.project_id != project_id
            {
                return Err(Error::ParentInOtherProject(parent.id));
            }
            let parent_id = parent.map(|p| p.id);
            let position = self.next_position(project_id, parent_id)?;
            self.conn.execute(
                "INSERT INTO tasks (project_id, parent_id, name, description, position, created_at, due)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    project_id.0,
                    parent_id.map(|p| p.0),
                    name,
                    new.description.trim(),
                    position,
                    Timestamp::now().as_second(),
                    new.due.map(|d| d.storage_value()),
                ],
            )?;
            let id = TaskId(self.conn.last_insert_rowid());
            self.insert_reminders(id, &new.reminders)?;
            self.task(id)
        })
    }

    pub fn update_task(&self, id: TaskId, name: &str, description: &str) -> Result<()> {
        let name = validated(name)?;
        self.change_task(
            id,
            "UPDATE tasks SET name = ?, description = ? WHERE id = ?",
            params![name, description.trim(), id.0],
        )
    }

    pub fn set_due(&self, id: TaskId, due: Option<Due>) -> Result<()> {
        self.change_task(
            id,
            "UPDATE tasks SET due = ? WHERE id = ?",
            params![due.map(|d| d.storage_value()), id.0],
        )
    }

    /// Replaces all of a task's reminders.
    pub fn set_reminders(&self, id: TaskId, reminders: &[Reminder]) -> Result<()> {
        self.transaction(|| {
            self.task(id)?;
            self.conn
                .execute("DELETE FROM reminders WHERE task_id = ?", [id.0])?;
            self.insert_reminders(id, reminders)
        })
    }

    pub fn set_collapsed(&self, id: TaskId, collapsed: bool) -> Result<()> {
        self.change_task(
            id,
            "UPDATE tasks SET collapsed = ? WHERE id = ?",
            params![collapsed, id.0],
        )
    }

    /// Completing a task also completes its subtasks, as in Todoist. Reopening one also reopens any completed
    /// ancestors, so it becomes visible again.
    pub fn set_completed(&self, id: TaskId, completed: bool) -> Result<()> {
        self.transaction(|| {
            self.task(id)?;
            if completed {
                let now = Timestamp::now().as_second();
                for task in self.subtree(id)? {
                    self.conn.execute(
                        "UPDATE tasks SET completed_at = COALESCE(completed_at, ?) WHERE id = ?",
                        params![now, task.0],
                    )?;
                }
            } else {
                let mut current = Some(id);
                while let Some(task) = current {
                    self.conn.execute(
                        "UPDATE tasks SET completed_at = NULL WHERE id = ?",
                        [task.0],
                    )?;
                    current = self.parent_of(task)?;
                }
            }
            Ok(())
        })
    }

    /// Deletes a task and its subtasks.
    pub fn delete_task(&self, id: TaskId) -> Result<()> {
        self.change_task(id, "DELETE FROM tasks WHERE id = ?", [id.0])
    }

    /// Moves a task, with its subtasks, to the end of another project's top level.
    pub fn move_to_project(&self, id: TaskId, project_id: ProjectId) -> Result<()> {
        self.transaction(|| {
            let task = self.task(id)?;
            self.project(project_id)?;
            if task.project_id == project_id {
                return Ok(());
            }
            let position = self.next_position(project_id, None)?;
            self.conn.execute(
                "UPDATE tasks SET parent_id = NULL, position = ? WHERE id = ?",
                params![position, id.0],
            )?;
            for task in self.subtree(id)? {
                self.conn.execute(
                    "UPDATE tasks SET project_id = ? WHERE id = ?",
                    params![project_id.0, task.0],
                )?;
            }
            Ok(())
        })
    }

    /// Makes a task the last subtask of the open sibling directly above it, expanding that sibling.
    /// Returns `false` when there is no such sibling.
    pub fn indent(&self, id: TaskId) -> Result<bool> {
        self.transaction(|| {
            let task = self.task(id)?;
            let siblings = self.siblings(task.project_id, task.parent_id, true)?;
            let Some(index) = siblings.iter().position(|&s| s == id).filter(|&i| i > 0) else {
                return Ok(false);
            };
            let parent = siblings[index - 1];
            let position = self.next_position(task.project_id, Some(parent))?;
            self.conn.execute(
                "UPDATE tasks SET parent_id = ?, position = ? WHERE id = ?",
                params![parent.0, position, id.0],
            )?;
            self.conn
                .execute("UPDATE tasks SET collapsed = 0 WHERE id = ?", [parent.0])?;
            Ok(true)
        })
    }

    /// Moves a subtask up a level, directly after its former parent. Returns `false` for top-level tasks.
    pub fn outdent(&self, id: TaskId) -> Result<bool> {
        self.transaction(|| {
            let task = self.task(id)?;
            let Some(parent_id) = task.parent_id else {
                return Ok(false);
            };
            let parent = self.task(parent_id)?;
            let mut siblings = self.siblings(parent.project_id, parent.parent_id, false)?;
            let Some(index) = siblings.iter().position(|&s| s == parent_id) else {
                return Ok(false);
            };
            siblings.insert(index + 1, id);
            self.conn.execute(
                "UPDATE tasks SET parent_id = ? WHERE id = ?",
                params![parent.parent_id.map(|p| p.0), id.0],
            )?;
            self.renumber(&siblings)?;
            Ok(true)
        })
    }

    /// Swaps a task with the open sibling `offset` (-1 or 1) places away. Returns `false` if there is none.
    pub fn move_by(&self, id: TaskId, offset: isize) -> Result<bool> {
        self.transaction(|| {
            let task = self.task(id)?;
            let open = self.siblings(task.project_id, task.parent_id, true)?;
            let Some(index) = open.iter().position(|&s| s == id) else {
                return Ok(false);
            };
            let Some(&neighbour) = index.checked_add_signed(offset).and_then(|i| open.get(i))
            else {
                return Ok(false);
            };
            let mut all = self.siblings(task.project_id, task.parent_id, false)?;
            let from = all
                .iter()
                .position(|&s| s == id)
                .expect("task is among its siblings");
            let to = all
                .iter()
                .position(|&s| s == neighbour)
                .expect("neighbour is among its siblings");
            all.swap(from, to);
            self.renumber(&all)?;
            Ok(true)
        })
    }

    // Copies

    /// Writes a consistent, compacted copy of the database to a new file at `destination`.
    pub fn copy_to(&self, destination: &Path) -> Result<()> {
        if self.depth.get() > 0 {
            panic!("copy_to can't run inside a transaction");
        }
        self.conn
            .execute("VACUUM INTO ?", [destination.to_string_lossy()])?;
        Ok(())
    }

    /// Replaces every project and task with those in the Ostracon database at `source`, such as a backup, in
    /// one transaction. Other connections see it as an ordinary change.
    pub fn restore_from(&self, source: &Path) -> Result<()> {
        // Bring a copy of the source up to this version's schema, leaving the original untouched.
        let scratch = tempfile_path(source)?;
        {
            let original = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| Error::NotADatabase(source.to_owned()))?;
            let version: i64 = original
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(|_| Error::NotADatabase(source.to_owned()))?;
            let tables: i64 = original
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('projects', 'tasks', 'reminders')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| Error::NotADatabase(source.to_owned()))?;
            if tables != 3 || version < 1 {
                return Err(Error::NotADatabase(source.to_owned()));
            }
            if version > SCHEMA_VERSION {
                return Err(Error::TooNew(source.to_owned()));
            }
            original.execute("VACUUM INTO ?", [scratch.to_string_lossy()])?;
        }
        let result = (|| {
            migrate(&Connection::open(&scratch)?)?;
            self.conn
                .execute("ATTACH DATABASE ? AS source", [scratch.to_string_lossy()])?;
            let copied = self.transaction(|| {
                self.conn.execute_batch(
                    "DELETE FROM reminders; DELETE FROM tasks; DELETE FROM projects;
                     INSERT INTO projects SELECT * FROM source.projects;
                     INSERT INTO tasks SELECT * FROM source.tasks;
                     INSERT INTO reminders SELECT * FROM source.reminders;",
                )?;
                self.ensure_inbox()
            });
            self.conn.execute("DETACH DATABASE source", [])?;
            copied
        })();
        let _ = std::fs::remove_file(&scratch);
        result
    }

    // Helpers

    fn change_task(&self, id: TaskId, sql: &str, params: impl rusqlite::Params) -> Result<()> {
        self.transaction(|| {
            if self.conn.execute(sql, params)? == 0 {
                return Err(Error::NoSuchTask(id));
            }
            Ok(())
        })
    }

    fn ensure_inbox(&self) -> Result<()> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE is_inbox = 1",
            [],
            |r| r.get(0),
        )?;
        if count == 0 {
            self.conn.execute(
                "INSERT INTO projects (name, color, position, is_inbox) VALUES ('Inbox', ?, 0, 1)",
                [ProjectColor::Blue.key()],
            )?;
        }
        Ok(())
    }

    fn next_position(&self, project: ProjectId, parent: Option<TaskId>) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks WHERE project_id = ? AND parent_id IS ?",
            params![project.0, parent.map(|p| p.0)],
            |row| row.get(0),
        )?)
    }

    fn siblings(
        &self,
        project: ProjectId,
        parent: Option<TaskId>,
        open_only: bool,
    ) -> Result<Vec<TaskId>> {
        let sql = format!(
            "SELECT id FROM tasks WHERE project_id = ? AND parent_id IS ? {} ORDER BY position, id",
            if open_only {
                "AND completed_at IS NULL"
            } else {
                ""
            }
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(params![project.0, parent.map(|p| p.0)], |row| {
            Ok(TaskId(row.get(0)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn renumber(&self, ids: &[TaskId]) -> Result<()> {
        for (position, id) in ids.iter().enumerate() {
            self.conn.execute(
                "UPDATE tasks SET position = ? WHERE id = ?",
                params![position as i64, id.0],
            )?;
        }
        Ok(())
    }

    fn parent_of(&self, id: TaskId) -> Result<Option<TaskId>> {
        let parent: Option<Option<i64>> = self
            .conn
            .query_row("SELECT parent_id FROM tasks WHERE id = ?", [id.0], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(parent.flatten().map(TaskId))
    }

    /// The task and all of its descendants.
    fn subtree(&self, id: TaskId) -> Result<Vec<TaskId>> {
        let mut statement = self.conn.prepare_cached(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT ? UNION ALL SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent_id = subtree.id
             )
             SELECT id FROM subtree",
        )?;
        let rows = statement.query_map([id.0], |row| Ok(TaskId(row.get(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn insert_reminders(&self, id: TaskId, reminders: &[Reminder]) -> Result<()> {
        let mut reminders = reminders.to_vec();
        Reminder::sort(&mut reminders);
        for reminder in reminders {
            let (fire_at, minutes) = match reminder {
                Reminder::At(moment) => (Some(moment.as_second()), None),
                Reminder::Before { minutes } => (None, Some(minutes)),
            };
            self.conn.execute(
                "INSERT INTO reminders (task_id, fire_at, minutes_before) VALUES (?, ?, ?)",
                params![id.0, fire_at, minutes],
            )?;
        }
        Ok(())
    }

    fn reminders_of(&self, id: TaskId) -> Result<Vec<Reminder>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT fire_at, minutes_before FROM reminders WHERE task_id = ?")?;
        let rows = statement.query_map([id.0], reminder_from_row)?;
        let mut reminders: Vec<Reminder> = rows
            .filter_map(|r| r.transpose())
            .collect::<rusqlite::Result<_>>()?;
        Reminder::sort(&mut reminders);
        Ok(reminders)
    }

    fn reminders_by_task(&self) -> Result<std::collections::HashMap<TaskId, Vec<Reminder>>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT task_id, fire_at, minutes_before FROM reminders")?;
        let mut rows = statement.query([])?;
        let mut map: std::collections::HashMap<TaskId, Vec<Reminder>> = Default::default();
        while let Some(row) = rows.next()? {
            let task = TaskId(row.get(0)?);
            let fire_at: Option<i64> = row.get(1)?;
            let minutes: Option<i64> = row.get(2)?;
            let reminder = match (fire_at, minutes) {
                (Some(at), _) => Timestamp::from_second(at).ok().map(Reminder::At),
                (None, Some(minutes)) => Some(Reminder::Before { minutes }),
                (None, None) => None,
            };
            if let Some(reminder) = reminder {
                map.entry(task).or_default().push(reminder);
            }
        }
        for reminders in map.values_mut() {
            Reminder::sort(reminders);
        }
        Ok(map)
    }
}

const TASK_COLUMNS: &str = "SELECT id, project_id, parent_id, name, description, position, completed_at, collapsed, created_at, due FROM tasks";

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(Error::TooNew(
            conn.path().map(PathBuf::from).unwrap_or_default(),
        ));
    }
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {migration}; PRAGMA user_version = {}; COMMIT;",
            index + 1
        ))
        .inspect_err(|_| {
            let _ = conn.execute_batch("ROLLBACK");
        })?;
    }
    Ok(())
}

fn project_from_row(row: &Row) -> rusqlite::Result<Project> {
    let color: String = row.get(2)?;
    Ok(Project {
        id: ProjectId(row.get(0)?),
        name: row.get(1)?,
        color: color.parse().unwrap_or(ProjectColor::Charcoal),
        position: row.get(3)?,
        is_inbox: row.get(4)?,
    })
}

fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
    let completed_at: Option<i64> = row.get(6)?;
    let created_at: i64 = row.get(8)?;
    let due: Option<String> = row.get(9)?;
    Ok(Task {
        id: TaskId(row.get(0)?),
        project_id: ProjectId(row.get(1)?),
        parent_id: row.get::<_, Option<i64>>(2)?.map(TaskId),
        name: row.get(3)?,
        description: row.get(4)?,
        position: row.get(5)?,
        completed_at: completed_at.and_then(|s| Timestamp::from_second(s).ok()),
        collapsed: row.get(7)?,
        created_at: Timestamp::from_second(created_at).unwrap_or(Timestamp::UNIX_EPOCH),
        due: due.as_deref().and_then(Due::from_storage),
        reminders: Vec::new(),
    })
}

fn reminder_from_row(row: &Row) -> rusqlite::Result<Option<Reminder>> {
    let fire_at: Option<i64> = row.get(0)?;
    let minutes: Option<i64> = row.get(1)?;
    Ok(match (fire_at, minutes) {
        (Some(at), _) => Timestamp::from_second(at).ok().map(Reminder::At),
        (None, Some(minutes)) => Some(Reminder::Before { minutes }),
        (None, None) => None,
    })
}

fn validated(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        Err(Error::EmptyName)
    } else {
        Ok(name.to_owned())
    }
}

/// A path in the temporary folder, not yet taken, for a scratch copy of `near`.
fn tempfile_path(near: &Path) -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let stem = near
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ostracon");
    for attempt in 0..1000u32 {
        let candidate = dir.join(format!(
            "{stem}-{}-{attempt}.restore.sqlite",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(std::io::Error::other("couldn't find a free scratch file name").into())
}
