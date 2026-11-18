//! Ostracon's core: the data model, storage, quick-add parsing, the task tree, reminder schedule,
//! backups and the tool layer shared by the CLI and the MCP server. No UI.

pub mod backup;
pub mod dates;
pub mod locale;
pub mod model;
pub mod paths;
pub mod quickadd;
pub mod repo;
pub mod schedule;
pub mod settings;
pub mod tree;

pub use model::{Due, Project, ProjectColor, ProjectId, Reminder, Task, TaskId};
pub use repo::{NewTask, Repository, Snapshot};
