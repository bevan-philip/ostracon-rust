//! The tools Ostracon offers over MCP and the CLI, each a thin layer over [`Repository`]. Both front ends call
//! [`Tools::call`], so they behave identically.

use std::collections::HashMap;

use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use serde_json::{Map, Value, json};

use crate::dates::DateParser;
use crate::model::{Due, Project, ProjectColor, ProjectId, Reminder, Task, TaskId};
use crate::quickadd;
use crate::repo::{self, NewTask, Repository};

/// A mistake the caller can fix, reported back as text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

impl From<repo::Error> for ToolError {
    fn from(error: repo::Error) -> Self {
        let message = error.to_string();
        let mut chars = message.chars();
        ToolError(
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect::<String>() + ".")
                .unwrap_or_default(),
        )
    }
}

type ToolResult = Result<Value, ToolError>;

/// A tool's advertised definition.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub properties: Value,
    pub required: &'static [&'static str],
    pub read_only: bool,
    pub destructive: bool,
}

impl ToolDefinition {
    /// The definition as MCP's `tools/list` describes it.
    pub fn to_mcp(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": {
                "type": "object",
                "properties": self.properties,
                "required": self.required,
                "additionalProperties": false,
            },
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.read_only,
                "openWorldHint": false,
            },
        })
    }
}

const DUE_HELP: &str = "Natural language such as \"today\", \"tomorrow 5pm\", \"next fri\", \"in 3 days\", \"jan 27\", \
                        or ISO \"2026-01-27\" / \"2026-01-27T17:00\", in the user's local time.";
const REMINDER_HELP: &str = "Reminders, each either before the due time (\"30min\", \"1 hour before\", \"1d\") or at a \
                             moment (\"tomorrow 9am\", \"3pm\" - a bare time falls on the due day).";

pub fn definitions() -> Vec<ToolDefinition> {
    let colors: Vec<&str> = ProjectColor::ALL.iter().map(|c| c.key()).collect();
    vec![
        ToolDefinition {
            name: "list_projects",
            title: "List projects",
            description: "Lists every project, Inbox first, with its ID, colour and number of open tasks.",
            properties: json!({}),
            required: &[],
            read_only: true,
            destructive: false,
        },
        ToolDefinition {
            name: "list_tasks",
            title: "List tasks",
            description: "Lists tasks in outline order (each subtask follows its parent, with its depth). Open tasks \
                          only unless include_completed is set. Filters combine.",
            properties: json!({
                "project_id": {"type": "integer", "description": "Only this project's tasks."},
                "search": {"type": "string", "description": "Only tasks whose name or description contain this text."},
                "due_by": {"type": "string", "description": "Only tasks due on or before this day, e.g. \"today\" for today and overdue, or \"next week\"."},
                "include_completed": {"type": "boolean", "description": "Include completed tasks. Defaults to false."},
            }),
            required: &[],
            read_only: true,
            destructive: false,
        },
        ToolDefinition {
            name: "get_task",
            title: "Get task",
            description: "Gets one task with all of its subtasks, completed or not.",
            properties: json!({"id": {"type": "integer", "description": "The task's ID."}}),
            required: &["id"],
            read_only: true,
            destructive: false,
        },
        ToolDefinition {
            name: "add_task",
            title: "Add task",
            description: "Adds a task, by default to the Inbox. Like Ostracon's quick add, the name may include \
                          \"#Project Name\", a due date and \"!reminders\", e.g. \"Pay rent tomorrow 9am #Home !1h\"; \
                          these are taken out of the name. Explicit arguments win over what the name says.",
            properties: json!({
                "name": {"type": "string", "description": "The task's name, in quick-add syntax unless quick_add is false."},
                "description": {"type": "string", "description": "Longer notes shown under the name."},
                "project_id": {"type": "integer", "description": "The project to add it to. Defaults to the parent's project, or the Inbox."},
                "parent_id": {"type": "integer", "description": "Makes this a subtask of that task, after its other subtasks."},
                "due": {"type": "string", "description": DUE_HELP},
                "reminders": {"type": "array", "items": {"type": "string"}, "description": REMINDER_HELP},
                "quick_add": {"type": "boolean", "description": "Read #project, dates and !reminders out of the name. Defaults to true; turn off to keep the name exactly as given."},
            }),
            required: &["name"],
            read_only: false,
            destructive: false,
        },
        ToolDefinition {
            name: "update_task",
            title: "Update task",
            description: "Changes a task. Only the fields given change. Pass due as null or \"\" to remove the due \
                          date, and reminders as [] to remove them. Changing project_id moves the task and its \
                          subtasks to the end of that project's top level.",
            properties: json!({
                "id": {"type": "integer", "description": "The task's ID."},
                "name": {"type": "string", "description": "The new name, taken literally."},
                "description": {"type": "string", "description": "The new notes."},
                "due": {"type": ["string", "null"], "description": DUE_HELP},
                "reminders": {"type": "array", "items": {"type": "string"}, "description": format!("Replaces every reminder. {REMINDER_HELP}")},
                "project_id": {"type": "integer", "description": "Moves the task to this project."},
            }),
            required: &["id"],
            read_only: false,
            destructive: false,
        },
        ToolDefinition {
            name: "complete_task",
            title: "Complete task",
            description: "Marks a task done, along with all its subtasks. Pass completed: false to reopen it, which \
                          also reopens any completed parents.",
            properties: json!({
                "id": {"type": "integer", "description": "The task's ID."},
                "completed": {"type": "boolean", "description": "Defaults to true."},
            }),
            required: &["id"],
            read_only: false,
            destructive: false,
        },
        ToolDefinition {
            name: "delete_task",
            title: "Delete task",
            description: "Permanently deletes a task and all of its subtasks. Prefer complete_task for finished work.",
            properties: json!({"id": {"type": "integer", "description": "The task's ID."}}),
            required: &["id"],
            read_only: false,
            destructive: true,
        },
        ToolDefinition {
            name: "add_project",
            title: "Add project",
            description: "Creates a project at the end of the sidebar.",
            properties: json!({
                "name": {"type": "string", "description": "The project's name."},
                "color": {"type": "string", "enum": colors, "description": "Defaults to charcoal."},
            }),
            required: &["name"],
            read_only: false,
            destructive: false,
        },
        ToolDefinition {
            name: "update_project",
            title: "Update project",
            description: "Renames or recolours a project. The Inbox can't be changed.",
            properties: json!({
                "id": {"type": "integer", "description": "The project's ID."},
                "name": {"type": "string"},
                "color": {"type": "string", "enum": colors},
            }),
            required: &["id"],
            read_only: false,
            destructive: false,
        },
        ToolDefinition {
            name: "delete_project",
            title: "Delete project",
            description: "Permanently deletes a project and every task in it. The Inbox can't be deleted.",
            properties: json!({"id": {"type": "integer", "description": "The project's ID."}}),
            required: &["id"],
            read_only: false,
            destructive: true,
        },
    ]
}

