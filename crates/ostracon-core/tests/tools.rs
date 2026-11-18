use jiff::ZonedDateTime;
use jiff::civil::date;
use ostracon_core::Repository;
use ostracon_core::tools::{Tools, definitions};
use serde_json::{Value, json};

/// Wednesday 14 January 2026, 10:30 in London.
fn now() -> ZonedDateTime {
    date(2026, 1, 14)
        .at(10, 30, 0, 0)
        .in_tz("Europe/London")
        .unwrap()
}

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    tools
        .call(name, &args)
        .unwrap_or_else(|e| panic!("{name} failed: {e}"))
}

fn fail(tools: &Tools, name: &str, args: Value) -> String {
    tools.call(name, &args).expect_err("should fail").0
}

#[test]
fn definitions_are_valid_mcp() {
    let defs = definitions();
    assert_eq!(defs.len(), 10);
    for def in defs {
        let value = def.to_mcp();
        assert_eq!(value["inputSchema"]["type"], "object");
        for required in value["inputSchema"]["required"].as_array().unwrap() {
            assert!(
                value["inputSchema"]["properties"]
                    .get(required.as_str().unwrap())
                    .is_some()
            );
        }
    }
}

#[test]
fn add_task_reads_quick_add_text() {
    let repo = Repository::in_memory().unwrap();
    let tools = Tools::with_clock(&repo, now);
    let home = call(
        &tools,
        "add_project",
        json!({"name": "Home", "color": "lime green"}),
    );
    assert_eq!(home["color"], "lime_green");
    let task = call(
        &tools,
        "add_task",
        json!({"name": "Pay rent tomorrow 9am #Home !1h"}),
    );
    assert_eq!(task["name"], "Pay rent");
    assert_eq!(task["project"], "Home");
    assert_eq!(task["due"], "2026-01-15T09:00");
    assert_eq!(task["reminders"], json!([{"minutes_before_due": 60}]));
    assert_eq!(task["overdue"], false);

    let literal = call(
        &tools,
        "add_task",
        json!({"name": "Read #Home tomorrow", "quick_add": false}),
    );
    assert_eq!(literal["name"], "Read #Home tomorrow");
    assert_eq!(literal["project"], "Inbox");
    assert_eq!(literal["due"], Value::Null);

    let explicit = call(
        &tools,
        "add_task",
        json!({"name": "Thing friday", "due": "2026-02-01", "reminders": ["tomorrow 9am"]}),
    );
    assert_eq!(explicit["due"], "2026-02-01", "explicit arguments win");
    assert_eq!(
        explicit["reminders"],
        json!([{"at": "2026-01-15T09:00:00+00:00"}])
    );
}

#[test]
fn subtasks_and_listing() {
    let repo = Repository::in_memory().unwrap();
    let tools = Tools::with_clock(&repo, now);
    let parent = call(&tools, "add_task", json!({"name": "Plan trip"}));
    let child = call(
        &tools,
        "add_task",
        json!({"name": "Book train today", "parent_id": parent["id"]}),
    );
    call(&tools, "add_task", json!({"name": "Unrelated next week"}));
    assert_eq!(child["parent_id"], parent["id"]);

    let listed = call(&tools, "list_tasks", json!({}));
    assert_eq!(listed["count"], 3);
    assert_eq!(listed["tasks"][1]["depth"], 1);
    assert_eq!(listed["tasks"][0]["subtask_count"], 1);

    let today = call(&tools, "list_tasks", json!({"due_by": "today"}));
    assert_eq!(today["count"], 1);
    assert_eq!(today["tasks"][0]["name"], "Book train");

    let search = call(&tools, "list_tasks", json!({"search": "TRIP"}));
    assert_eq!(search["count"], 1);

    call(&tools, "complete_task", json!({"id": parent["id"]}));
    assert_eq!(call(&tools, "list_tasks", json!({}))["count"], 1);
    assert_eq!(
        call(&tools, "list_tasks", json!({"include_completed": true}))["count"],
        3
    );

    let detail = call(&tools, "get_task", json!({"id": parent["id"]}));
    assert_eq!(detail["subtasks"][0]["name"], "Book train");
    assert_eq!(detail["subtasks"][0]["completed"], true);

    call(
        &tools,
        "complete_task",
        json!({"id": child["id"], "completed": false}),
    );
    assert_eq!(
        call(&tools, "get_task", json!({"id": parent["id"]}))["completed"],
        false,
        "reopening reopens parents"
    );
}

