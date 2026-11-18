//! Renders the tools' JSON as text for a terminal.

use jiff::civil::Date;
use jiff::{Timestamp, ZonedDateTime};
use ostracon_core::Due;
use ostracon_core::model::days_between;
use serde_json::Value;

pub struct Printer {
    now: ZonedDateTime,
}

impl Printer {
    pub fn new(now: ZonedDateTime) -> Self {
        Printer { now }
    }

    pub fn projects(&self, output: &Value) -> String {
        let projects = array(&output["projects"]);
        let id_width = projects
            .iter()
            .map(|p| text(&p["id"]).len())
            .max()
            .unwrap_or(0);
        let name_width = projects
            .iter()
            .map(|p| text(&p["name"]).chars().count())
            .max()
            .unwrap_or(0);
        projects
            .iter()
            .map(|p| {
                format!(
                    "{:>id_width$}  {:<name_width$}  {} open",
                    text(&p["id"]),
                    text(&p["name"]),
                    text(&p["open_tasks"])
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// "5 Garden (lime_green)", for a project the tools just changed.
    pub fn project(&self, output: &Value) -> String {
        format!(
            "{} {} ({})",
            text(&output["id"]),
            text(&output["name"]),
            text(&output["color"])
        )
    }

    /// A task list in outline order, under a heading for each project when it spans several.
    pub fn tasks(&self, output: &Value, grouped: bool) -> String {
        let tasks = array(&output["tasks"]);
        if tasks.is_empty() {
            return "No tasks.".into();
        }
        let id_width = tasks
            .iter()
            .map(|t| text(&t["id"]).len())
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        let mut current_project = None;
        for task in tasks {
            if grouped && current_project != Some(&task["project_id"]) {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(text(&task["project"]));
                current_project = Some(&task["project_id"]);
            }
            let depth = task["depth"].as_u64().unwrap_or(0) as usize;
            lines.push(self.line(task, id_width, depth, false));
        }
        lines.join("\n")
    }

    /// One task on one line: "12  [ ] Pay rent  due tomorrow 09:00 · 1/3 subtasks".
    pub fn line(&self, task: &Value, id_width: usize, depth: usize, with_project: bool) -> String {
        let mut notes = Vec::new();
        if let Some(due) = task["due"].as_str() {
            let overdue = if task["overdue"] == true {
                ", overdue"
            } else {
                ""
            };
            notes.push(format!("due {}{overdue}", self.due_text(due)));
        }
        if with_project {
            notes.push(format!("#{}", text(&task["project"])));
        }
        if let Some(count) = task["subtask_count"].as_u64() {
            notes.push(format!(
                "{}/{count} subtasks",
                task["completed_subtask_count"].as_u64().unwrap_or(0)
            ));
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("  {}", notes.join(" · "))
        };
        format!(
            "{:>id_width$}  {}{} {}{notes}",
            text(&task["id"]),
            "  ".repeat(depth),
            checkbox(task),
            text(&task["name"])
        )
    }

    /// Everything about a task, for `show`.
    pub fn detail(&self, task: &Value) -> String {
        let mut facts = vec![
            format!("ID {}", text(&task["id"])),
            format!("#{}", text(&task["project"])),
        ];
        if let Some(due) = task["due"].as_str() {
            let overdue = if task["overdue"] == true {
                ", overdue"
            } else {
                ""
            };
            facts.push(format!("due {}{overdue}", self.due_text(due)));
        }
        if let Some(at) = task["completed_at"]
            .as_str()
            .and_then(|s| s.parse::<Timestamp>().ok())
        {
            facts.push(format!("completed {}", self.moment_text(at)));
        }
        let mut lines = vec![
            format!("{} {}", checkbox(task), text(&task["name"])),
            facts.join(" · "),
        ];
        let reminders = array(&task["reminders"]);
        if !reminders.is_empty() {
            let texts: Vec<String> = reminders.iter().map(|r| self.reminder_text(r)).collect();
            lines.push(format!("Reminders: {}", texts.join(", ")));
        }
        if let Some(description) = task["description"].as_str() {
            lines.push(String::new());
            lines.push(description.to_owned());
        }
        let mut subtasks = Vec::new();
        fn collect<'v>(children: &'v Value, depth: usize, into: &mut Vec<(&'v Value, usize)>) {
            for child in array(children) {
                into.push((child, depth));
                collect(&child["subtasks"], depth + 1, into);
            }
        }
        collect(&task["subtasks"], 0, &mut subtasks);
        if !subtasks.is_empty() {
            let id_width = subtasks
                .iter()
                .map(|(t, _)| text(&t["id"]).len())
                .max()
                .unwrap_or(0);
            lines.push(String::new());
            lines.push("Subtasks:".into());
            lines.extend(
                subtasks
                    .iter()
                    .map(|(t, depth)| format!("  {}", self.line(t, id_width, *depth, false))),
            );
        }
        lines.join("\n")
    }

    /// "today", "tomorrow 17:00", "Wednesday", "Sat 7 Nov", "Mon 4 Jan 2027".
    pub fn due_text(&self, storage: &str) -> String {
        let Some(due) = Due::from_storage(storage) else {
            return storage.to_owned();
        };
        let day = self.day_text(due.date);
        match due.time {
            Some(time) => format!("{day} {}", time.strftime("%H:%M")),
            None => day,
        }
    }

    fn moment_text(&self, at: Timestamp) -> String {
        let local = ZonedDateTime::new(at, self.now.time_zone().clone());
        format!(
            "{} {}",
            self.day_text(local.date()),
            local.strftime("%H:%M")
        )
    }

    fn day_text(&self, date: Date) -> String {
        match days_between(self.now.date(), date) {
            0 => "today".into(),
            1 => "tomorrow".into(),
            -1 => "yesterday".into(),
            2..=6 => date.strftime("%A").to_string(),
            _ if date.year() == self.now.year() => date.strftime("%a %-d %b").to_string(),
            _ => date.strftime("%a %-d %b %Y").to_string(),
        }
    }

    fn reminder_text(&self, reminder: &Value) -> String {
        if let Some(minutes) = reminder["minutes_before_due"].as_i64() {
            let (amount, unit) = match minutes {
                m if m > 0 && m % 1440 == 0 => (m / 1440, "day"),
                m if m > 0 && m % 60 == 0 => (m / 60, "hour"),
                m => (m, "min"),
            };
            let s = if amount == 1 || unit == "min" {
                ""
            } else {
                "s"
            };
            return format!("{amount} {unit}{s} before");
        }
        match reminder["at"]
            .as_str()
            .and_then(|s| s.parse::<Timestamp>().ok())
        {
            Some(at) => self.moment_text(at),
            None => reminder.to_string(),
        }
    }
}

fn checkbox(task: &Value) -> &'static str {
    if task["completed"] == true {
        "[x]"
    } else {
        "[ ]"
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