/// The tools, bound to a repository and a clock.
pub struct Tools<'a> {
    repo: &'a Repository,
    clock: Box<dyn Fn() -> ZonedDateTime + 'a>,
}

impl<'a> Tools<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Tools {
            repo,
            clock: Box::new(ZonedDateTime::now),
        }
    }

    /// Tools that read "now" from `clock`, for tests.
    pub fn with_clock(repo: &'a Repository, clock: impl Fn() -> ZonedDateTime + 'a) -> Self {
        Tools {
            repo,
            clock: Box::new(clock),
        }
    }

    pub fn repository(&self) -> &Repository {
        self.repo
    }

    /// Runs the tool called `name` with `arguments` (a JSON object), all in one transaction.
    pub fn call(&self, name: &str, arguments: &Value) -> ToolResult {
        let Some(arguments) = arguments.as_object() else {
            return Err(error("Arguments must be an object."));
        };
        let definition = definitions()
            .into_iter()
            .find(|d| d.name == name)
            .ok_or_else(|| error(format!("No tool called {name}.")))?;
        if let Some(unknown) = arguments
            .keys()
            .find(|key| definition.properties.get(key.as_str()).is_none())
        {
            return Err(error(format!("{name} has no argument called {unknown}.")));
        }
        let args = Args(arguments);
        self.repo.transaction(|| self.dispatch(name, &args))
    }

    /// Runs several tool calls as one transaction: all of them happen, or none do.
    pub fn all_or_nothing<T>(
        &self,
        calls: impl FnOnce(&Self) -> Result<T, ToolError>,
    ) -> Result<T, ToolError> {
        self.repo.transaction(|| calls(self))
    }

    fn dispatch(&self, name: &str, args: &Args) -> ToolResult {
        match name {
            "list_projects" => self.list_projects(),
            "list_tasks" => self.list_tasks(args),
            "get_task" => self.get_task(args),
            "add_task" => self.add_task(args),
            "update_task" => self.update_task(args),
            "complete_task" => self.complete_task(args),
            "delete_task" => self.delete_task(args),
            "add_project" => self.add_project(args),
            "update_project" => self.update_project(args),
            "delete_project" => self.delete_project(args),
            _ => Err(error(format!("No tool called {name}."))),
        }
    }

    fn dates(&self) -> DateParser {
        DateParser::new((self.clock)())
    }

    fn list_projects(&self) -> ToolResult {
        let snapshot = self.repo.snapshot()?;
        let projects: Vec<Value> = snapshot
            .projects
            .iter()
            .map(|project| {
                let mut value = project_json(project);
                value["open_tasks"] = json!(crate::tree::open_count(&snapshot.tasks, project.id));
                value
            })
            .collect();
        Ok(json!({ "projects": projects }))
    }

    fn list_tasks(&self, args: &Args) -> ToolResult {
        let project = args
            .id("project_id")?
            .map(|id| self.repo.project(ProjectId(id)))
            .transpose()?;
        let search = args
            .string("search")?
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let due_by = args
            .string("due_by")?
            .map(|text| self.due(text))
            .transpose()?;
        let include_completed = args.bool("include_completed")?.unwrap_or(false);

        let formatter = Formatter::new(self.repo, (self.clock)())?;
        let rows: Vec<Value> = formatter
            .outline(project.map(|p| p.id), include_completed)
            .into_iter()
            .filter(|(task, _)| {
                let matches_search = search.as_ref().is_none_or(|s| {
                    task.name.to_lowercase().contains(s)
                        || task.description.to_lowercase().contains(s)
                });
                let matches_due =
                    due_by.is_none_or(|by| task.due.is_some_and(|due| due.date <= by.date));
                matches_search && matches_due
            })
            .map(|(task, depth)| formatter.task(task, Some(depth), false))
            .collect();
        Ok(json!({ "count": rows.len(), "tasks": rows }))
    }

    fn get_task(&self, args: &Args) -> ToolResult {
        let task = self.repo.task(TaskId(args.required_id("id")?))?;
        Ok(Formatter::new(self.repo, (self.clock)())?.task(&task, None, true))
    }

    fn add_task(&self, args: &Args) -> ToolResult {
        let text = args.required_string("name")?;
        let dates = self.dates();
        let parsed = if args.bool("quick_add")?.unwrap_or(true) {
            quickadd::parse(text, &self.repo.projects()?, &dates)
        } else {
            quickadd::QuickAddResult {
                name: text.to_owned(),
                project_id: None,
                due: None,
                reminders: Vec::new(),
                due_text: None,
            }
        };
        let parent = args
            .id("parent_id")?
            .map(|id| self.repo.task(TaskId(id)))
            .transpose()?;
        let project_id = match args.id("project_id")? {
            Some(id) => Some(ProjectId(id)),
            None => parent.as_ref().map(|p| p.project_id).or(parsed.project_id),
        };
        let due = match args.string("due")? {
            Some(text) => Some(self.due(text)?),
            None => parsed.due,
        };
        let reminders = match args.strings("reminders")? {
            Some(texts) => self.reminders(&texts, due)?,
            None => parsed.reminders,
        };
        let task = self.repo.add_task(NewTask {
            name: parsed.name,
            description: args.string("description")?.unwrap_or_default().to_owned(),
            project_id,
            parent_id: parent.map(|p| p.id),
            due,
            reminders,
        })?;
        self.describe(&task)
    }

    fn update_task(&self, args: &Args) -> ToolResult {
        let existing = self.repo.task(TaskId(args.required_id("id")?))?;
        let name = args.string("name")?.unwrap_or(&existing.name);
        let description = args.string("description")?.unwrap_or(&existing.description);
        self.repo.update_task(existing.id, name, description)?;
        let mut due = existing.due;
        if args.has("due") {
            let text = args.string("due")?.unwrap_or_default().trim();
            due = if text.is_empty() {
                None
            } else {
                Some(self.due(text)?)
            };
            self.repo.set_due(existing.id, due)?;
        }
        if let Some(texts) = args.strings("reminders")? {
            self.repo
                .set_reminders(existing.id, &self.reminders(&texts, due)?)?;
        }
        if let Some(project) = args.id("project_id")? {
            self.repo.move_to_project(existing.id, ProjectId(project))?;
        }
        self.describe(&self.repo.task(existing.id)?)
    }

    fn complete_task(&self, args: &Args) -> ToolResult {
        let id = TaskId(args.required_id("id")?);
        self.repo
            .set_completed(id, args.bool("completed")?.unwrap_or(true))?;
        self.describe(&self.repo.task(id)?)
    }

    fn delete_task(&self, args: &Args) -> ToolResult {
        let task = self.repo.task(TaskId(args.required_id("id")?))?;
        let tasks = self.repo.tasks()?;
        let subtasks = tasks
            .iter()
            .filter(|t| crate::tree::ancestors(&tasks, t.id).contains(&task.id))
            .count();
        self.repo.delete_task(task.id)?;
        Ok(json!({ "deleted": task.id, "name": task.name, "subtasks_deleted": subtasks }))
    }

    fn add_project(&self, args: &Args) -> ToolResult {
        let color = self
            .color(args.string("color")?)?
            .unwrap_or(ProjectColor::Charcoal);
        let project = self
            .repo
            .create_project(args.required_string("name")?, color)?;
        Ok(project_json(&project))
    }

    fn update_project(&self, args: &Args) -> ToolResult {
        let existing = self.repo.project(ProjectId(args.required_id("id")?))?;
        let name = args.string("name")?.unwrap_or(&existing.name);
        let color = self.color(args.string("color")?)?.unwrap_or(existing.color);
        self.repo.update_project(existing.id, name, color)?;
        Ok(project_json(&self.repo.project(existing.id)?))
    }

    fn delete_project(&self, args: &Args) -> ToolResult {
        let project = self.repo.project(ProjectId(args.required_id("id")?))?;
        let count = self.repo.delete_project(project.id)?;
        Ok(json!({ "deleted": project.id, "name": project.name, "tasks_deleted": count }))
    }

    // Helpers

    fn describe(&self, task: &Task) -> ToolResult {
        Ok(Formatter::new(self.repo, (self.clock)())?.task(task, None, false))
    }

    fn due(&self, text: &str) -> Result<Due, ToolError> {
        let text = text.trim();
        if let Some(due) = Due::from_storage(text) {
            return Ok(due);
        }
        self.dates().due(text).ok_or_else(|| {
            error(format!("Couldn't read \"{text}\" as a date. Try \"tomorrow 5pm\", \"next fri\" or \"2026-01-27\"."))
        })
    }

    fn reminders(&self, texts: &[&str], due: Option<Due>) -> Result<Vec<Reminder>, ToolError> {
        let dates = self.dates();
        texts
            .iter()
            .map(|text| {
                dates.reminder(text, due).ok_or_else(|| {
                    error(format!(
                        "Couldn't read \"{text}\" as a reminder. Try \"30min\" or \"tomorrow 9am\"."
                    ))
                })
            })
            .collect()
    }

    fn color(&self, name: Option<&str>) -> Result<Option<ProjectColor>, ToolError> {
        name.map(|name| {
            name.parse().map_err(|_| {
                let all: Vec<&str> = ProjectColor::ALL.iter().map(|c| c.key()).collect();
                error(format!(
                    "Unknown colour \"{name}\". Use one of: {}.",
                    all.join(", ")
                ))
            })
        })
        .transpose()
    }
}

