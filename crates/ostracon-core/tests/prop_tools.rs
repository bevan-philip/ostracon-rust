//! Property tests of the tool layer the CLI and MCP server share: random calls with arguments both good and
//! bad, checking that a call that fails changes nothing, and that one that succeeds leaves a sound outline.

use jiff::ZonedDateTime;
use jiff::civil::date;
use ostracon_core::tools::{Tools, definitions};
use ostracon_core::{NewTask, ProjectColor, Repository};
use proptest::prelude::*;
use proptest::sample::Index;
use serde_json::{Map, Value, json};

fn now() -> ZonedDateTime {
    date(2026, 1, 14)
        .at(10, 30, 0, 0)
        .in_tz("Europe/London")
        .unwrap()
}

#[derive(Debug, Clone)]
enum Arg {
    /// An existing task's or project's id, by kind.
    Task(Index),
    Project(Index),
    Missing,
    Text(&'static str),
    Bool(bool),
    Null,
    Strings(Vec<&'static str>),
    Number(f64),
}

const TEXTS: &[&str] = &[
    "Buy milk",
    "Pay rent tomorrow 9am #Home !1h",
    "#Work report next fri",
    "",
    "   ",
    "today",
    "tomorrow 5pm",
    "2026-01-27",
    "2026-02-30",
    "2026-01-27T17:00",
    "feb 29",
    "nonsense",
    "30min",
    "lime green",
    "puce",
    "!",
];

fn arg() -> impl Strategy<Value = Arg> {
    prop_oneof![
        3 => any::<Index>().prop_map(Arg::Task),
        2 => any::<Index>().prop_map(Arg::Project),
        6 => Just(Arg::Missing),
        4 => proptest::sample::select(TEXTS).prop_map(Arg::Text),
        1 => any::<bool>().prop_map(Arg::Bool),
        1 => Just(Arg::Null),
        1 => proptest::collection::vec(proptest::sample::select(TEXTS), 0..3).prop_map(Arg::Strings),
        1 => proptest::sample::select(vec![1.5, -1.0, 1e300, 2.0]).prop_map(Arg::Number),
    ]
}

#[derive(Debug, Clone)]
struct Call {
    tool: Index,
    args: Vec<(Index, Arg)>,
    /// Occasionally an argument no tool takes.
    stray: bool,
}

fn call() -> impl Strategy<Value = Call> {
    (
        any::<Index>(),
        proptest::collection::vec((any::<Index>(), arg()), 8),
        proptest::bool::weighted(0.05),
    )
        .prop_map(|(tool, args, stray)| Call { tool, args, stray })
}

/// Each of the tool's arguments given or not; ids mostly refer to tasks and projects that exist.
fn arguments(
    repo: &Repository,
    call: &Call,
    tool: &str,
    properties: &[String],
    required: &[&str],
) -> Value {
    let snapshot = repo.snapshot().unwrap();
    let mut object = Map::new();
    for (key, (_, arg)) in properties.iter().zip(&call.args) {
        let arg = match arg {
            // Required arguments are usually there, so calls get as far as writing.
            Arg::Missing | Arg::Null | Arg::Number(_) if required.contains(&key.as_str()) => {
                &Arg::Text("Write report")
            }
            arg => arg,
        };
        let value = match (key.as_str(), arg) {
            (_, Arg::Missing) => continue,
            ("id" | "parent_id", Arg::Task(_) | Arg::Project(_) | Arg::Text(_) | Arg::Bool(_)) => {
                if key == "id" && tool.ends_with("_project") {
                    json!(snapshot.projects[pick(arg, snapshot.projects.len())].id)
                } else if snapshot.tasks.is_empty() {
                    json!(9999)
                } else {
                    json!(snapshot.tasks[pick(arg, snapshot.tasks.len())].id)
                }
            }
            ("project_id", Arg::Task(i) | Arg::Project(i)) => {
                json!(snapshot.projects[i.index(snapshot.projects.len())].id)
            }
            (_, Arg::Null) => Value::Null,
            // Numbers are the wrong type everywhere but ids.
            (_, Arg::Number(n)) => json!(n),
            ("completed" | "include_completed" | "quick_add", arg) => {
                json!(matches!(arg, Arg::Bool(true)) || pick(arg, 2) == 0)
            }
            ("reminders", Arg::Strings(s)) => json!(s),
            ("reminders", arg) => json!([TEXTS[pick(arg, TEXTS.len())]]),
            (_, Arg::Text(t)) => json!(t),
            (_, Arg::Strings(s)) => json!(s.first().copied().unwrap_or_default()),
            (_, arg) => json!(TEXTS[pick(arg, TEXTS.len())]),
        };
        object.insert(key.clone(), value);
    }
    if call.stray {
        object.insert("stray".into(), json!(1));
    }
    Value::Object(object)
}

/// Which of `len` things an argument picks: the first unless it carries an index.
fn pick(arg: &Arg, len: usize) -> usize {
    match arg {
        Arg::Task(i) | Arg::Project(i) => i.index(len),
        _ => 0,
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn a_failed_call_changes_nothing(calls in proptest::collection::vec(call(), 1..30)) {
        let repo = Repository::in_memory().unwrap();
        repo.create_project("Work", ProjectColor::Red).unwrap();
        repo.create_project("Home", ProjectColor::Blue).unwrap();
        let mut parent = None;
        for name in ["A", "B", "C", "D", "E"] {
            let task = repo.add_task(NewTask { name: name.into(), parent_id: parent, ..Default::default() }).unwrap();
            parent = (name != "C").then_some(task.id);
        }
        let tools = Tools::with_clock(&repo, now);
        let definitions = definitions();
        for call in &calls {
            let definition = &definitions[call.tool.index(definitions.len())];
            let properties: Vec<String> = definition.properties.as_object().unwrap().keys().cloned().collect();
            let args = arguments(&repo, call, definition.name, &properties, definition.required);
            let before = repo.snapshot().unwrap();
            let result = tools.call(definition.name, &args);
            let after = repo.snapshot().unwrap();
            if std::env::var_os("OSTRACON_COVERAGE").is_some() {
                let what = match &result { Ok(_) => "ok".to_owned(), Err(e) => e.0.chars().take(40).collect() };
                eprintln!("COV {} {}", definition.name, what);
            }
            match &result {
                Err(e) => prop_assert_eq!(&after, &before, "{} {} failed with {} yet changed things", definition.name, args, e),
                Ok(_) if definition.read_only => prop_assert_eq!(&after, &before, "{} {} is read-only", definition.name, args),
                Ok(_) => {}
            }
            for task in &after.tasks {
                if let Some(parent) = task.parent_id {
                    prop_assert_eq!(after.task(parent).map(|p| p.project_id), Some(task.project_id));
                }
                prop_assert!(!task.name.trim().is_empty() && task.name.trim() == task.name);
            }
        }
    }
}
