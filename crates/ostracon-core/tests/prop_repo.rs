//! Model-based tests of the repository: random histories of outline edits run against the real database and
//! against a plain model (each list of siblings a `Vec`), comparing results and the whole outline after every
//! step. Transactions, nested ones included, roll the model back with them.

use std::collections::BTreeMap;

use ostracon_core::repo::Error;
use ostracon_core::{NewTask, ProjectColor, ProjectId, Repository, TaskId};
use proptest::prelude::*;
use proptest::sample::Index;

/// Where a list of siblings hangs: a project's top level or a task's subtasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Top(i64),
    Under(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MTask {
    project: i64,
    parent: Option<i64>,
    completed: bool,
    collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    inbox: i64,
    /// Inbox first, then in creation order.
    projects: Vec<i64>,
    tasks: BTreeMap<i64, MTask>,
    lists: BTreeMap<Key, Vec<i64>>,
}

/// What an operation returned, comparable across the two.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Done,
    Bool(bool),
    Count(usize),
    Err(&'static str),
}

fn kind(error: &Error) -> &'static str {
    match error {
        Error::EmptyName => "EmptyName",
        Error::NoSuchTask(_) => "NoSuchTask",
        Error::NoSuchProject(_) => "NoSuchProject",
        Error::InboxIsFixed => "InboxIsFixed",
        Error::ParentInOtherProject(_) => "ParentInOtherProject",
        Error::InsideItself(_) => "InsideItself",
        Error::NotASibling(_) => "NotASibling",
        other => panic!("unexpected error {other}"),
    }
}

impl Model {
    fn new(inbox: i64) -> Self {
        Model {
            inbox,
            projects: vec![inbox],
            tasks: BTreeMap::new(),
            lists: BTreeMap::new(),
        }
    }

    fn key_of(&self, id: i64) -> Key {
        let task = &self.tasks[&id];
        match task.parent {
            Some(parent) => Key::Under(parent),
            None => Key::Top(task.project),
        }
    }

    fn list(&self, key: Key) -> Vec<i64> {
        self.lists.get(&key).cloned().unwrap_or_default()
    }

    fn list_mut(&mut self, key: Key) -> &mut Vec<i64> {
        self.lists.entry(key).or_default()
    }

    fn unlink(&mut self, id: i64) {
        let key = self.key_of(id);
        self.list_mut(key).retain(|&t| t != id);
    }