fn error(message: impl Into<String>) -> ToolError {
    ToolError(message.into())
}

fn project_json(project: &Project) -> Value {
    json!({
        "id": project.id,
        "name": project.name,
        "color": project.color.key(),
        "is_inbox": project.is_inbox,
    })
}

/// Turns tasks into the JSON the tools return, from one snapshot of the database.
struct Formatter {
    tasks: Vec<Task>,
    projects: Vec<Project>,
    children: HashMap<Option<TaskId>, Vec<usize>>,
    now: ZonedDateTime,
}

impl Formatter {
    fn new(repo: &Repository, now: ZonedDateTime) -> Result<Self, ToolError> {
        let snapshot = repo.snapshot()?;
        let mut children: HashMap<Option<TaskId>, Vec<usize>> = HashMap::new();
        for (index, task) in snapshot.tasks.iter().enumerate() {
            children.entry(task.parent_id).or_default().push(index);
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|&i| (snapshot.tasks[i].position, snapshot.tasks[i].id));
        }
        Ok(Formatter {
            tasks: snapshot.tasks,
            projects: snapshot.projects,
            children,
            now,
        })
    }

    fn children_of(&self, parent: Option<TaskId>) -> impl Iterator<Item = &Task> {
        self.children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|&i| &self.tasks[i])
    }

    /// Tasks depth-first in sidebar then sibling order, ignoring collapsing. Without `include_completed`, a
    /// completed task hides its subtasks too, as in the app.
    fn outline(&self, project: Option<ProjectId>, include_completed: bool) -> Vec<(&Task, usize)> {
        fn append<'f>(
            formatter: &'f Formatter,
            rows: &mut Vec<(&'f Task, usize)>,
            siblings: Vec<&'f Task>,
            depth: usize,
            include_completed: bool,
        ) {
            for task in siblings {
                if include_completed || !task.is_completed() {
                    rows.push((task, depth));
                    let children = formatter.children_of(Some(task.id)).collect();
                    append(formatter, rows, children, depth + 1, include_completed);
                }
            }
        }
        let mut rows = Vec::new();
        for p in self
            .projects
            .iter()
            .filter(|p| project.is_none_or(|id| id == p.id))
        {
            let top = self
                .children_of(None)
                .filter(|t| t.project_id == p.id)
                .collect();
            append(self, &mut rows, top, 0, include_completed);
        }
        rows
    }

    fn task(&self, task: &Task, depth: Option<usize>, with_subtasks: bool) -> Value {
        let tz = self.now.time_zone();
        let subtasks: Vec<&Task> = self.children_of(Some(task.id)).collect();
        let mut object = Map::new();
        object.insert("id".into(), json!(task.id));
        object.insert("name".into(), json!(task.name));
        if !task.description.is_empty() {
            object.insert("description".into(), json!(task.description));
        }
        object.insert("project_id".into(), json!(task.project_id));
        let project = self
            .projects
            .iter()
            .find(|p| p.id == task.project_id)
            .map_or("", |p| p.name.as_str());
        object.insert("project".into(), json!(project));
        object.insert("parent_id".into(), json!(task.parent_id));
        object.insert("completed".into(), json!(task.is_completed()));
        if let Some(at) = task.completed_at {
            object.insert("completed_at".into(), json!(local_timestamp(at, tz)));
        }
        object.insert("due".into(), json!(task.due.map(|d| d.storage_value())));
        if let (Some(due), false) = (task.due, task.is_completed()) {
            object.insert("overdue".into(), json!(due.is_overdue(&self.now)));
        }
        let reminders: Vec<Value> = task
            .reminders
            .iter()
            .map(|reminder| match reminder {
                Reminder::At(at) => json!({ "at": local_timestamp(*at, tz) }),
                Reminder::Before { minutes } => json!({ "minutes_before_due": minutes }),
            })
            .collect();
        object.insert("reminders".into(), json!(reminders));
        object.insert(
            "created_at".into(),
            json!(local_timestamp(task.created_at, tz)),
        );
        if let Some(depth) = depth {
            object.insert("depth".into(), json!(depth));
        }
        if !subtasks.is_empty() {
            object.insert("subtask_count".into(), json!(subtasks.len()));
            object.insert(
                "completed_subtask_count".into(),
                json!(subtasks.iter().filter(|t| t.is_completed()).count()),
            );
        }
        if with_subtasks {
            let subtasks: Vec<Value> = subtasks.iter().map(|t| self.task(t, None, true)).collect();
            object.insert("subtasks".into(), json!(subtasks));
        }
        Value::Object(object)
    }
}

