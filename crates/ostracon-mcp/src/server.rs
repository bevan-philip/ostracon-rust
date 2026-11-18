//! MCP's JSON-RPC methods. Transport-agnostic: stdio and HTTP both hand it raw messages.

use std::sync::Mutex;

use jiff::ZonedDateTime;
use ostracon_core::Repository;
use ostracon_core::tools::{Tools, definitions};
use serde_json::{Value, json};

pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Ostracon is the user's task manager. Every task belongs to a project; the Inbox always \
    exists. Tasks can have subtasks (parent_id), a due date and reminders. Dates accept natural language such as \
    \"tomorrow 5pm\", \"next fri\" or \"jan 27\", or ISO \"2026-01-27\" / \"2026-01-27T17:00\", read in the user's \
    local time zone. Completing a task also completes its subtasks.";

type Clock = Box<dyn Fn() -> ZonedDateTime + Send + Sync>;

pub struct Server {
    repo: Mutex<Repository>,
    clock: Clock,
}

impl Server {
    pub fn new(repo: Repository) -> Self {
        Server {
            repo: Mutex::new(repo),
            clock: Box::new(ZonedDateTime::now),
        }
    }

    /// A server that reads "now" from `clock`, for tests.
    pub fn with_clock(
        repo: Repository,
        clock: impl Fn() -> ZonedDateTime + Send + Sync + 'static,
    ) -> Self {
        Server {
            repo: Mutex::new(repo),
            clock: Box::new(clock),
        }
    }

    /// Handles raw bytes: one message or a batch. Returns `None` when nothing should be sent back.
    pub fn handle_bytes(&self, bytes: &[u8]) -> Option<Value> {
        match serde_json::from_slice::<Value>(bytes) {
            Ok(message) => self.handle(&message),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        }
    }

    pub fn handle(&self, message: &Value) -> Option<Value> {
        match message {
            Value::Array(batch) if batch.is_empty() => {
                Some(error(Value::Null, -32600, "Invalid request"))
            }
            Value::Array(batch) => {
                let responses: Vec<Value> =
                    batch.iter().filter_map(|m| self.handle_one(m)).collect();
                (!responses.is_empty()).then_some(Value::Array(responses))
            }
            single => self.handle_one(single),
        }
    }

    fn handle_one(&self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to something we sent (we send nothing), or junk.
            return match id {
                Some(id) if message.get("result").is_none() && message.get("error").is_none() => {
                    Some(error(id, -32600, "Invalid request"))
                }
                None if !message.is_object() => Some(error(Value::Null, -32600, "Invalid request")),
                _ => None,
            };
        };
        // Without an id it's a notification, such as notifications/initialized, and gets no reply.
        let id = id?;
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        Some(match method {
            "initialize" => result(id, self.initialize(&params)),
            "ping" => result(id, json!({})),
            "tools/list" => {
                let tools: Vec<Value> = definitions().iter().map(|d| d.to_mcp()).collect();
                result(id, json!({ "tools": tools }))
            }
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(error(id, -32602, "Unknown tool: (none)"));
                };
                if !definitions().iter().any(|d| d.name == name) {
                    return Some(error(id, -32602, &format!("Unknown tool: {name}")));
                }
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                result(id, self.call_tool(name, &arguments))
            }
            "resources/list" => result(id, json!({ "resources": [] })),
            "prompts/list" => result(id, json!({ "prompts": [] })),
            _ => error(id, -32601, &format!("Method not found: {method}")),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let version = PROTOCOL_VERSIONS
            .iter()
            .find(|&&v| v == requested)
            .unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "ostracon", "title": "Ostracon", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    /// Runs a tool. Its failures are reported as tool errors, so the model can see and correct them.
    fn call_tool(&self, name: &str, arguments: &Value) -> Value {
        let repo = self
            .repo
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tools = Tools::with_clock(&repo, &self.clock);
        match tools.call(name, arguments) {
            Ok(output) => json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&output).unwrap_or_default() }],
                "structuredContent": output,
                "isError": false,
            }),
            Err(e) => json!({ "content": [{ "type": "text", "text": e.0 }], "isError": true }),
        }
    }
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
