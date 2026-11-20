//! Codex wire boundary. Only validated, normalized provider events leave this module.
//! Field names follow the installed app-server v2 schema; unrelated fields stay forward compatible.
pub(super) use super::activity::Location;
use super::activity::{Details, Kind, Phase};
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
        id: String,
        error: Option<String>,
    },
    MessageDelta {
        id: String,
        text: String,
    },
    ActivityDelta {
        id: String,
        kind: Kind,
        text: String,
    },
    Activity {
        id: String,
        details: Details,
        phase: Phase,
    },
    ThreadStatus {
        status: ThreadPhase,
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
        activity: Details,
    },
    Collaboration {
        agents: HashMap<String, AgentState>,
        tool: Option<String>,
    },
    Activity(Details),
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ThreadPhase {
    Active,
    Idle,
    SystemError,
    NotLoaded,
    #[serde(other)]
    Other,
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
                    id: event.turn.id,
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
        "item/plan/delta"
        | "item/reasoning/summaryTextDelta"
        | "item/commandExecution/outputDelta"
        | "item/mcpToolCall/progress" => {
            let event: MessageDelta = decode(method, params)?;
            let kind = match method {
                "item/plan/delta" => Kind::Plan,
                "item/reasoning/summaryTextDelta" => Kind::Reasoning,
                "item/commandExecution/outputDelta" => Kind::Command,
                _ => Kind::Tool,
            };
            Notification {
                thread: event.thread_id,
                update: Update::ActivityDelta {
                    id: event.item_id,
                    kind,
                    text: event.delta,
                },
            }
        }
        "thread/status/changed" => {
            let event: ThreadStatus = decode(method, params)?;
            Notification {
                thread: event.thread_id,
                update: Update::ThreadStatus {
                    status: event.status.kind,
                },
            }
        }
        "turn/plan/updated" => {
            let event: PlanUpdate = decode(method, params)?;
            let mut details = Details::new(Kind::Plan, "Plan status");
            details.body = event
                .explanation
                .into_iter()
                .chain(event.plan.into_iter().take(24).map(|step| {
                    let status = match step.status {
                        PlanStatus::Pending => "Pending",
                        PlanStatus::InProgress => "In progress",
                        PlanStatus::Completed => "Done",
                    };
                    format!("{status} · {}", step.step)
                }))
                .collect::<Vec<_>>()
                .join("\n");
            Notification {
                thread: event.thread_id,
                update: Update::Activity {
                    id: format!("plan:{}", event.turn_id),
                    details,
                    phase: Phase::Running,
                },
            }
        }
        "item/autoApprovalReview/started" | "item/autoApprovalReview/completed" => {
            let event: AutomaticReview = decode(method, params)?;
            let (title, phase) = match event.review.status {
                ReviewStatus::InProgress => ("Codex checking permission", Phase::Running),
                ReviewStatus::Approved => ("Permission approved by Codex", Phase::Completed),
                ReviewStatus::Denied => ("Permission denied by Codex", Phase::Failed),
                ReviewStatus::TimedOut => ("Permission review timed out", Phase::Failed),
                ReviewStatus::Aborted => ("Permission review stopped", Phase::Ended),
                ReviewStatus::Other => return Ok(None),
            };
            let mut details = Details::new(Kind::Session, title);
            details.body = event.review.rationale.unwrap_or_default();
            Notification {
                thread: event.thread_id,
                update: Update::Activity {
                    id: format!("approval:{}", event.review_id),
                    details,
                    phase,
                },
            }
        }
        "item/fileChange/patchUpdated" => {
            let event: PatchUpdate = decode(method, params)?;
            Notification {
                thread: event.thread_id,
                update: Update::Item {
                    id: event.item_id,
                    completed: false,
                    content: FileChange::normalize(event.changes, root, false),
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
    #[serde(alias = "message")]
    delta: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadStatus {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    status: StatusValue,
}
#[derive(Deserialize)]
struct StatusValue {
    #[serde(rename = "type")]
    kind: ThreadPhase,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanUpdate {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    #[serde(deserialize_with = "identifier")]
    turn_id: String,
    explanation: Option<String>,
    plan: Vec<PlanStep>,
}
#[derive(Deserialize)]
struct PlanStep {
    step: String,
    status: PlanStatus,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutomaticReview {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    #[serde(deserialize_with = "identifier")]
    review_id: String,
    review: Review,
}
#[derive(Deserialize)]
struct Review {
    status: ReviewStatus,
    rationale: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReviewStatus {
    InProgress,
    Approved,
    Denied,
    TimedOut,
    Aborted,
    #[serde(other)]
    Other,
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
        #[serde(rename = "commandActions", default)]
        actions: Vec<CommandAction>,
        command: Option<ShellCommand>,
        status: Option<StepStatus>,
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        #[serde(rename = "aggregatedOutput")]
        output: Option<String>,
    },
    McpToolCall {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(default)]
        arguments: Value,
        #[serde(alias = "serverName")]
        server: Option<String>,
        #[serde(alias = "toolName", alias = "name")]
        tool: Option<String>,
        status: Option<StepStatus>,
    },
    DynamicToolCall {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(default)]
        arguments: Value,
        namespace: Option<String>,
        tool: Option<String>,
        status: Option<StepStatus>,
    },
    #[serde(alias = "collabToolCall")]
    CollabAgentToolCall {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(rename = "agentsStates", default)]
        agents: HashMap<String, AgentState>,
        tool: Option<String>,
    },
    SubAgentActivity {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(rename = "agentThreadId", deserialize_with = "identifier")]
        thread: String,
        kind: SubagentAction,
    },
    ImageView {
        #[serde(deserialize_with = "identifier")]
        id: String,
        path: String,
    },
    WebSearch {
        #[serde(deserialize_with = "identifier")]
        id: String,
        query: Option<String>,
    },
    Plan {
        #[serde(deserialize_with = "identifier")]
        id: String,
        text: Option<String>,
    },
    Reasoning {
        #[serde(deserialize_with = "identifier")]
        id: String,
        #[serde(default)]
        summary: Vec<String>,
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
            } => (
                id,
                FileChange::normalize(changes, root, matches!(status, PatchStatus::Completed)),
            ),
            Self::CommandExecution {
                id,
                actions,
                command,
                status,
                exit_code,
                output,
            } => {
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
                let title = command.map_or_else(|| "Shell command".into(), ShellCommand::label);
                let mut activity = Details::new(Kind::Command, title);
                activity.failed = status.is_some_and(StepStatus::failed)
                    || exit_code.is_some_and(|code| code != 0);
                activity.body = output.unwrap_or_default();
                if let Some(code) = exit_code {
                    activity.title = format!("{} · exit {code}", activity.title);
                }
                (id, Item::Read { files, activity })
            }
            Self::McpToolCall {
                id,
                arguments,
                server,
                tool,
                status,
            }
            | Self::DynamicToolCall {
                id,
                arguments,
                namespace: server,
                tool,
                status,
            } => {
                let tool = tool.unwrap_or_else(|| "Tool call".into());
                let title =
                    server.map_or_else(|| tool.clone(), |server| format!("{server}/{tool}"));
                let mut activity = Details::new(Kind::Tool, title);
                activity.failed = status.is_some_and(StepStatus::failed);
                (
                    id,
                    Item::Read {
                        files: mcp_location(root, arguments).into_iter().collect(),
                        activity,
                    },
                )
            }
            Self::CollabAgentToolCall { id, agents, tool } => {
                (id, Item::Collaboration { agents, tool })
            }
            Self::SubAgentActivity { id, thread, kind } => {
                let (title, status) = match kind {
                    SubagentAction::Started => ("Subagent started", AgentStatus::Running),
                    SubagentAction::Interacted => ("Subagent received input", AgentStatus::Running),
                    SubagentAction::Interrupted => ("Subagent interrupted", AgentStatus::Shutdown),
                    SubagentAction::Completed => ("Subagent finished", AgentStatus::Completed),
                    SubagentAction::Other => return None,
                };
                (
                    id,
                    Item::Collaboration {
                        agents: HashMap::from([(thread, AgentState { status })]),
                        tool: Some(title.into()),
                    },
                )
            }
            Self::ImageView { id, path } => (
                id,
                Item::Read {
                    files: relative(root, &path)
                        .map(|path| Location { path, line: 1 })
                        .into_iter()
                        .collect(),
                    activity: Details::new(Kind::Tool, "Open image"),
                },
            ),
            Self::WebSearch { id, query } => (
                id,
                Item::Activity(Details::new(
                    Kind::Search,
                    query.unwrap_or_else(|| "Web search".into()),
                )),
            ),
            Self::Plan { id, text } => {
                let mut activity = Details::new(Kind::Plan, "Plan");
                activity.body = text.unwrap_or_default();
                (id, Item::Activity(activity))
            }
            Self::Reasoning { id, summary } => {
                let mut activity = Details::new(Kind::Reasoning, "Progress summary");
                activity.body = summary.into_iter().take(24).collect::<Vec<_>>().join("\n");
                (id, Item::Activity(activity))
            }
            Self::Other => return None,
        };
        Some(normalized)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum SubagentAction {
    Started,
    Interacted,
    Interrupted,
    Completed,
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ShellCommand {
    Text(String),
    Arguments(Vec<String>),
}
impl ShellCommand {
    fn label(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Arguments(args) => args.into_iter().take(32).collect::<Vec<_>>().join(" "),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum StepStatus {
    Failed,
    Declined,
    Interrupted,
    Errored,
    #[serde(other)]
    Other,
}
impl StepStatus {
    fn failed(self) -> bool {
        !matches!(self, Self::Other)
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

impl FileChange {
    fn normalize(changes: Vec<Self>, root: &Path, applied: bool) -> Item {
        let detail = changes
            .iter()
            .take(12)
            .flat_map(|change| {
                change
                    .path
                    .chars()
                    .chain(['\n'])
                    .chain(change.diff.chars())
                    .chain(['\n'])
            })
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
        Item::Edit {
            files,
            applied,
            detail,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchUpdate {
    #[serde(deserialize_with = "identifier")]
    thread_id: String,
    #[serde(deserialize_with = "identifier")]
    item_id: String,
    changes: Vec<FileChange>,
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

    #[test]
    fn plan_and_public_reasoning_deltas_are_visible_activity() {
        let params = json!({"threadId":"main","itemId":"action","delta":"Read the file"});
        for method in ["item/plan/delta", "item/reasoning/summaryTextDelta"] {
            assert!(
                notification(Path::new("/repo"), method, params.clone())
                    .unwrap()
                    .is_some(),
                "{method}"
            );
        }
        assert!(
            notification(Path::new("/repo"), "item/reasoning/textDelta", params)
                .unwrap()
                .is_none()
        );
    }

    fn event(method: &str, params: Value) -> Result<Option<Notification>, String> {
        notification(Path::new("/repo"), method, params)
    }

    #[test]
    fn an_in_flight_patch_reports_the_changed_line_before_completion() {
        let patch = event(
            "item/fileChange/patchUpdated",
            json!({"threadId":"main","turnId":"turn","itemId":"patch","changes":[{
                "path":"src/state.rs","kind":{"type":"update"},"diff":"@@ -8 +12 @@\n-old\n+new"
            }]}),
        )
        .unwrap()
        .unwrap();
        let Update::Item {
            id,
            completed,
            content: Item::Edit { files, .. },
        } = patch.update
        else {
            panic!("expected a file edit");
        };
        assert_eq!(id, "patch");
        assert!(!completed);
        assert_eq!(
            files,
            [Location {
                path: "src/state.rs".into(),
                line: 12
            }]
        );
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
            content: Item::Read { files, .. },
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
                matches!(value.update, Update::Item { content: Item::Read { files, .. }, .. } if files.is_empty())
            );
        }
        let value = event("item/started", json!({"threadId":"main","item":{"id":"mcp","type":"mcpToolCall","arguments":{"path":"src/main.rs","line":23}}})).unwrap().unwrap();
        assert!(
            matches!(value.update, Update::Item { content: Item::Read { files, .. }, .. } if files[0].line == 23)
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