#[test]
fn update_task_changes_only_what_is_given() {
    let repo = Repository::in_memory().unwrap();
    let tools = Tools::with_clock(&repo, now);
    let work = call(&tools, "add_project", json!({"name": "Work"}));
    let task = call(
        &tools,
        "add_task",
        json!({"name": "Report friday 5pm !30min", "description": "Q1"}),
    );
    let updated = call(
        &tools,
        "update_task",
        json!({"id": task["id"], "name": "Report monday"}),
    );
    assert_eq!(
        updated["name"], "Report monday",
        "update takes the name literally"
    );
    assert_eq!(updated["description"], "Q1");
    assert_eq!(updated["due"], "2026-01-16T17:00");
    let cleared = call(
        &tools,
        "update_task",
        json!({"id": task["id"], "due": null, "reminders": [], "project_id": work["id"]}),
    );
    assert_eq!(cleared["due"], Value::Null);
    assert_eq!(cleared["reminders"], json!([]));
    assert_eq!(cleared["project"], "Work");
}

#[test]
fn mistakes_are_reported_and_change_nothing() {
    let repo = Repository::in_memory().unwrap();
    let tools = Tools::with_clock(&repo, now);
    let task = call(&tools, "add_task", json!({"name": "Keep me"}));
    assert_eq!(
        fail(&tools, "get_task", json!({"id": 99})),
        "No task with ID 99."
    );
    assert!(
        fail(&tools, "add_task", json!({"name": "x", "due": "someday"}))
            .starts_with("Couldn't read \"someday\"")
    );
    assert!(
        fail(&tools, "add_project", json!({"name": "x", "color": "puce"}))
            .starts_with("Unknown colour")
    );
    assert_eq!(fail(&tools, "add_task", json!({})), "name is required.");
    assert_eq!(
        fail(&tools, "add_task", json!({"name": 3})),
        "name must be a string."
    );
    assert_eq!(
        fail(&tools, "add_task", json!({"name": "x", "bogus": 1})),
        "add_task has no argument called bogus."
    );
    assert_eq!(fail(&tools, "nope", json!({})), "No tool called nope.");
    // The rename happens before the bad date is read, and is rolled back with it.
    fail(
        &tools,
        "update_task",
        json!({"id": task["id"], "name": "Renamed", "due": "someday"}),
    );
    assert_eq!(
        call(&tools, "get_task", json!({"id": task["id"]}))["name"],
        "Keep me"
    );
    let inbox = call(&tools, "list_projects", json!({}))["projects"][0]["id"].clone();
    assert!(fail(&tools, "delete_project", json!({"id": inbox})).contains("Inbox"));
}

#[test]
fn deleting() {
    let repo = Repository::in_memory().unwrap();
    let tools = Tools::with_clock(&repo, now);
    let project = call(&tools, "add_project", json!({"name": "Old"}));
    let parent = call(
        &tools,
        "add_task",
        json!({"name": "A", "project_id": project["id"]}),
    );
    call(
        &tools,
        "add_task",
        json!({"name": "B", "parent_id": parent["id"]}),
    );
    let projects = call(&tools, "list_projects", json!({}));
    assert_eq!(projects["projects"][1]["open_tasks"], 2);
    let deleted = call(&tools, "delete_task", json!({"id": parent["id"]}));
    assert_eq!(deleted["subtasks_deleted"], 1);
    let deleted = call(&tools, "delete_project", json!({"id": project["id"]}));
    assert_eq!(deleted["tasks_deleted"], 0);
}