    fn subtree(&self, id: i64) -> Vec<i64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.list(Key::Under(out[i])));
            i += 1;
        }
        out
    }

    fn ancestors(&self, id: i64) -> Vec<i64> {
        let mut out = Vec::new();
        let mut current = self.tasks[&id].parent;
        while let Some(p) = current {
            out.push(p);
            current = self.tasks[&p].parent;
        }
        out
    }

    fn add(&mut self, id: i64, project: Option<i64>, parent: Option<i64>) -> Outcome {
        let project = match (project, parent) {
            (Some(p), _) => p,
            (None, Some(parent)) => self.tasks[&parent].project,
            (None, None) => self.inbox,
        };
        if let Some(parent) = parent
            && self.tasks[&parent].project != project
        {
            return Outcome::Err("ParentInOtherProject");
        }
        self.tasks.insert(
            id,
            MTask {
                project,
                parent,
                completed: false,
                collapsed: false,
            },
        );
        let key = self.key_of(id);
        self.list_mut(key).push(id);
        Outcome::Done
    }

    fn delete(&mut self, id: i64) -> Outcome {
        self.unlink(id);
        for t in self.subtree(id) {
            self.tasks.remove(&t);
            self.lists.remove(&Key::Under(t));
        }
        Outcome::Done
    }

    fn complete(&mut self, id: i64, completed: bool) -> Outcome {
        if completed {
            for t in self.subtree(id) {
                self.tasks.get_mut(&t).unwrap().completed = true;
            }
        } else {
            for t in std::iter::once(id).chain(self.ancestors(id)) {
                self.tasks.get_mut(&t).unwrap().completed = false;
            }
        }
        Outcome::Done
    }

    fn move_to_project(&mut self, id: i64, project: i64) -> Outcome {
        if self.tasks[&id].project == project {
            return Outcome::Done;
        }
        self.unlink(id);
        self.tasks.get_mut(&id).unwrap().parent = None;
        for t in self.subtree(id) {
            self.tasks.get_mut(&t).unwrap().project = project;
        }
        self.list_mut(Key::Top(project)).push(id);
        Outcome::Done
    }

    fn open_siblings(&self, id: i64) -> Vec<i64> {
        self.list(self.key_of(id))
            .into_iter()
            .filter(|t| !self.tasks[t].completed)
            .collect()
    }

    fn indent(&mut self, id: i64) -> Outcome {
        let open = self.open_siblings(id);
        let Some(index) = open.iter().position(|&t| t == id).filter(|&i| i > 0) else {
            return Outcome::Bool(false);
        };
        let parent = open[index - 1];
        self.unlink(id);
        self.tasks.get_mut(&id).unwrap().parent = Some(parent);
        self.list_mut(Key::Under(parent)).push(id);
        self.tasks.get_mut(&parent).unwrap().collapsed = false;
        Outcome::Bool(true)
    }

    fn outdent(&mut self, id: i64) -> Outcome {
        let Some(parent) = self.tasks[&id].parent else {
            return Outcome::Bool(false);
        };
        let outer = self.key_of(parent);
        self.unlink(id);
        self.tasks.get_mut(&id).unwrap().parent = self.tasks[&parent].parent;
        let list = self.list_mut(outer);
        let at = list.iter().position(|&t| t == parent).unwrap();
        list.insert(at + 1, id);
        Outcome::Bool(true)
    }

    fn move_by(&mut self, id: i64, offset: isize) -> Outcome {
        let open = self.open_siblings(id);
        let Some(index) = open.iter().position(|&t| t == id) else {
            return Outcome::Bool(false);
        };
        let Some(&neighbour) = index.checked_add_signed(offset).and_then(|i| open.get(i)) else {
            return Outcome::Bool(false);
        };
        let key = self.key_of(id);
        let list = self.list_mut(key);
        let a = list.iter().position(|&t| t == id).unwrap();
        let b = list.iter().position(|&t| t == neighbour).unwrap();
        list.swap(a, b);
        Outcome::Bool(true)
    }

    fn place(
        &mut self,
        ids: &[i64],
        project: i64,
        parent: Option<i64>,
        after: Option<i64>,
    ) -> Outcome {
        if let Some(parent) = parent {
            if self.tasks[&parent].project != project {
                return Outcome::Err("ParentInOtherProject");
            }
            for &id in ids {
                if self.subtree(id).contains(&parent) {
                    return Outcome::Err("InsideItself");
                }
            }
        }
        let key = match parent {
            Some(p) => Key::Under(p),
            None => Key::Top(project),
        };
        let mut siblings: Vec<i64> = self
            .list(key)
            .into_iter()
            .filter(|t| !ids.contains(t))
            .collect();
        let index = match after {
            None => 0,
            Some(after) => match siblings.iter().position(|&s| s == after) {
                Some(i) => i + 1,
                None => return Outcome::Err("NotASibling"),
            },
        };
        if let Some(parent) = parent {
            self.tasks.get_mut(&parent).unwrap().collapsed = false;
        }
        for &id in ids {
            self.unlink(id);
            self.tasks.get_mut(&id).unwrap().parent = parent;
            for t in self.subtree(id) {
                self.tasks.get_mut(&t).unwrap().project = project;
            }
        }
        siblings.splice(index..index, ids.iter().copied());
        self.lists.insert(key, siblings);
        Outcome::Done
    }

    fn delete_project(&mut self, project: i64) -> Outcome {
        if project == self.inbox {
            return Outcome::Err("InboxIsFixed");
        }
        let doomed: Vec<i64> = self
            .tasks
            .iter()
            .filter(|(_, t)| t.project == project)
            .map(|(&id, _)| id)
            .collect();
        for id in &doomed {
            self.tasks.remove(id);
            self.lists.remove(&Key::Under(*id));
        }
        self.lists.remove(&Key::Top(project));
        self.projects.retain(|&p| p != project);
        Outcome::Count(doomed.len())
    }

    /// The model with empty lists dropped, for comparison.
    fn normalised(&self) -> Model {
        let mut m = self.clone();
        m.lists.retain(|_, v| !v.is_empty());
        m
    }
}

