//! Desktop ACP client for Grok and Agy. Ecky 9ea94d7 informed the Grok handshake.
use super::{
    Command, Engine, Event, Projection, Prompt, Provider, Start,
    activity::{Details, Kind, Location, Phase},
    state::FileActivity,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Component, Path, PathBuf},
    process::{Child, ChildStdin, Command as Process, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Adapter {
    provider: Provider,
    commands: SyncSender<Command>,
    events: Mutex<Receiver<Update>>,
    stopped: Arc<AtomicBool>,
}

enum Update {
    Event(Event),
    Turn(Option<String>, &'static str),
    Delta {
        id: String,
        text: String,
    },
    File {
        path: String,
        line: usize,
    },
    Activity {
        thread: String,
        id: String,
        details: Details,
        phase: Phase,
    },
}

impl Adapter {
    pub fn start(provider: Provider, start: Start) -> Result<Self, String> {
        let binary = executable(provider)
            .ok_or_else(|| format!("Install the {} CLI and sign in to connect.", provider.label))?;
        let (commands, input) = mpsc::sync_channel(16);
        let (updates, events) = mpsc::sync_channel(128);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        thread::spawn(move || {
            let result = Wire::open(&binary, provider, &start.root)
                .and_then(|wire| run(wire, provider, start, &input, &updates, &worker_stop));
            if let Err(error) = result {
                let _ = updates.send(Update::Event(Event::Error(error)));
            }
            let _ = updates.send(Update::Event(Event::Closed));
        });
        Ok(Self {
            provider,
            commands,
            events: Mutex::new(events),
            stopped,
        })
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

impl Engine for Adapter {
    fn provider(&self) -> Provider {
        self.provider
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| format!("Could not queue {} request: {error}", self.provider.label))
    }

    fn poll(&self, projection: &mut Projection) -> Vec<Event> {
        let Ok(updates) = self.events.lock() else {
            return vec![Event::Closed];
        };
        let mut events = Vec::new();
        for update in updates.try_iter().take(64) {
            match update {
                Update::Event(event) => events.push(event),
                Update::Turn(turn, status) => {
                    if turn.is_none()
                        && let Some(thread) = &projection.dialogue.thread
                    {
                        projection.timeline.end_turn(thread);
                    }
                    projection.turn = turn;
                    events.push(Event::Changed {
                        status: Some(status.into()),
                    });
                }
                Update::Delta { id, text } => {
                    projection.dialogue.message(&id, "assistant", &text, true);
                    events.push(Event::Changed { status: None });
                }
                Update::File { path, line } => {
                    let serial = projection
                        .activity
                        .as_ref()
                        .map_or(1, |activity| activity.serial.wrapping_add(1));
                    projection.activity = Some(FileActivity { serial, path, line });
                    events.push(Event::Changed { status: None });
                }
                Update::Activity {
                    thread,
                    id,
                    details,
                    phase,
                } => projection.timeline.record(&thread, &id, details, phase),
            }
        }
        events
    }
}

fn executable(provider: Provider) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{}.exe", provider.id)
    } else {
        provider.id.into()
    };
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .chain(home.iter().flat_map(|home| {
            [
                Path::new(home).join(".local/bin"),
                Path::new(home).join(".grok/bin"),
            ]
        }))
        .chain([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .map(|path| path.join(&filename))
        .find(|path| path.is_file())
}

struct Wire {
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<Value, String>>,
    next: u64,
}

impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Wire {
    fn open(binary: &Path, provider: Provider, root: &Path) -> Result<Self, String> {
        let args = if provider == super::AGY {
            ["acp", "--sandbox"]
        } else {
            ["agent", "stdio"]
        };
        let mut child = Process::new(binary)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start {}: {error}", provider.label))?;
        let input = child.stdin.take().expect("piped CLI stdin");
        let stdout = child.stdout.take().expect("piped CLI stdout");
        let (send, output) = mpsc::sync_channel(32);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let message = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|error| format!("Invalid agent JSON: {error}"))
                });
                if send.send(message).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            output,
            next: 1,
        })
    }

    fn write(&mut self, value: Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.input, &value).map_err(|error| error.to_string())?;
        self.input
            .write_all(b"\n")
            .and_then(|()| self.input.flush())
            .map_err(|error| error.to_string())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<u64, String> {
        let id = self.next;
        self.next += 1;
        self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        Ok(id)
    }

    fn read(&self) -> Result<Option<Value>, String> {
        match self.output.recv_timeout(Duration::from_millis(25)) {
            Ok(value) => value.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Agent CLI disconnected. Check its login and reconnect.".into())
            }
        }
    }

    fn call(&mut self, method: &str, params: Value, stopped: &AtomicBool) -> Result<Value, String> {
        let id = self.request(method, params)?;
        let started = Instant::now();
        loop {
            if stopped.load(Ordering::Acquire) {
                return Err("Agent connection canceled.".into());
            }
            if started.elapsed() > Duration::from_secs(30) {
                return Err(format!(
                    "Agent {method} did not respond. Check the CLI login."
                ));
            }
            if let Some(frame) = self.read()? {
                if frame.get("method").is_none() && frame["id"].as_u64() == Some(id) {
                    return response(frame);
                }
                if frame.get("method").is_some()
                    && let Some(id) = frame.get("id")
                {
                    self.reject_request(id)?;
                }
            }
        }
    }

    fn reject_request(&mut self, id: &Value) -> Result<(), String> {
        self.write(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Client method unavailable"}}))
    }
}

