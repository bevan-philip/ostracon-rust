//! `ostracon`: the MCP tools as shell commands. Each command turns its arguments into one tool's, then prints
//! what the tool returns readably, or as the tool's own JSON with `--json`.

mod printer;

use clap::{Args, Parser, Subcommand};
use jiff::ZonedDateTime;
use ostracon_core::tools::{ToolError, Tools};
use ostracon_core::{ProjectColor, Repository};
use serde_json::{Map, Value, json};

pub use printer::Printer;

const AFTER_HELP: &str = "P is a project's ID or name. DATE is natural language such as \"tomorrow 5pm\", \"next fri\" \
or \"in 3 days\", or ISO \"2026-01-27\" / \"2026-01-27T17:00\". R is a reminder: \"30min\" before the due time, or \
\"tomorrow 9am\". Single-quote TEXT so the shell leaves # and ! alone.

Set OSTRACON_DATABASE=/path.sqlite to use a database other than the app's.";

#[derive(Debug, Parser)]
#[command(name = "ostracon", version, about = "Ostracon's tasks from the shell", after_help = AFTER_HELP)]
pub struct Cli {
    /// Print the tool's JSON output instead of text
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a task, by default to the Inbox. TEXT may use quick-add syntax: 'Pay rent tomorrow 9am #Home !1h'
    Add {
        #[arg(required = true, num_args = 1.., allow_hyphen_values = false)]
        text: Vec<String>,
        #[arg(short, long, value_name = "P")]
        project: Option<String>,
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        #[arg(short, long, value_name = "DATE")]
        due: Option<String>,
        /// May be given more than once
        #[arg(short = 'r', long = "remind", value_name = "R")]
        reminders: Vec<String>,
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// Keep TEXT exactly as typed, without reading #project, dates or !reminders out of it
        #[arg(long)]
        literal: bool,
    },
    /// List open tasks in outline order
    #[command(alias = "ls")]
    List {
        #[arg(short, long, value_name = "P")]
        project: Option<String>,
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Only tasks due on or before DATE
        #[arg(long, value_name = "DATE", conflicts_with = "today")]
        due_by: Option<String>,
        /// Only tasks due today or overdue
        #[arg(long)]
        today: bool,
        /// Include completed tasks
        #[arg(short, long)]
        all: bool,
    },
    /// Show a task with its description, reminders and subtasks
    Show { id: String },
    /// Change a task; --remind replaces every reminder, and moving project moves its subtasks too
    Edit {
        id: String,
        #[arg(long, value_name = "TEXT")]
        name: Option<String>,
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        #[arg(short, long, value_name = "DATE", conflicts_with = "no_due")]
        due: Option<String>,
        #[arg(long)]
        no_due: bool,
        #[arg(
            short = 'r',
            long = "remind",
            value_name = "R",
            conflicts_with = "no_reminders"
        )]
        reminders: Vec<String>,
        #[arg(long)]
        no_reminders: bool,
        #[arg(short, long, value_name = "P")]
        project: Option<String>,
    },
    /// Complete tasks along with their subtasks
    #[command(alias = "complete")]
    Done {
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Reopen tasks, and any completed parents
    Reopen {
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Delete a task and its subtasks
    #[command(alias = "rm")]
    Delete {
        id: String,
        /// Don't ask first
        #[arg(short, long)]
        yes: bool,
    },
    /// List projects with their IDs and open task counts
    Projects,
    /// Add, edit or delete a project
    #[command(subcommand)]
    Project(ProjectCommand),
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    Add {
        #[arg(required = true)]
        name: Vec<String>,
        #[command(flatten)]
        color: ColorArg,
    },
    Edit {
        #[arg(required = true)]
        project: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        color: ColorArg,
    },
    #[command(alias = "rm")]
    Delete {
        #[arg(required = true)]
        project: Vec<String>,
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
struct ColorArg {
    /// One of the Todoist colours, such as "lime green"
    #[arg(long, value_name = "C")]
    color: Option<String>,
}

/// Why a command failed, and the exit status to report it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    pub status: u8,
}

impl CliError {
    /// A mistake in how the command was typed (EX_USAGE).
    fn usage(message: impl Into<String>) -> Self {
        CliError {
            message: message.into(),
            status: 64,
        }
    }
}

impl From<ToolError> for CliError {
    fn from(error: ToolError) -> Self {
        CliError {
            message: error.0,
            status: 1,
        }
    }
}

/// Asks a yes/no question before deleting anything. `None` when there's no one to ask.
pub type Confirm<'a> = Option<&'a mut dyn FnMut(&str) -> bool>;

pub struct Runner<'a> {
    tools: Tools<'a>,
    now: Box<dyn Fn() -> ZonedDateTime + 'a>,
}

impl<'a> Runner<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Runner {
            tools: Tools::new(repo),
            now: Box::new(ZonedDateTime::now),
        }
    }

    pub fn with_clock(repo: &'a Repository, now: impl Fn() -> ZonedDateTime + Clone + 'a) -> Self {
        Runner {
            tools: Tools::with_clock(repo, now.clone()),
            now: Box::new(now),
        }
    }

    /// Runs one parsed command line, returning what to print.
    pub fn run(&self, cli: Cli, confirm: Confirm) -> Result<String, CliError> {
        let printer = Printer::new((self.now)());
        let json = cli.json;
        let (output, text) = self.perform(cli.command, confirm, &printer)?;
        Ok(if json {
            serde_json::to_string_pretty(&output).unwrap_or_default()
        } else {
            text
        })
    }

    fn call(&self, name: &str, args: Value) -> Result<Value, CliError> {
        Ok(self.tools.call(name, &args)?)
    }

    fn perform(
        &self,
        command: Command,
        confirm: Confirm,
        printer: &Printer,
    ) -> Result<(Value, String), CliError> {
        match command {
            Command::Projects => {
                let output = self.call("list_projects", json!({}))?;
                let text = printer.projects(&output);
                Ok((output, text))
            }
            Command::List {
                project,
                search,
                due_by,
                today,
                all,
            } => {
                let mut args = Map::new();
                args.insert("include_completed".into(), json!(all));
                let grouped = project.is_none();
                if let Some(project) = project {
                    args.insert("project_id".into(), self.project_id(&project)?);
                }
                if let Some(search) = search {
                    args.insert("search".into(), json!(search));
                }
                if let Some(due_by) = if today {
                    Some("today".to_owned())
                } else {
                    due_by
                } {
                    args.insert("due_by".into(), json!(due_by));
                }
                let output = self.call("list_tasks", Value::Object(args))?;
                let text = printer.tasks(&output, grouped);
                Ok((output, text))
            }
            Command::Show { id } => {
                let output = self.call("get_task", json!({ "id": task_id(&id)? }))?;
                let text = printer.detail(&output);
                Ok((output, text))
            }
            Command::Add {
                text,
                project,
                parent,
                due,
                reminders,
                description,
                literal,
            } => {
                let text = text.join(" ");
                if text.trim().is_empty() {
                    return Err(CliError::usage("add needs the task's text."));
                }
                let mut args = Map::new();
                args.insert("name".into(), json!(text));
                args.insert("quick_add".into(), json!(!literal));
                if let Some(project) = project {
                    args.insert("project_id".into(), self.project_id(&project)?);
                }
                if let Some(parent) = parent {
                    args.insert("parent_id".into(), task_id(&parent)?);
                }
                if let Some(due) = due {
                    args.insert("due".into(), json!(due));
                }
                if !reminders.is_empty() {
                    args.insert("reminders".into(), json!(reminders));
                }
                if let Some(description) = description {
                    args.insert("description".into(), json!(description));
                }
                let output = self.call("add_task", Value::Object(args))?;
                let text = format!("Added {}", printer.line(&output, 0, 0, true));
                Ok((output, text))
            }
            Command::Edit {
                id,
                name,
                description,
                due,
                no_due,
                reminders,
                no_reminders,
                project,
            } => {
                let mut args = Map::new();
                args.insert("id".into(), task_id(&id)?);
                if let Some(name) = name {
                    args.insert("name".into(), json!(name));
                }
                if let Some(description) = description {
                    args.insert("description".into(), json!(description));
                }
                if let Some(due) = due {
                    args.insert("due".into(), json!(due));
                }
                if no_due {
                    args.insert("due".into(), Value::Null);
                }
                if !reminders.is_empty() {
                    args.insert("reminders".into(), json!(reminders));
                }
                if no_reminders {
                    args.insert("reminders".into(), json!([]));
                }
                if let Some(project) = project {
                    args.insert("project_id".into(), self.project_id(&project)?);
                }
                if args.len() == 1 {
                    return Err(CliError::usage(
                        "Nothing to change. See ostracon edit --help for the options.",
                    ));
                }
                let output = self.call("update_task", Value::Object(args))?;
                let text = format!("Updated {}", printer.line(&output, 0, 0, true));
                Ok((output, text))
            }
            Command::Done { ids } => self.complete(&ids, true, printer),
            Command::Reopen { ids } => self.complete(&ids, false, printer),
            Command::Delete { id, yes } => {
                let id = task_id(&id)?;
                let task = self.call("get_task", json!({ "id": id }))?;
                let subtasks = count_subtasks(&task);
                let what = format!(
                    "task {} \"{}\"{}",
                    task["id"],
                    task["name"].as_str().unwrap_or_default(),
                    match subtasks {
                        0 => String::new(),
                        n => format!(" and its {}", plural(n, "subtask")),
                    }
                );
                ask(confirm, &format!("Delete {what}?"), yes)?;
                let output = self.call("delete_task", json!({ "id": id }))?;
                Ok((output, format!("Deleted {what}.")))
            }
            Command::Project(command) => self.perform_project(command, confirm, printer),
        }
    }

    fn complete(
        &self,
        ids: &[String],
        completed: bool,
        printer: &Printer,
    ) -> Result<(Value, String), CliError> {
        let ids: Vec<Value> = ids.iter().map(|id| task_id(id)).collect::<Result<_, _>>()?;
        // All or nothing, so a typo doesn't leave the list half done.
        let outputs = self.tools.all_or_nothing(|tools| {
            ids.iter()
                .map(|id| {
                    tools.call(
                        "complete_task",
                        &json!({ "id": id, "completed": completed }),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        let verb = if completed { "Completed" } else { "Reopened" };
        let text = outputs
            .iter()
            .map(|o| format!("{verb} {}", printer.line(o, 0, 0, true)))
            .collect::<Vec<_>>()
            .join("\n");
        let output = if outputs.len() == 1 {
            outputs[0].clone()
        } else {
            Value::Array(outputs)
        };
        Ok((output, text))
    }

    fn perform_project(
        &self,
        command: ProjectCommand,
        confirm: Confirm,
        printer: &Printer,
    ) -> Result<(Value, String), CliError> {
        match command {
            ProjectCommand::Add { name, color } => {
                let mut args = json!({ "name": name.join(" ") });
                if let Some(color) = color.color {
                    args["color"] = json!(color_key(&color));
                }
                let output = self.call("add_project", args)?;
                let text = format!("Added project {}", printer.project(&output));
                Ok((output, text))
            }
            ProjectCommand::Edit {
                project,
                name,
                color,
            } => {
                let mut args = json!({ "id": self.project_id(&project.join(" "))? });
                if let Some(name) = name {
                    args["name"] = json!(name);
                }
                if let Some(color) = color.color {
                    args["color"] = json!(color_key(&color));
                }
                if args.as_object().is_some_and(|a| a.len() == 1) {
                    return Err(CliError::usage(
                        "Nothing to change. Pass --name or --color.",
                    ));
                }
                let output = self.call("update_project", args)?;
                let text = format!("Updated project {}", printer.project(&output));
                Ok((output, text))
            }
            ProjectCommand::Delete { project, yes } => {
                let id = self.project_id(&project.join(" "))?;
                let projects = self.call("list_projects", json!({}))?;
                let found = projects["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["id"] == id);
                if let Some(found) = found.filter(|p| p["is_inbox"] == false) {
                    let tasks = self.call(
                        "list_tasks",
                        json!({ "project_id": id, "include_completed": true }),
                    )?;
                    let count = tasks["count"].as_u64().unwrap_or(0) as usize;
                    let name = found["name"].as_str().unwrap_or_default();
                    let question = match count {
                        0 => format!("Delete project \"{name}\"?"),
                        n => format!("Delete project \"{name}\" and its {}?", plural(n, "task")),
                    };
                    ask(confirm, &question, yes)?;
                }
                let output = self.call("delete_project", json!({ "id": id }))?;
                let name = output["name"].as_str().unwrap_or_default();
                let text = match output["tasks_deleted"].as_u64().unwrap_or(0) as usize {
                    0 => format!("Deleted project \"{name}\"."),
                    n => format!("Deleted project \"{name}\" and its {}.", plural(n, "task")),
                };
                Ok((output, text))
            }
        }
    }

    /// A project by ID or by name, ignoring case and a leading "#".
    fn project_id(&self, text: &str) -> Result<Value, CliError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CliError::usage("Give a project's ID or name."));
        }
        if let Ok(id) = text.parse::<i64>() {
            return Ok(json!(id));
        }
        let name = text.strip_prefix('#').unwrap_or(text).to_lowercase();
        let projects = self.call("list_projects", json!({}))?;
        projects["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["name"].as_str().is_some_and(|n| n.to_lowercase() == name))
            .map(|p| p["id"].clone())
            .ok_or_else(|| CliError {
                message: format!(
                    "No project called \"{}\". Run ostracon projects to see them.",
                    text.trim_start_matches('#')
                ),
                status: 1,
            })
    }
}

fn task_id(text: &str) -> Result<Value, CliError> {
    let trimmed = text.trim();
    trimmed
        .strip_prefix('#')
        .unwrap_or(trimmed)
        .parse::<i64>()
        .map(|id| json!(id))
        .map_err(|_| CliError::usage(format!("\"{text}\" isn't a task ID.")))
}

/// Accepts "lime green", "Lime-Green" and the like; anything else goes to the tool to reject.
fn color_key(text: &str) -> String {
    text.parse::<ProjectColor>()
        .map(|c| c.key().to_owned())
        .unwrap_or_else(|_| text.to_owned())
}

fn count_subtasks(task: &Value) -> usize {
    task["subtasks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| 1 + count_subtasks(t))
        .sum()
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn ask(confirm: Confirm, question: &str, skip: bool) -> Result<(), CliError> {
    if skip {
        return Ok(());
    }
    let Some(confirm) = confirm else {
        return Err(CliError::usage("Pass --yes to delete without being asked."));
    };
    if confirm(question) {
        Ok(())
    } else {
        Err(CliError {
            message: "Nothing deleted.".into(),
            status: 1,
        })
    }
}
