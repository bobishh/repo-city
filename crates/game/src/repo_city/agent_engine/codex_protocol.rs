//! Codex wire boundary. Only validated, normalized provider events leave this module.
//! Field names follow the installed app-server v2 schema; unrelated fields stay forward compatible.
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Component, Path},
};

pub(super) struct Notification {
    pub thread: String,
    pub update: Update,
}

pub(super) enum Update {
    TurnStarted {
        id: String,
    },
    TurnCompleted {
        error: Option<String>,
    },
    MessageDelta {
        id: String,
        text: String,
    },
    Transcript {
        session: String,
        id: String,
        role: Role,
        text: String,
    },
    Item {
        id: String,
        completed: bool,
        content: Item,
    },
}

pub(super) enum Item {
    Message {
        role: Role,
        text: String,
    },
    Edit {
        files: Vec<Location>,
        applied: bool,
        detail: String,
    },
    Read {
        files: Vec<Location>,
    },
    Collaboration {
        agents: HashMap<String, AgentState>,
    },
}

pub(super) struct Location {
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Role {
    User,
    Assistant,
}
impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentState {
    pub status: AgentStatus,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum AgentStatus {
    Running,
    PendingInit,
    Completed,
    Errored,
    Shutdown,
    NotFound,
    #[serde(other)]
    Unknown,
}
impl AgentStatus {
    pub fn active(&self) -> bool {
        matches!(self, Self::Running | Self::PendingInit)
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::PendingInit => "pendingInit",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Shutdown => "shutdown",
            Self::NotFound => "notFound",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) fn notification(
    root: &Path,
    method: &str,
    params: Value,
) -> Result<Option<Notification>, String> {
    let decoded = match method {
        "turn/started" => {
            let event: TurnNotification = decode(method, params)?;
            Notification {
                thread: event.thread_id,
                update: Update::TurnStarted { id: event.turn.id },
            }
        }
        "turn/completed" => {
            let event: TurnNotification = decode(method, params)?;
            Notification {
                thread: event.thread_id,
                update: Update::TurnCompleted {
                    error: event.turn.error.map(|error| error.message),
                },
            }
        }
        "item/agentMessage/delta" => {
            let event: MessageDelta = decode(method, params)?;
            Notification {
                thread: event.thread_id,
                update: Update::MessageDelta {
                    id: event.item_id,
                    text: event.delta,
                },
            }
        }
        "item/started" | "item/completed" => {
            let event: ItemNotification = decode(method, params)?;
            let Some((id, content)) = event.item.normalize(root) else {
                return Ok(None);
            };
            Notification {
                thread: event.thread_id,
                update: Update::Item {
                    id,
                    completed: method == "item/completed",
                    content,
                },
            }
        }
        "thread/realtime/item/completed" => {
            let event: RealtimeNotification = decode(method, params)?;
            match event.item {
                RealtimeItem::TranscriptSegment {
                    id,
                    realtime_session_id,
                    role,
                    text,
                } => Notification {
                    thread: event.thread_id,
                    update: Update::Transcript {
                        session: realtime_session_id,
                        id,
                        role,
                        text,
                    },
                },
                RealtimeItem::Other => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(decoded))
}

pub(super) fn approval(params: Value) -> Result<Approval, String> {
    decode("approval", params)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Approval {
    #[serde(deserialize_with = "identifier")]
    pub thread_id: String,
    #[serde(deserialize_with = "identifier")]
    pub item_id: String,
    pub reason: Option<String>,
    pub command: Option<String>,
}

fn decode<T: DeserializeOwned>(method: &str, params: Value) -> Result<T, String> {
    serde_json::from_value(params)
        .map_err(|error| format!("Invalid Codex {method} notification: {error}"))
}
fn identifier<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom("identifier must not be empty"));
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnNotification {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    turn: Turn,
}
#[derive(Deserialize)]
struct Turn {
    #[serde(deserialize_with = "identifier")]
    id: String,
    error: Option<TurnError>,
}
#[derive(Deserialize)]
struct TurnError {
    message: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDelta {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    #[serde(deserialize_with = "identifier")]
    item_id: String,
    delta: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemNotification {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    item: WireItem,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WireItem {
    AgentMessage {
        #[serde(deserialize_with = "identifier")]
        id: String,
        text: String,
    },
    UserMessage {
        #[serde(deserialize_with = "identifier")]
        id: String,
        content: Vec<UserInput>,
    },
    FileChange {
        #[serde(deserialize_with = "identifier")]
        id: String,
        changes: Vec<FileChange>,
        status: PatchStatus,
    },
    CommandExecution {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(rename = "commandActions")]
        actions: Vec<CommandAction>,
    },
    McpToolCall {
        #[serde(deserialize_with = "identifier")]
        id: String,
        arguments: Value,
    },
    CollabAgentToolCall {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(rename = "agentsStates")]
        agents: HashMap<String, AgentState>,
    },
    #[serde(other)]
    Other,
}
impl WireItem {
    fn normalize(self, root: &Path) -> Option<(String, Item)> {
        let normalized = match self {
            Self::AgentMessage { id, text } => (
                id,
                Item::Message {
                    role: Role::Assistant,
                    text,
                },
            ),
            Self::UserMessage { id, content } => {
                let text = content
                    .into_iter()
                    .filter_map(|part| match part {
                        UserInput::Text { text } => Some(text),
                        UserInput::Other => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if text.starts_with("<repo_city_context>") {
                    text.split_once("</repo_city_context>")
                        .map_or(text.as_str(), |(_, text)| text.trim())
                        .to_owned()
                } else {
                    text
                };
                (
                    id,
                    Item::Message {
                        role: Role::User,
                        text,
                    },
                )
            }
            Self::FileChange {
                id,
                changes,
                status,
            } => {
                let detail = changes
                    .iter()
                    .take(12)
                    .map(|change| format!("{}\n{}", change.path, change.diff))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .chars()
                    .take(6000)
                    .collect();
                let files = changes
                    .into_iter()
                    .take(16)
                    .filter_map(|change| {
                        relative(root, &change.path).map(|path| Location {
                            path,
                            line: changed_line(&change.diff).unwrap_or(1),
                        })
                    })
                    .collect();
                (
                    id,
                    Item::Edit {
                        files,
                        applied: matches!(status, PatchStatus::Completed),
                        detail,
                    },
                )
            }
            Self::CommandExecution { id, actions } => {
                let files = actions
                    .into_iter()
                    .filter_map(|action| match action {
                        CommandAction::Read { path } => {
                            relative(root, &path).map(|path| Location { path, line: 1 })
                        }
                        CommandAction::Other => None,
                    })
                    .take(16)
                    .collect();
                (id, Item::Read { files })
            }
            Self::McpToolCall { id, arguments } => (
                id,
                Item::Read {
                    files: mcp_location(root, arguments).into_iter().collect(),
                },
            ),
            Self::CollabAgentToolCall { id, agents } => (id, Item::Collaboration { agents }),
            Self::Other => return None,
        };
        Some(normalized)
    }
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum UserInput {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
struct FileChange {
    path: String,
    diff: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum PatchStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum CommandAction {
    Read {
        path: String,
    },
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RealtimeNotification {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    item: RealtimeItem,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum RealtimeItem {
    TranscriptSegment {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(rename = "realtimeSessionId", deserialize_with = "identifier")]
        realtime_session_id: String,
        role: Role,
        text: String,
    },
    #[serde(other)]
    Other,
}

// MCP arguments are tool-defined JSON, not a fixed Codex schema. An absent or differently
// shaped optional hint is valid; only an explicit safe path can become measured activity.
fn mcp_location(root: &Path, arguments: Value) -> Option<Location> {
    let object = arguments.as_object()?;
    let path = relative(root, object.get("path")?.as_str()?)?;
    let line = object
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|line| usize::try_from(line).ok())
        .filter(|&line| line > 0)
        .unwrap_or(1);
    Some(Location { path, line })
}
fn changed_line(diff: &str) -> Option<usize> {
    let header = diff.lines().find(|line| line.starts_with("@@ "))?;
    let range = header
        .split_whitespace()
        .find(|part| part.starts_with('+'))?;
    range[1..]
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()
        .map(|line| line.max(1))
}
fn relative(root: &Path, path: &str) -> Option<String> {
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    path.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(method: &str, params: Value) -> Result<Option<Notification>, String> {
        notification(Path::new("/repo"), method, params)
    }

    #[test]
    fn supported_payloads_reject_missing_or_mistyped_required_data() {
        for (method, params) in [
            ("turn/started", json!({"threadId":"main","turn":{"id":""}})),
            (
                "turn/completed",
                json!({"threadId":"main","turn":{"id":"t","error":{}}}),
            ),
            (
                "item/agentMessage/delta",
                json!({"threadId":"main","itemId":"a","delta":42}),
            ),
            (
                "item/completed",
                json!({"threadId":"main","item":{"id":"a","type":"agentMessage"}}),
            ),
            (
                "item/completed",
                json!({"threadId":"main","item":{"id":"a","type":"userMessage","content":[{"type":"text"}]}}),
            ),
            (
                "item/completed",
                json!({"threadId":"main","item":{"id":"a","type":"fileChange","status":"completed","changes":[{"path":"src/main.rs"}]}}),
            ),
            (
                "item/started",
                json!({"threadId":"main","item":{"id":"a","type":"commandExecution","commandActions":[{"type":"read","path":5}]}}),
            ),
            (
                "item/started",
                json!({"threadId":"main","item":{"id":"a","type":"collabAgentToolCall","agentsStates":{"child":{}}}}),
            ),
            (
                "thread/realtime/item/completed",
                json!({"threadId":"main","item":{"id":"a","type":"transcriptSegment","role":"user","text":"hello"}}),
            ),
        ] {
            assert!(
                event(method, params).is_err(),
                "accepted malformed {method}"
            );
        }
    }

    #[test]
    fn unknown_variants_are_ignored_without_discarding_known_parts() {
        assert!(event("future/event", json!(null)).unwrap().is_none());
        assert!(
            event(
                "item/started",
                json!({"threadId":"main","item":{"type":"futureItem","newField":5}})
            )
            .unwrap()
            .is_none()
        );
        let value = event("item/completed", json!({"threadId":"main","futureField":true,"item":{"id":"user","type":"userMessage","content":[{"type":"futureInput"},{"type":"text","text":"hello","futureField":true}]}})).unwrap().unwrap();
        assert!(
            matches!(value.update, Update::Item { content: Item::Message { text, .. }, .. } if text == "hello")
        );
    }

    #[test]
    fn explicit_file_locations_are_normalized_once_and_unsafe_paths_are_excluded() {
        let value = event("item/started", json!({"threadId":"main","item":{"id":"read","type":"commandExecution","commandActions":[{"type":"unknown","command":"cat invisible.rs"},{"type":"read","path":"/repo/src/main.rs"},{"type":"read","path":"../secret"},{"type":"read","path":"/other/private"}]}})).unwrap().unwrap();
        let Update::Item {
            content: Item::Read { files },
            ..
        } = value.update
        else {
            panic!("expected read");
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].line, 1);
        let value = event("item/completed", json!({"threadId":"main","item":{"id":"edit","type":"fileChange","status":"completed","changes":[{"path":"src/main.rs","diff":"@@ -4 +9,2 @@\n+new"}]}})).unwrap().unwrap();
        let Update::Item {
            content: Item::Edit { files, applied, .. },
            ..
        } = value.update
        else {
            panic!("expected edit");
        };
        assert!(applied);
        assert_eq!(files[0].line, 9);
    }

    #[test]
    fn mcp_arguments_stay_tool_defined_and_only_explicit_hints_create_activity() {
        for arguments in [
            json!(null),
            json!(["src/main.rs"]),
            json!({"command":"cat src/main.rs"}),
            json!({"path":7}),
        ] {
            let value = event("item/started", json!({"threadId":"main","item":{"id":"mcp","type":"mcpToolCall","arguments":arguments}})).unwrap().unwrap();
            assert!(
                matches!(value.update, Update::Item { content: Item::Read { files }, .. } if files.is_empty())
            );
        }
        let value = event("item/started", json!({"threadId":"main","item":{"id":"mcp","type":"mcpToolCall","arguments":{"path":"src/main.rs","line":23}}})).unwrap().unwrap();
        assert!(
            matches!(value.update, Update::Item { content: Item::Read { files }, .. } if files[0].line == 23)
        );
    }

    #[test]
    fn approval_requires_explicit_owner_and_item_but_optional_display_fields_can_be_absent() {
        let value = approval(json!({"threadId":"main","itemId":"item"})).unwrap();
        assert_eq!(value.thread_id, "main");
        assert!(value.reason.is_none());
        assert!(approval(json!({"threadId":"main"})).is_err());
        assert!(approval(json!({"threadId":"main","itemId":"item","command":[]})).is_err());
    }
}