fn response(frame: Value) -> Result<Value, String> {
    if let Some(error) = frame.get("error") {
        return Err(error["message"]
            .as_str()
            .unwrap_or("Agent request failed.")
            .chars()
            .take(1000)
            .collect());
    }
    frame
        .get("result")
        .cloned()
        .ok_or_else(|| "Agent response did not contain a result.".into())
}

fn send(updates: &SyncSender<Update>, update: Update) -> Result<(), String> {
    updates
        .send(update)
        .map_err(|_| "Agent view closed.".into())
}

struct Turn {
    request: u64,
    prompt: Prompt,
    acknowledged: bool,
    stopping: Option<Instant>,
}

impl Turn {
    fn delivered(
        &mut self,
        updates: &SyncSender<Update>,
        result: Result<(), String>,
    ) -> Result<(), String> {
        if !self.acknowledged {
            send(
                updates,
                Update::Event(Event::Delivery {
                    id: self.prompt.id.clone(),
                    result,
                }),
            )?;
            self.acknowledged = true;
        }
        Ok(())
    }
}

struct Permission {
    id: Value,
    allow: Option<String>,
    reject: Option<String>,
}

impl Permission {
    fn option(params: &Value, kind: &str) -> Option<String> {
        params["options"]
            .as_array()?
            .iter()
            .find(|option| option["kind"] == kind)?["optionId"]
            .as_str()
            .map(str::to_owned)
    }

    fn answer(self, accept: bool, wire: &mut Wire) -> Result<(), String> {
        let outcome = match if accept { self.allow } else { self.reject } {
            Some(option) => json!({"outcome":"selected","optionId":option}),
            None => json!({"outcome":"cancelled"}),
        };
        wire.write(json!({"jsonrpc":"2.0","id":self.id,"result":{"outcome":outcome}}))
    }
}