/// The model the database describes.
fn observe(repo: &Repository) -> Model {
    let snapshot = repo.snapshot().unwrap();
    let inbox = snapshot.inbox().unwrap().id.0;
    let mut model = Model::new(inbox);
    model.projects = snapshot.projects.iter().map(|p| p.id.0).collect();
    // Tasks come in (position, id) order, so each list fills in sibling order.
    for task in &snapshot.tasks {
        model.tasks.insert(
            task.id.0,
            MTask {
                project: task.project_id.0,
                parent: task.parent_id.map(|p| p.0),
                completed: task.is_completed(),
                collapsed: task.collapsed,
            },
        );
    }
    for task in &snapshot.tasks {
        let key = model.key_of(task.id.0);
        model.list_mut(key).push(task.id.0);
    }
    // Invariants the outline needs, checked independently of the model.
    for task in &snapshot.tasks {
        if let Some(parent) = task.parent_id {
            let parent = snapshot.task(parent).expect("parent exists");
            assert_eq!(
                parent.project_id, task.project_id,
                "a subtask is in its parent's project"
            );
        }
        assert!(
            snapshot.project(task.project_id).is_some(),
            "the project exists"
        );
    }
    let mut positions: BTreeMap<(Key, i64), i64> = BTreeMap::new();
    for task in &snapshot.tasks {
        let key = model.key_of(task.id.0);
        if let Some(other) = positions.insert((key, task.position), task.id.0) {
            panic!(
                "tasks {other} and {} share position {} under {key:?}",
                task.id, task.position
            );
        }
    }
    model
}

#[derive(Debug, Clone)]
enum ParentSel {
    None,
    /// Among the tasks of the chosen project.
    InProject(Index),
    Any(Index),
}

#[derive(Debug, Clone)]
enum AfterSel {
    None,
    /// Among the would-be siblings.
    Sibling(Index),
    Any(Index),
}

#[derive(Debug, Clone)]
enum Op {
    Add {
        project: Option<Index>,
        parent: Option<Index>,
    },
    Delete(Index),
    Complete(Index, bool),
    MoveToProject(Index, Index),
    Indent(Index),
    Outdent(Index),
    MoveBy(Index, bool),
    Place {
        ids: Vec<Index>,
        project: Index,
        parent: ParentSel,
        after: AfterSel,
    },
    SetCollapsed(Index, bool),
    CreateProject,
    DeleteProject(Index),
    Transaction {
        ops: Vec<Op>,
        fail: bool,
    },
}

fn parent_sel() -> impl Strategy<Value = ParentSel> {
    prop_oneof![
        1 => Just(ParentSel::None),
        3 => any::<Index>().prop_map(ParentSel::InProject),
        1 => any::<Index>().prop_map(ParentSel::Any),
    ]
}

fn after_sel() -> impl Strategy<Value = AfterSel> {
    prop_oneof![
        1 => Just(AfterSel::None),
        4 => any::<Index>().prop_map(AfterSel::Sibling),
        1 => any::<Index>().prop_map(AfterSel::Any),
    ]
}