/// ISO 8601 in the user's time zone, so a model reads local times directly.
pub fn local_timestamp(at: Timestamp, tz: &TimeZone) -> String {
    ZonedDateTime::new(at, tz.clone())
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

/// Typed access to a tool call's arguments.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    /// `true` when the key was sent, even as `null` (which some tools read as "clear this").
    fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key).filter(|v| !v.is_null())
    }

    fn string(&self, key: &str) -> Result<Option<&str>, ToolError> {
        self.get(key)
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| error(format!("{key} must be a string.")))
            })
            .transpose()
    }

    fn required_string(&self, key: &str) -> Result<&str, ToolError> {
        self.string(key)?
            .ok_or_else(|| error(format!("{key} is required.")))
    }

    fn id(&self, key: &str) -> Result<Option<i64>, ToolError> {
        self.get(key)
            .map(|v| {
                v.as_i64()
                    .or_else(|| {
                        v.as_f64()
                            .filter(|f| f.fract() == 0.0 && f.abs() < 9e15)
                            .map(|f| f as i64)
                    })
                    .ok_or_else(|| error(format!("{key} must be an integer ID.")))
            })
            .transpose()
    }

    fn required_id(&self, key: &str) -> Result<i64, ToolError> {
        self.id(key)?
            .ok_or_else(|| error(format!("{key} is required.")))
    }

    fn bool(&self, key: &str) -> Result<Option<bool>, ToolError> {
        self.get(key)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| error(format!("{key} must be true or false.")))
            })
            .transpose()
    }

    fn strings(&self, key: &str) -> Result<Option<Vec<&str>>, ToolError> {
        let Some(value) = self.get(key) else {
            return Ok(None);
        };
        let not_strings = || error(format!("{key} must be an array of strings."));
        let array = value.as_array().ok_or_else(not_strings)?;
        array
            .iter()
            .map(|v| v.as_str().ok_or_else(not_strings))
            .collect::<Result<_, _>>()
            .map(Some)
    }
}