fn run(
    mut wire: Wire,
    provider: Provider,
    start: Start,
    commands: &Receiver<Command>,
    updates: &SyncSender<Update>,
    stopped: &AtomicBool,
) -> Result<(), String> {
    let init = wire.call(
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{}}),
        stopped,
    )?;
    if provider == super::GROK
        && let Some(methods) = init["authMethods"].as_array()
    {
        let has = |id: &str| methods.iter().any(|method| method["id"] == id);
        let method = if has("cached_token") {
            Some("cached_token")
        } else if has("xai.api_key") && std::env::var_os("XAI_API_KEY").is_some() {
            Some("xai.api_key")
        } else if has("grok.com") && grok_has_login() {
            Some("grok.com")
        } else {
            None
        };
        if let Some(method) = method {
            wire.call("authenticate", json!({"methodId":method}), stopped)?;
        } else if !methods.is_empty() {
            return Err("Grok needs a CLI login. Run `grok login`, then reconnect.".into());
        }
    }
    let mut params = json!({"cwd":start.root,"mcpServers":[]});
    let session = if let Some(session) = start.conversation {
        params["sessionId"] = json!(session);
        wire.call("session/load", params, stopped)?;
        session
    } else {
        wire.call("session/new", params, stopped)?["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Agent did not return a session ID.")?
            .to_owned()
    };
    send(updates, Update::Event(Event::Connected(session.clone())))?;
    let mut turn: Option<Turn> = None;
    let mut permissions = HashMap::new();
    while !stopped.load(Ordering::Acquire) {
        match commands.try_recv() {
            Ok(Command::Send(prompt)) if turn.is_none() => {
                let text = format!("{}\n\n{}", prompt.context, prompt.text);
                let request = wire.request(
                    "session/prompt",
                    json!({"sessionId":session,"prompt":[{"type":"text","text":text}]}),
                )?;
                send(updates, Update::Turn(Some(prompt.id.clone()), "Working"))?;
                turn = Some(Turn {
                    request,
                    prompt,
                    acknowledged: false,
                    stopping: None,
                });
            }
            Ok(Command::Send(prompt) | Command::Steer(prompt)) => {
                send(
                    updates,
                    Update::Event(Event::Delivery {
                        id: prompt.id,
                        result: Err("Wait for the current turn to finish.".into()),
                    }),
                )?;
            }
            Ok(Command::Interrupt) => match &mut turn {
                Some(turn) if turn.stopping.is_none() => {
                    wire.write(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}))?;
                    turn.stopping = Some(Instant::now());
                }
                None => send(
                    updates,
                    Update::Event(Event::StopUnconfirmed("No active turn to stop.".into())),
                )?,
                _ => {}
            },
            Ok(Command::Approval { token, accept }) => {
                if let Some(permission) = permissions.remove(&token) {
                    Permission::answer(permission, accept, &mut wire)?;
                }
            }
            Ok(Command::Voice { .. } | Command::StopVoice) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if turn
            .as_ref()
            .and_then(|turn| turn.stopping)
            .is_some_and(|stop| stop.elapsed() > Duration::from_secs(3))
        {
            return Err("Agent did not confirm stop and was disconnected.".into());
        }
        let Some(frame) = wire.read()? else { continue };
        let method = frame["method"].as_str();
        let params = &frame["params"];
        let in_session = params["sessionId"].as_str() == Some(session.as_str());
        match (method, frame.get("id"), &mut turn) {
            (Some("session/request_permission"), Some(id), Some(turn)) if in_session => {
                turn.delivered(updates, Ok(()))?;
                let token = id.to_string();
                permissions.insert(
                    token.clone(),
                    Permission {
                        id: id.clone(),
                        allow: Permission::option(params, "allow_once"),
                        reject: Permission::option(params, "reject_once"),
                    },
                );
                let title = params["toolCall"]["title"]
                    .as_str()
                    .unwrap_or("Allow this agent tool call?")
                    .chars()
                    .take(300)
                    .collect();
                send(updates, Update::Event(Event::Approval { token, title }))?;
            }
            (Some(_), Some(id), _) => wire.reject_request(id)?,
            (Some("session/update"), None, Some(turn)) if in_session => {
                turn.delivered(updates, Ok(()))?;
                project(
                    &params["update"],
                    &start.root,
                    &session,
                    &turn.prompt.id,
                    updates,
                )?;
            }
            (None, Some(id), Some(active)) if id.as_u64() == Some(active.request) => {
                let result = response(frame);
                let canceled = result
                    .as_ref()
                    .is_ok_and(|result| result["stopReason"] == "cancelled");
                let result = result.map(|_| ());
                send(
                    updates,
                    Update::Turn(
                        None,
                        if canceled {
                            "Work stopped"
                        } else {
                            "Connected"
                        },
                    ),
                )?;
                if active.acknowledged
                    && let Err(error) = &result
                {
                    send(updates, Update::Event(Event::Error(error.clone())))?;
                }
                active.delivered(updates, result)?;
                turn = None;
                permissions.clear();
            }
            _ => {}
        }
    }
    Ok(())
}

fn grok_has_login() -> bool {
    std::env::var_os("GROK_DEPLOYMENT_KEY").is_some()
        || std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .is_some_and(|home| Path::new(&home).join(".grok/auth.json").is_file())
}

