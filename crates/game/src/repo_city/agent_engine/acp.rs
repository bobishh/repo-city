//! Desktop ACP client for Grok, Agy and Muse. Ecky 9ea94d7 informed the Grok handshake.
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
    Models(Result<super::models::Catalog, String>),
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
                Update::Models(models) => events.push(Event::Models(models)),
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
        let args: &[&str] = match provider.id {
            "agy" => &["acp", "--sandbox"],
            "grok" => &["agent", "stdio"],
            "muse" => &["acp"],
            _ => return Err(format!("Unsupported ACP provider {}.", provider.label)),
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
    let servers = if provider == super::MUSE {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not locate Repo City's MCP server: {error}"))?;
        json!([{"name":"repo_city","command":executable,"args":["--notes-mcp","--repo",start.root],"env":[]}])
    } else {
        json!([])
    };
    let mut params = json!({"cwd":start.root,"mcpServers":servers});
    let (session, opened) = if let Some(session) = start.conversation {
        params["sessionId"] = json!(session);
        let opened = wire.call("session/load", params.clone(), stopped)?;
        (session, opened)
    } else {
        let opened = wire.call("session/new", params.clone(), stopped)?;
        let session = opened["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Agent did not return a session ID.")?
            .to_owned();
        (session, opened)
    };
    params["sessionId"] = json!(session);
    let catalog = super::models::acp(&opened);
    let catalog_error = catalog.as_ref().err().cloned();
    let mut models = catalog.ok().flatten();
    let mut model_error = None;
    if let Some(requested) = start.model {
        match models
            .as_mut()
            .ok_or_else(|| "Provider did not return a model catalog.".to_owned())
            .and_then(|models| select_model(&mut wire, &session, models, &requested, stopped))
        {
            Ok(()) => {}
            Err(error) => model_error = Some(error),
        }
    }
    send(
        updates,
        Update::Models(models.as_ref().map(|m| m.catalog.clone()).ok_or_else(|| {
            catalog_error.unwrap_or_else(|| "Provider did not return a model catalog.".into())
        })),
    )?;
    if let Some(error) = &model_error {
        send(updates, Update::Models(Err(error.clone())))?;
    }
    send(updates, Update::Event(Event::Connected(session.clone())))?;
    let mut turn: Option<Turn> = None;
    let mut permissions = HashMap::new();
    while !stopped.load(Ordering::Acquire) {
        match commands.try_recv() {
            Ok(Command::RefreshModels) if turn.is_none() => {
                let refreshed = wire
                    .call("session/load", params.clone(), stopped)
                    .and_then(|value| super::models::acp(&value))
                    .and_then(|value| {
                        value.ok_or_else(|| "Provider did not return a model catalog.".to_owned())
                    });
                match refreshed {
                    Ok(refreshed) => {
                        send(updates, Update::Models(Ok(refreshed.catalog.clone())))?;
                        models = Some(refreshed);
                    }
                    Err(error) => send(updates, Update::Models(Err(error)))?,
                }
            }
            Ok(Command::SelectModel(id)) if turn.is_none() => {
                let result = models
                    .as_mut()
                    .ok_or_else(|| "Load the provider's model catalog first.".to_owned())
                    .and_then(|models| {
                        select_model(&mut wire, &session, models, &id, stopped)?;
                        Ok(models.catalog.clone())
                    });
                if result.is_ok() {
                    model_error = None;
                }
                send(updates, Update::Models(result))?;
            }
            Ok(Command::RefreshModels | Command::SelectModel(_)) => {
                send(
                    updates,
                    Update::Models(Err(
                        "Wait for the current turn to finish before changing models.".into(),
                    )),
                )?;
            }
            Ok(Command::Send(prompt)) if turn.is_none() && model_error.is_some() => {
                send(
                    updates,
                    Update::Event(Event::Delivery {
                        id: prompt.id,
                        result: Err(model_error.clone().unwrap()),
                    }),
                )?;
            }
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
        if method == Some("session/update") && in_session {
            match super::models::acp(&params["update"]) {
                Ok(Some(refreshed)) => {
                    send(updates, Update::Models(Ok(refreshed.catalog.clone())))?;
                    models = Some(refreshed);
                }
                Err(error) => send(updates, Update::Models(Err(error)))?,
                Ok(None) => {}
            }
        }
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

fn select_model(
    wire: &mut Wire,
    session: &str,
    models: &mut super::models::AcpCatalog,
    id: &str,
    stopped: &AtomicBool,
) -> Result<(), String> {
    if !models.catalog.contains(id) {
        return Err("Choose a model returned by this provider.".into());
    }
    let result = if let Some(config) = &models.config_id {
        wire.call(
            "session/set_config_option",
            json!({"sessionId":session,"configId":config,"value":id}),
            stopped,
        )?
    } else {
        wire.call(
            "session/set_model",
            json!({"sessionId":session,"modelId":id}),
            stopped,
        )?
    };
    if let Some(updated) = super::models::acp(&result)? {
        if updated.catalog.current.as_deref() != Some(id) {
            return Err("Provider did not confirm the selected model.".into());
        }
        *models = updated;
    } else {
        models.catalog.current = Some(id.into());
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
    fn model_selection_validates_ids_and_keeps_the_confirmed_model_on_rejection() {
        let root = std::env::temp_dir().join(format!(
            "city-acp-models-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let binary = root.join("agent");
        for config in [true, false] {
            let script = r#"#!/usr/bin/env python3
import json, sys
config = CONFIG
for line in sys.stdin:
    request = json.loads(line)
    assert request['method'] == ('session/set_config_option' if config else 'session/set_model')
    params = request['params']
    assert params['sessionId'] == 'session'
    if config: assert params['configId'] == 'model'
    selected = params['value' if config else 'modelId']
    if selected == 'refused':
        reply = {'error': {'message':'Model rejected'}}
    else:
        assert selected == 'next'
        # Legacy ACP permits an empty successful acknowledgement.
        reply = {'result': {'configOptions':[{'id':'model','type':'select','currentValue':selected,'options':[{'value':'next','name':'Next'}]}]} if config else {}}
    print(json.dumps({'id':request['id'], **reply}), flush=True)
"#.replace("CONFIG", if config { "True" } else { "False" });
            std::fs::write(&binary, script).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut wire = Wire::open(&binary, super::super::MUSE, &root).unwrap();
            let mut models = super::super::models::AcpCatalog {
                catalog: super::super::models::Catalog {
                    choices: ["current", "refused", "next"]
                        .into_iter()
                        .map(|id| super::super::models::Model {
                            id: id.into(),
                            name: id.into(),
                        })
                        .collect(),
                    current: Some("current".into()),
                },
                config_id: config.then(|| "model".into()),
            };
            let stop = AtomicBool::new(false);
            assert!(
                select_model(&mut wire, "session", &mut models, "invented", &stop)
                    .unwrap_err()
                    .contains("returned by this provider")
            );
            assert_eq!(
                select_model(&mut wire, "session", &mut models, "refused", &stop).unwrap_err(),
                "Model rejected"
            );
            assert_eq!(models.catalog.current.as_deref(), Some("current"));
            select_model(&mut wire, "session", &mut models, "next", &stop).unwrap();
            assert_eq!(models.catalog.current.as_deref(), Some("next"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cli_sessions_start_resume_and_cancel_with_tool_decisions() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("city-acp-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let cli = root.join("agent");
        std::fs::write(&cli, r#"#!/usr/bin/env python3
import json, pathlib, sys
provider = pathlib.Path(sys.argv[0]).name
expected = {"agy": ["acp", "--sandbox"], "grok": ["agent", "stdio"], "muse": ["acp"]}
assert sys.argv[1:] == expected[provider], (provider, sys.argv[1:])
def emit(**value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)
init = json.loads(input())
assert init["method"] == "initialize"
emit(id=init["id"], result={"protocolVersion": 1, "authMethods": []})
load = json.loads(input())
assert load["method"] in ("session/load", "session/new")
session = "saved" if load["method"] == "session/load" else "created"
if session == "saved":
    assert load["params"]["sessionId"] == session
servers = load["params"]["mcpServers"]
if provider == "muse":
    assert len(servers) == 1 and servers[0]["name"] == "repo_city"
    assert pathlib.Path(servers[0]["command"]).is_absolute()
    assert servers[0]["args"][:2] == ["--notes-mcp", "--repo"]
    assert pathlib.Path(servers[0]["args"][2]).resolve() == pathlib.Path.cwd()
    assert servers[0]["env"] == []
else:
    assert servers == []
def catalog(current):
    if provider == "grok":
        return {"models":{"currentModelId":current,"availableModels":[{"modelId":"default","name":"Default"},{"modelId":"requested","name":"Requested"}]}}
    return {"configOptions":[{"id":"model","type":"select","category":"model","currentValue":current,"options":[{"value":"default","name":"Default"},{"value":"requested","name":"Requested"}]}]}
emit(id=load["id"], result={"sessionId":session, **catalog("default")})
selection = json.loads(input())
assert selection["method"] == ("session/set_model" if provider == "grok" else "session/set_config_option")
assert selection["params"]["sessionId"] == session
assert selection["params"].get("modelId", selection["params"].get("value")) == "requested"
emit(id=selection["id"], result=catalog("requested"))
prompt = json.loads(input())
assert prompt["method"] == "session/prompt"
assert prompt["params"]["sessionId"] == session
text = prompt["params"]["prompt"][0]["text"]
assert text in ("camera\n\nRead the file", "camera\n\nStop after denied tool")
denied = text.endswith("Stop after denied tool")
emit(method="session/request_permission", id=99, params={"sessionId":session, "toolCall":{"title":"Read the file"}, "options":[{"optionId":"one","kind":"allow_once"},{"optionId":"all","kind":"allow_always"},{"optionId":"no","kind":"reject_once"}]})
decision = json.loads(input())
assert decision["id"] == 99 and decision["result"]["outcome"] == {"outcome":"selected","optionId":"no" if denied else "one"}
if denied:
    emit(method="session/update", params={"sessionId":session,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Permission denied"}}})
    cancel = json.loads(input())
    assert cancel["method"] == "session/cancel" and cancel["params"]["sessionId"] == session
    emit(id=prompt["id"], result={"stopReason":"cancelled"})
else:
    emit(method="session/update", params={"sessionId":session, "update":{"sessionUpdate":"tool_call", "locations":[{"path":"src/main.rs","line":7}]}})
    emit(method="session/update", params={"sessionId":session, "update":{"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"Answer"}}})
    emit(id=prompt["id"], result={"stopReason":"end_turn"})
sys.stdin.read()
"#).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        for (provider, restore, deny) in [
            (super::super::AGY, false, false),
            (super::super::AGY, true, false),
            (super::super::GROK, false, false),
            (super::super::GROK, true, false),
            (super::super::MUSE, false, false),
            (super::super::MUSE, true, false),
            (super::super::MUSE, false, true),
        ] {
            let binary = root.join(provider.id);
            if !binary.exists() {
                std::fs::hard_link(&cli, &binary).unwrap();
            }
            let wire = Wire::open(&binary, provider, &root).unwrap();
            let (commands, input) = mpsc::sync_channel(4);
            let (updates, events) = mpsc::sync_channel(16);
            let start = Start {
                model: Some("requested".into()),
                root: root.clone(),
                conversation: restore.then(|| "saved".into()),
                approval_mode: super::super::ApprovalMode::Ask,
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
                    text: if deny {
                        "Stop after denied tool"
                    } else {
                        "Read the file"
                    }
                    .into(),
                }))
                .unwrap();
            let (mut connected, mut delivered, mut file, mut answer, mut model) =
                (false, false, false, false, false);
            while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
                match event {
                    Update::Models(Ok(catalog)) => {
                        model = catalog.current.as_deref() == Some("requested")
                            && catalog.choices.len() == 2
                    }
                    Update::Event(Event::Connected(id)) => {
                        connected = id == if restore { "saved" } else { "created" }
                    }
                    Update::Event(Event::Delivery { id, result }) => {
                        delivered = id == "prompt" && result.is_ok()
                    }
                    Update::Event(Event::Approval { token, title }) => {
                        assert_eq!(title, "Read the file");
                        commands
                            .send(Command::Approval {
                                token,
                                accept: !deny,
                            })
                            .unwrap();
                    }
                    Update::File { path, line } => file = path == "src/main.rs" && line == 7,
                    Update::Delta { text, .. } => {
                        answer = text == if deny { "Permission denied" } else { "Answer" };
                        if deny {
                            commands.send(Command::Interrupt).unwrap();
                        }
                    }
                    Update::Turn(None, status) => {
                        assert_eq!(status, if deny { "Work stopped" } else { "Connected" });
                        break;
                    }
                    _ => {}
                }
            }
            drop(commands);
            let outcome = worker.join().unwrap();
            assert!(outcome.is_ok(), "{}: {outcome:?}", provider.label);
            assert!(
                connected && delivered && (file || deny) && answer && model,
                "{}",
                provider.label
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
