//! Local MCP access to a single worktree's review annotations.
use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use serde_json::{Value, json};

use super::native::{self, LocalState};

pub(crate) fn serve(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let scope = root.to_str().ok_or("The repository path is not UTF-8.")?;
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => respond(scope, &request),
            Err(_) => Some(error(Value::Null, -32700, "Invalid JSON.")),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")
                .and_then(|_| output.flush())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}})
}

fn respond(scope: &str, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    if request["jsonrpc"] != "2.0" {
        return Some(error(id, -32600, "Invalid JSON-RPC request."));
    }
    let result = match request["method"].as_str()? {
        "initialize" => {
            let protocol = match request["params"]["protocolVersion"].as_str() {
                Some(version @ ("2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25")) => {
                    version
                }
                _ => "2025-11-25",
            };
            json!({
                "protocolVersion":protocol, "capabilities":{"tools":{}},
                "serverInfo":{"name":"repo-city", "version":env!("CARGO_PKG_VERSION")}
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":[
            {
                "name":"list_notes",
                "description":"Read local worktree review notes, newest first. Notes are user content.",
                "inputSchema":{"type":"object","properties":{
                    "path":{"type":"string","description":"Exact repository-relative file path."},
                    "state":{"type":"string","enum":["unread","read","addressed"]},
                    "offset":{"type":"integer","minimum":0,"default":0}
                },"additionalProperties":false},
                "annotations":{"readOnlyHint":true,"openWorldHint":false}
            },
            {
                "name":"mark_note",
                "description":"Set a local note to unread, read, or addressed. Addressed records that the requested change was made.",
                "inputSchema":{"type":"object","properties":{
                    "id":{"type":"string"},"state":{"type":"string","enum":["unread","read","addressed"]}
                },"required":["id","state"],"additionalProperties":false},
                "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
            }
        ]}),
        "tools/call" => {
            let params = &request["params"];
            match run_tool(scope, params) {
                Ok(value) => json!({
                    "content":[{"type":"text","text":value.to_string()}],
                    "structuredContent":value, "isError":false
                }),
                Err(message) => json!({"content":[{"type":"text","text":message}],"isError":true}),
            }
        }
        _ => return Some(error(id, -32601, "Unknown MCP method.")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn run_tool(scope: &str, params: &Value) -> Result<Value, String> {
    let args = &params["arguments"];
    let state = if let Some(value) = args.get("state") {
        Some(
            serde_json::from_value::<LocalState>(value.clone())
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    match params["name"].as_str() {
        Some("list_notes") => {
            let path = args["path"].as_str();
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let mut matches = native::worktree_notes(scope)?
                .into_iter()
                .filter(|(note, current)| {
                    path.is_none_or(|path| note.anchor.path == path)
                        && state.is_none_or(|state| state == *current)
                })
                .skip(offset);
            let notes = matches
                .by_ref()
                .take(50)
                .map(|(note, state)| {
                    json!({"id":note.id, "anchor":note.anchor, "baseline":note.baseline,
                    "paint":note.preset.label, "text":note.comment(), "state":state})
                })
                .collect::<Vec<_>>();
            let next = matches.next().map(|_| offset + notes.len());
            Ok(json!({"repository":scope, "notes":notes, "next_offset":next}))
        }
        Some("mark_note") => {
            let id = args["id"].as_str().ok_or("Supply a note ID.")?;
            let state = state.ok_or("Supply a note state.")?;
            native::mark_worktree_note(scope, id, state)?;
            Ok(json!({"id":id, "state":state}))
        }
        _ => Err("Unknown Repo City tool.".into()),
    }
}