fn project(
    value: &Value,
    root: &Path,
    session: &str,
    prompt: &str,
    updates: &SyncSender<Update>,
) -> Result<(), String> {
    match value["sessionUpdate"].as_str() {
        Some("agent_message_chunk") if value["content"]["type"] == "text" => {
            if let Some(text) = value["content"]["text"].as_str() {
                send(
                    updates,
                    Update::Delta {
                        id: format!("assistant:{prompt}"),
                        text: text.into(),
                    },
                )?;
            }
        }
        Some("tool_call" | "tool_call_update") => {
            let Ok(tool) = serde_json::from_value::<ToolUpdate>(value.clone()) else {
                return Ok(());
            };
            let kind = match tool.kind.as_deref() {
                Some("edit") => Kind::Edit,
                Some("execute" | "read") => Kind::Command,
                Some("search") => Kind::Search,
                _ => Kind::Tool,
            };
            let phase = match tool.status.as_deref() {
                Some("completed") => Phase::Completed,
                Some("failed") => Phase::Failed,
                _ => Phase::Running,
            };
            let mut details = Details::new(kind, tool.title.unwrap_or_default());
            for position in tool.locations.into_iter().take(16) {
                let Some(path) = repo_path(root, &position.path) else {
                    continue;
                };
                let line = position.line.unwrap_or(1).clamp(1, 1_000_000);
                if phase == Phase::Running {
                    send(
                        updates,
                        Update::File {
                            path: path.clone(),
                            line,
                        },
                    )?;
                }
                details.locations.push(Location { path, line });
            }
            if let Some(id) = tool.tool_call_id.filter(|id| !id.is_empty()) {
                send(
                    updates,
                    Update::Activity {
                        thread: session.into(),
                        id,
                        details,
                        phase,
                    },
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolUpdate {
    tool_call_id: Option<String>,
    title: Option<String>,
    status: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    locations: Vec<ToolLocation>,
}

#[derive(Deserialize)]
struct ToolLocation {
    path: String,
    line: Option<usize>,
}

fn repo_path(root: &Path, path: &str) -> Option<String> {
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let parts = path
        .components()
        .filter(|part| *part != Component::CurDir)
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, time::SystemTime};

    #[test]
    fn cli_session_resumes_prompts_as_text_and_waits_for_a_tool_decision() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("city-acp-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let cli = root.join("agent");
        std::fs::write(&cli, r#"#!/usr/bin/env python3
import json, sys
assert sys.argv[1:] in (["agent", "stdio"], ["acp", "--sandbox"])
def emit(**value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)
init = json.loads(input())
assert init["method"] == "initialize"
emit(id=init["id"], result={"protocolVersion": 1, "authMethods": []})
load = json.loads(input())
assert load["method"] == "session/load" and load["params"]["sessionId"] == "saved"
emit(id=load["id"], result={})
prompt = json.loads(input())
assert prompt["method"] == "session/prompt" and prompt["params"]["prompt"][0]["text"] == "camera\n\nRead the file"
emit(method="session/request_permission", id=99, params={"sessionId":"saved", "toolCall":{"title":"Read the file"}, "options":[{"optionId":"one","kind":"allow_once"},{"optionId":"all","kind":"allow_always"}]})
decision = json.loads(input())
assert decision["id"] == 99 and decision["result"]["outcome"] == {"outcome":"selected","optionId":"one"}
emit(method="session/update", params={"sessionId":"saved", "update":{"sessionUpdate":"tool_call", "locations":[{"path":"src/main.rs","line":7}]}})
emit(method="session/update", params={"sessionId":"saved", "update":{"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"Answer"}}})
emit(id=prompt["id"], result={"stopReason":"end_turn"})
sys.stdin.read()
"#).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        for provider in [super::super::AGY, super::super::GROK] {
            let wire = Wire::open(&cli, provider, &root).unwrap();
            let (commands, input) = mpsc::sync_channel(4);
            let (updates, events) = mpsc::sync_channel(16);
            let start = Start {
                root: root.clone(),
                conversation: Some("saved".into()),
                automatic_review: false,
                speaking: Arc::default(),
                diagnostics: None,
            };
            let worker = thread::spawn(move || {
                run(
                    wire,
                    provider,
                    start,
                    &input,
                    &updates,
                    &AtomicBool::new(false),
                )
            });
            commands
                .send(Command::Send(Prompt {
                    id: "prompt".into(),
                    context: "camera".into(),
                    text: "Read the file".into(),
                }))
                .unwrap();
            let (mut connected, mut delivered, mut file, mut answer) = (false, false, false, false);
            while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
                match event {
                    Update::Event(Event::Connected(id)) => connected = id == "saved",
                    Update::Event(Event::Delivery { id, result }) => {
                        delivered = id == "prompt" && result.is_ok()
                    }
                    Update::Event(Event::Approval { token, title }) => {
                        assert_eq!(title, "Read the file");
                        commands
                            .send(Command::Approval {
                                token,
                                accept: true,
                            })
                            .unwrap();
                    }
                    Update::File { path, line } => file = path == "src/main.rs" && line == 7,
                    Update::Delta { text, .. } => answer = text == "Answer",
                    Update::Turn(None, _) => break,
                    _ => {}
                }
            }
            drop(commands);
            assert!(worker.join().unwrap().is_ok());
            assert!(
                connected && delivered && file && answer,
                "{}",
                provider.label
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