fn leaf_op() -> impl Strategy<Value = Op> {
    let idx = any::<Index>;
    prop_oneof![
        6 => (proptest::option::weighted(0.3, idx()), proptest::option::weighted(0.6, idx()))
            .prop_map(|(project, parent)| Op::Add { project, parent }),
        1 => idx().prop_map(Op::Delete),
        2 => (idx(), any::<bool>()).prop_map(|(i, b)| Op::Complete(i, b)),
        1 => (idx(), idx()).prop_map(|(i, p)| Op::MoveToProject(i, p)),
        3 => idx().prop_map(Op::Indent),
        2 => idx().prop_map(Op::Outdent),
        2 => (idx(), any::<bool>()).prop_map(|(i, b)| Op::MoveBy(i, b)),
        3 => (proptest::collection::vec(idx(), 1..4), idx(), parent_sel(), after_sel())
            .prop_map(|(ids, project, parent, after)| Op::Place { ids, project, parent, after }),
        1 => (idx(), any::<bool>()).prop_map(|(i, b)| Op::SetCollapsed(i, b)),
        1 => Just(Op::CreateProject),
        1 => idx().prop_map(Op::DeleteProject),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    leaf_op().prop_recursive(2, 16, 6, |inner| {
        prop_oneof![
            8 => leaf_op(),
            1 => (proptest::collection::vec(inner, 1..6), any::<bool>())
                .prop_map(|(ops, fail)| Op::Transaction { ops, fail }),
        ]
    })
}

struct Ctx {
    model: Model,
    projects_made: usize,
}

fn outcome<T>(result: Result<T, Error>, f: impl FnOnce(T) -> Outcome) -> Outcome {
    match result {
        Ok(v) => f(v),
        Err(e) => Outcome::Err(kind(&e)),
    }
}

impl Ctx {
    fn new(repo: &Repository) -> Self {
        Ctx {
            model: Model::new(repo.inbox().unwrap().id.0),
            projects_made: 0,
        }
    }

    fn task(&self, i: &Index) -> Option<i64> {
        let ids: Vec<i64> = self.model.tasks.keys().copied().collect();
        (!ids.is_empty()).then(|| ids[i.index(ids.len())])
    }

    fn project(&self, i: &Index) -> i64 {
        self.model.projects[i.index(self.model.projects.len())]
    }

    fn run(&mut self, repo: &Repository, op: &Op) {
        let (real, modelled) = match op {
            Op::Add { project, parent } => {
                let project = project.as_ref().map(|p| self.project(p));
                let parent = parent.as_ref().and_then(|p| self.task(p));
                let result = repo.add_task(NewTask {
                    name: "task".into(),
                    project_id: project.map(ProjectId),
                    parent_id: parent.map(TaskId),
                    ..Default::default()
                });
                match result {
                    Ok(task) => (Outcome::Done, self.model.add(task.id.0, project, parent)),
                    Err(e) => {
                        // The model decides whether it should have failed; it learns no id.
                        let mut scratch = self.model.clone();
                        (
                            Outcome::Err(kind(&e)),
                            scratch.add(i64::MAX, project, parent),
                        )
                    }
                }
            }
            Op::Delete(i) => {
                let Some(id) = self.task(i) else { return };
                (
                    outcome(repo.delete_task(TaskId(id)), |_| Outcome::Done),
                    self.model.delete(id),
                )
            }
            Op::Complete(i, done) => {
                let Some(id) = self.task(i) else { return };
                (
                    outcome(repo.set_completed(TaskId(id), *done), |_| Outcome::Done),
                    self.model.complete(id, *done),
                )
            }
            Op::MoveToProject(i, p) => {
                let Some(id) = self.task(i) else { return };
                let project = self.project(p);
                (
                    outcome(repo.move_to_project(TaskId(id), ProjectId(project)), |_| {
                        Outcome::Done
                    }),
                    self.model.move_to_project(id, project),
                )
            }
            Op::Indent(i) => {
                let Some(id) = self.task(i) else { return };
                (
                    outcome(repo.indent(TaskId(id)), Outcome::Bool),
                    self.model.indent(id),
                )
            }
            Op::Outdent(i) => {
                let Some(id) = self.task(i) else { return };
                (
                    outcome(repo.outdent(TaskId(id)), Outcome::Bool),
                    self.model.outdent(id),
                )
            }
            Op::MoveBy(i, down) => {
                let Some(id) = self.task(i) else { return };
                let offset = if *down { 1 } else { -1 };
                (
                    outcome(repo.move_by(TaskId(id), offset), Outcome::Bool),
                    self.model.move_by(id, offset),
                )
            }
            Op::Place {
                ids,
                project,
                parent,
                after,
            } => {
                let mut chosen: Vec<i64> = Vec::new();
                for i in ids {
                    let Some(id) = self.task(i) else { return };
                    if !chosen.contains(&id) {
                        chosen.push(id);
                    }
                }
                let project = self.project(project);
                let parent = match parent {
                    ParentSel::None => None,
                    ParentSel::Any(i) => self.task(i),
                    ParentSel::InProject(i) => {
                        let in_project: Vec<i64> = self
                            .model
                            .tasks
                            .iter()
                            .filter(|(_, t)| t.project == project)
                            .map(|(&id, _)| id)
                            .collect();
                        (!in_project.is_empty()).then(|| in_project[i.index(in_project.len())])
                    }
                };
                let key = match parent {
                    Some(p) => Key::Under(p),
                    None => Key::Top(project),
                };
                let after = match after {
                    AfterSel::None => None,
                    AfterSel::Any(i) => self.task(i),
                    AfterSel::Sibling(i) => {
                        let siblings: Vec<i64> = self
                            .model
                            .list(key)
                            .into_iter()
                            .filter(|t| !chosen.contains(t))
                            .collect();
                        (!siblings.is_empty()).then(|| siblings[i.index(siblings.len())])
                    }
                };
                let task_ids: Vec<TaskId> = chosen.iter().copied().map(TaskId).collect();
                (
                    outcome(
                        repo.place(
                            &task_ids,
                            ProjectId(project),
                            parent.map(TaskId),
                            after.map(TaskId),
                        ),
                        |_| Outcome::Done,
                    ),
                    self.model.place(&chosen, project, parent, after),
                )
            }
            Op::SetCollapsed(i, collapsed) => {
                let Some(id) = self.task(i) else { return };
                self.model.tasks.get_mut(&id).unwrap().collapsed = *collapsed;
                (
                    outcome(repo.set_collapsed(TaskId(id), *collapsed), |_| {
                        Outcome::Done
                    }),
                    Outcome::Done,
                )
            }
            Op::CreateProject => {
                self.projects_made += 1;
                let project = repo
                    .create_project(&format!("P{}", self.projects_made), ProjectColor::Blue)
                    .unwrap();
                self.model.projects.push(project.id.0);
                (Outcome::Done, Outcome::Done)
            }
            Op::DeleteProject(p) => {
                let project = self.project(p);
                (
                    outcome(repo.delete_project(ProjectId(project)), Outcome::Count),
                    self.model.delete_project(project),
                )
            }
            Op::Transaction { ops, fail } => {
                let before = self.model.clone();
                let result: Result<(), Error> = repo.transaction(|| {
                    for op in ops {
                        self.run(repo, op);
                    }
                    if *fail { Err(Error::EmptyName) } else { Ok(()) }
                });
                if *fail {
                    self.model = before;
                }
                (
                    outcome(result, |_| Outcome::Done),
                    if *fail {
                        Outcome::Err("EmptyName")
                    } else {
                        Outcome::Done
                    },
                )
            }
        };
        record(op, &real);
        assert_eq!(real, modelled, "result of {op:?}");
        assert_eq!(
            observe(repo).normalised(),
            self.model.normalised(),
            "state after {op:?}"
        );
    }
}

static COVERAGE: std::sync::Mutex<BTreeMap<String, usize>> = std::sync::Mutex::new(BTreeMap::new());

/// Counts what each kind of operation did, printed with `OSTRACON_COVERAGE=1` to check the generator reaches
/// the interesting cases.
fn record(op: &Op, outcome: &Outcome) {
    let name = format!("{op:?}");
    let name = name.split([' ', '(', '{']).next().unwrap_or_default();
    *COVERAGE
        .lock()
        .unwrap()
        .entry(format!("{name} -> {outcome:?}"))
        .or_default() += 1;
}

fn print_coverage() {
    if std::env::var_os("OSTRACON_COVERAGE").is_some() {
        for (what, count) in COVERAGE.lock().unwrap().iter() {
            eprintln!("{count:>8}  {what}");
        }
    }
}

/// `OSTRACON_COVERAGE=1 cargo test --test prop_repo -- --ignored --nocapture`
#[test]
#[ignore = "prints what the generated histories reach"]
fn coverage() {
    repository_matches_the_model();
    print_coverage();
}

/// `PROPTEST_CASES` when set, for longer searches, or `default`.
fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(256),
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn repository_matches_the_model(ops in proptest::collection::vec(op(), 1..40)) {
        let repo = Repository::in_memory().unwrap();
        let mut ctx = Ctx::new(&repo);
        for op in &ops {
            ctx.run(&repo, op);
        }
    }
}
