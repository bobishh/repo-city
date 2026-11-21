//! Desktop HTTP adapter for the same authorized rooms used by the browser.
//! Network work stays off the render thread; one request batch is in flight.
use super::*;
use bevy::{clipboard::Clipboard, input_focus::InputFocus, text::EditableText};
use serde_json::{Value, json};

const DEFAULT_SERVICE: &str = "https://repo-city.meta-uber-engineer.dev";

#[derive(Default)]
pub(super) struct State {
    room: Option<Value>,
    members: Vec<Value>,
    token: String,
    member_id: String,
    cursor: u64,
    pose_seq: u64,
    next: f64,
    renew: f64,
    pending: Option<(Task, native_github::Job<Value>)>,
    panel: Option<Entity>,
    roster: Option<Entity>,
    status_label: Option<Entity>,
    fields: Vec<Entity>,
    action: Option<Action>,
    status: String,
    invite: String,
    focus_enabled: bool,
    last_focus: Option<Focus>,
    cues: Vec<Value>,
    opening: bool,
    ice: Vec<Value>,
    media: Option<media::Client>,
    media_ready: bool,
    talking: bool,
    outgoing: std::collections::VecDeque<Value>,
    muted: HashSet<String>,
    poses: std::collections::HashMap<String, Value>,
    pub(super) chat: chat::Chat,
}
impl State {
    pub(super) fn connected(&self) -> bool {
        self.room.is_some() && !self.opening
    }
    pub(super) fn chat_room(&self) -> Option<&str> {
        self.room.as_ref().filter(|_| !self.opening)?["id"].as_str()
    }
    pub(super) fn hosts_room(&self) -> bool {
        self.connected()
            && self.room.as_ref().and_then(|room| room["host_id"].as_str())
                == Some(self.member_id.as_str())
    }
    pub(super) fn post_event(&self, body: Value) -> Result<native_github::Job<Value>, String> {
        let path = room_path(self, "events")
            .filter(|_| self.connected())
            .ok_or("Connect to the room to share.")?;
        let token = self.token.clone();
        Ok(native_github::Job::spawn(move || {
            request("POST", &path, &token, Some(body))
        }))
    }
    pub(super) fn peers(&self) -> Vec<Value> {
        self.members
            .iter()
            .filter(|member| member["id"].as_str() != Some(self.member_id.as_str()))
            .map(|member| {
                let mut member = member.clone();
                if let Some(pose) = member["id"]
                    .as_str()
                    .and_then(|id| self.poses.get(id))
                    .cloned()
                {
                    member["pose"] = pose;
                }
                member
            })
            .collect()
    }
}
#[derive(Clone, Copy)]
enum Task {
    Create,
    Join,
    Invite,
    RevokeInvite,
    Poll,
    Renew,
    Leave,
    Close,
    Remove,
}
#[derive(Component, Clone, Copy)]
enum Action {
    Create,
    Join,
    Invite,
    RevokeInvite,
    Leave,
    Close,
    Dismiss,
    Copy,
    Focus,
    Voice,
    Mute(usize),
    ShareCue,
    Fly(usize),
    Cue(usize),
    Remove(usize),
}
impl native_panel::Actions<Action> for Party {
    fn activate(&mut self, action: Action) {
        self.native.action = Some(action);
    }
}

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, update.before(project));
}

fn service() -> Result<String, String> {
    let origin =
        std::env::var("VIBE_CITY_PARTY_SERVICE").unwrap_or_else(|_| DEFAULT_SERVICE.into());
    let origin = origin.trim_end_matches('/');
    if origin == DEFAULT_SERVICE
        || origin.starts_with("http://127.0.0.1:")
        || origin.starts_with("http://localhost:")
    {
        if !origin.contains(['?', '#', '@']) {
            return Ok(origin.into());
        }
    }
    Err("The room service must use the Repo City HTTPS origin or an isolated loopback port.".into())
}

fn request(method: &str, path: &str, token: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{}{path}", service()?);
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .redirects(0)
        .build();
    let req = agent
        .request(method, &url)
        .set("Authorization", &format!("Bearer {token}"));
    let response = match body {
        Some(body) => req.send_json(body),
        None => req.call(),
    };
    match response {
        Ok(response) => response
            .into_json()
            .map_err(|_| "The room service returned invalid JSON.".into()),
        Err(ureq::Error::Status(code, response)) => {
            let message = response
                .into_json::<Value>()
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned));
            Err(message.unwrap_or_else(|| format!("Room request failed ({code}).")))
        }
        Err(_) => Err("Could not reach the room service. Check your connection and retry.".into()),
    }
}

fn room_path(state: &State, suffix: &str) -> Option<String> {
    let id = state.room.as_ref()?["id"].as_str()?;
    Some(format!("/party/rooms/{id}/{suffix}"))
}
fn start(
    state: &mut State,
    task: Task,
    method: &str,
    path: String,
    token: String,
    body: Option<Value>,
) {
    if state.pending.is_some() {
        return;
    }
    let method = method.to_owned();
    state.pending = Some((
        task,
        native_github::Job::spawn(move || request(&method, &path, &token, body)),
    ));
}
fn invitation(input: &str) -> Result<(String, String), String> {
    let input = input.trim();
    let (address, fragment) = input.split_once('#').unwrap_or((input, ""));
    let query = address.split_once('?').map_or(address, |(_, value)| value);
    let pairs: std::collections::HashMap<_, _> = query
        .split('&')
        .chain(
            fragment
                .split('&')
                .filter(|part| part.starts_with("invite=")),
        )
        .filter_map(|part| part.split_once('='))
        .collect();
    let id = pairs
        .get("party")
        .ok_or("Paste a Repo City invitation link.")?;
    let token = pairs
        .get("invite")
        .ok_or("This link has no invitation token.")?;
    if id.len() != 36
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        || token.len() > 200
        || token.is_empty()
    {
        return Err("Invalid room invitation.".into());
    }
    Ok(((*id).into(), (*token).into()))
}

fn comparison(repo: &Repository, target: &native_github::Target) -> Result<Value, String> {
    if repo.is_worktree() {
        return Err(
            "Open a pinned GitHub revision before creating a room. Worktree source stays local."
                .into(),
        );
    }
    let url = target.url();
    let parts: Vec<_> = url
        .trim_start_matches("https://github.com/")
        .split('/')
        .collect();
    let sha = |value: &str| {
        matches!(value.len(), 40 | 64) && value.bytes().all(|c| c.is_ascii_hexdigit())
    };
    if !sha(&repo.revision) || repo.baseline.as_deref().is_none_or(|base| !sha(base)) {
        return Err(
            "Choose a GitHub revision with a comparison base before creating a room.".into(),
        );
    }
    Ok(
        json!({"owner":parts[0],"repo":parts[1],"head_sha":repo.revision,"base_sha":repo.baseline,"layout_version":1,
        "pr_number":parts.get(3).and_then(|value| value.parse::<u64>().ok())}),
    )
}

fn pinned(repo: &Repository, room: &Value) -> bool {
    !repo.is_worktree()
        && room["comparison"]["head_sha"].as_str() == Some(repo.revision.as_str())
        && room["comparison"]["base_sha"].as_str() == repo.baseline.as_deref()
}

fn read_focus(value: &Value) -> Option<Focus> {
    let mut value = value.clone();
    let object = value.as_object_mut()?;
    let before = object.remove("side").as_ref().and_then(Value::as_str) == Some("before");
    object.insert("before".into(), Value::Bool(before));
    serde_json::from_value(value).ok()
}

fn write_focus(focus: &Option<Focus>) -> Value {
    let Some(focus) = focus else {
        return Value::Null;
    };
    json!({"path":focus.path,"side":if focus.before {"before"}else{"after"},"face":focus.face,"line":focus.line,"end_line":focus.end_line})
}

pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    mut party: ResMut<Party>,
    mut picker: ResMut<repository_picker::RepositoryPicker>,
    mut review: ResMut<review_notes::Review>,
    mut focus: ResMut<InputFocus>,
    parents: Query<&ChildOf>,
    fields: Query<&EditableText>,
    mut camera: Query<&mut Pilot, PlayerCamera>,
    mut clipboard: ResMut<Clipboard>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut labels: Query<&mut Text>,
    agent: Res<agent_session::Session>,
    mut last_follow_key: Local<Option<KeyCode>>,
) {
    let now = time.elapsed_secs_f64();
    let talk_key = if agent.follow_key() == KeyCode::F8 {
        KeyCode::F7
    } else {
        KeyCode::F8
    };
    let talk_label = if talk_key == KeyCode::F7 { "F7" } else { "F8" };
    let follow_key_changed = *last_follow_key != Some(agent.follow_key());
    *last_follow_key = Some(agent.follow_key());
    let toggle = keys.just_pressed(KeyCode::F3);
    let result = party
        .native
        .pending
        .as_ref()
        .and_then(|(task, job)| job.take().map(|result| (*task, result)));
    let polled = result
        .as_ref()
        .is_some_and(|(task, _)| matches!(task, Task::Poll | Task::Renew));
    let mut redraw = toggle
        || result
            .as_ref()
            .is_some_and(|(task, _)| !matches!(task, Task::Poll | Task::Renew));
    if let Some((task, result)) = result {
        party.native.pending = None;
        match result {
            Err(error) => {
                redraw = true;
                if matches!(task, Task::Poll | Task::Renew) {
                    party.native.room = None;
                    party.native.members.clear();
                    party.native.token.clear();
                    party.native.media = None;
                    party.native.media_ready = false;
                }
                party.native.status = error;
            }
            Ok(value) => {
                if matches!(task, Task::Create | Task::Join) {
                    party.native.chat = default();
                    party.native.invite.clear();
                    party.native.cues.clear();
                    party.native.poses.clear();
                    party.native.muted.clear();
                    party.native.outgoing.clear();
                }
                if matches!(task, Task::Create | Task::Join | Task::Renew) {
                    if let Some(token) = value["room_token"].as_str() {
                        party.native.token = token.into();
                    }
                    if let Some(id) = value["member"]["id"].as_str() {
                        party.native.member_id = id.into();
                    }
                    party.native.renew = now + 240.;
                    party.native.ice = value["ice_servers"].as_array().cloned().unwrap_or_default();
                    if let Some(media) = &party.native.media {
                        media.command(media::Command::Ice(party.native.ice.clone()));
                    }
                }
                if let Some(room) = value.get("room") {
                    if room["closed"] == true {
                        party.native.room = None;
                        party.native.members.clear();
                        party.native.token.clear();
                        party.native.media = None;
                        party.native.media_ready = false;
                        party.native.status = "Room closed. Solo mode.".into();
                    } else {
                        party.native.room = Some(room.clone());
                    }
                }
                if let Some(members) = value["members"].as_array() {
                    party.native.members = members.clone();
                    if let Some(media) = &party.native.media {
                        media.command(media::Command::Members(members.clone()));
                    }
                }
                if let Some(cursor) = value["cursor"].as_u64() {
                    party.native.cursor = cursor;
                }
                for event in value["events"].as_array().into_iter().flatten() {
                    if event["type"] == "chat" || event["type"] == "sketch" {
                        party.native.chat.receive(event);
                    }
                    if event["type"] == "pose" {
                        if let Some(from) = event["from"].as_str() {
                            party.native.poses.insert(from.into(), event.clone());
                        }
                    }
                    if event["type"] == "signal" {
                        if let (Some(media), Some(from)) =
                            (&party.native.media, event["from"].as_str())
                        {
                            media.command(media::Command::Signal {
                                from: from.into(),
                                signal: event["signal"].clone(),
                            });
                        }
                    }
                    if event["type"] == "cue" {
                        if party.native.cues.len() >= 32 {
                            party.native.cues.remove(0);
                        }
                        party.native.cues.push(event.clone());
                    }
                }
                match task {
                    Task::Create | Task::Join => {
                        review.auto_publish = false;
                        party.native.focus_enabled = false;
                        party.native.status =
                            "Connected. Focus sharing and microphone are off.".into();
                        if let Some(room) = party
                            .native
                            .room
                            .as_ref()
                            .filter(|room| !pinned(&repo.0, room))
                        {
                            party.native.status = match picker.open_party(&room["comparison"]) {
                                Ok(()) => "Loading the room's pinned comparison…".into(),
                                Err(error) => error,
                            };
                            party.native.opening = true;
                        }
                    }
                    Task::Invite => {
                        party.native.invite = value["invite_url"].as_str().unwrap_or("").into();
                        party.native.status =
                            "Invitation created for the verified GitHub account.".into();
                    }
                    Task::RevokeInvite => {
                        party.native.invite.clear();
                        party.native.status = "Invitation revoked.".into();
                    }
                    Task::Leave | Task::Close => {
                        party.native.opening = false;
                        party.native.room = None;
                        party.native.members.clear();
                        party.native.token.clear();
                        party.native.cues.clear();
                        party.native.media = None;
                        party.native.media_ready = false;
                        party.native.status =
                            "Solo mode. Private drafts remain on this device.".into();
                    }
                    _ => {}
                }
            }
        }
    }
    if party.native.opening && party.native.pending.is_none() {
        if party
            .native
            .room
            .as_ref()
            .is_some_and(|room| pinned(&repo.0, room))
        {
            party.native.opening = false;
            party.native.status = "Connected to the pinned comparison.".into();
            redraw = true;
        } else if !picker.party_loading()
            && let Some(path) = room_path(&party.native, "leave")
        {
            let token = party.native.token.clone();
            start(
                &mut party.native,
                Task::Leave,
                "POST",
                path,
                token,
                Some(json!({})),
            );
            party.native.status = "Could not load the room's pinned comparison.".into();
            redraw = true;
        }
    }
    // Any local comparison change leaves the room before publishing another pose.
    if party.native.connected()
        && party
            .native
            .room
            .as_ref()
            .is_some_and(|room| !pinned(&repo.0, room))
    {
        if let Some(path) = room_path(&party.native, "leave") {
            let token = party.native.token.clone();
            start(
                &mut party.native,
                Task::Leave,
                "POST",
                path,
                token,
                Some(json!({})),
            );
        }
        party.native.opening = true;
        party.native.status = "Leaving room because the comparison changed…".into();
    }
    let value = |index: usize| {
        party
            .native
            .fields
            .get(index)
            .and_then(|entity| fields.get(*entity).ok())
            .map(|field| field.editor().text().to_string())
            .unwrap_or_default()
    };
    let invite_input = value(0);
    let login = value(1);
    if let Some(action) = party.native.action.take() {
        redraw = true;
        match action {
            Action::Dismiss => {
                if let Some(panel) = party.native.panel.take() {
                    native_panel::close(&mut commands, panel, &mut focus, &parents);
                }
                redraw = false;
            }
            Action::Copy => {
                let _ = clipboard.set_text(party.native.invite.clone());
                redraw = false;
            }
            Action::Focus => party.native.focus_enabled = !party.native.focus_enabled,
            Action::Voice => {
                if party.native.media.is_some() {
                    party.native.media = None;
                    party.native.media_ready = false;
                    party.native.talking = false;
                    party.native.outgoing.push_back(json!({"type":"voice","enabled":false,"speaking":false,"agent_input":false,"share_with_agents":false}));
                    party.native.status = "Microphone disabled.".into();
                } else if party.native.connected() {
                    match media::Client::start() {
                        Ok(media) => {
                            media.command(media::Command::Enable {
                                id: party.native.member_id.clone(),
                                ice: party.native.ice.clone(),
                            });
                            party.native.media = Some(media);
                            party.native.status = "Opening microphone and speakers…".into();
                        }
                        Err(error) => party.native.status = error,
                    }
                }
            }
            Action::Mute(index) => {
                if let Some(id) = party
                    .native
                    .members
                    .get(index)
                    .and_then(|member| member["id"].as_str())
                    .map(str::to_owned)
                {
                    let muted = if party.native.muted.contains(&id) {
                        party.native.muted.remove(&id);
                        false
                    } else {
                        party.native.muted.insert(id.clone());
                        true
                    };
                    if let Some(media) = &party.native.media {
                        media.command(media::Command::Mute { peer: id, muted });
                    }
                }
            }
            Action::ShareCue => {
                if let Some(focus) = party
                    .projection
                    .as_ref()
                    .and_then(|projection| projection.focus.clone())
                {
                    party
                        .native
                        .outgoing
                        .push_back(json!({"type":"cue","cue":write_focus(&Some(focus))}));
                } else {
                    party.native.status = "Approach a code wall, then share its location.".into();
                }
            }
            Action::Fly(index) => {
                party.destination = party
                    .native
                    .members
                    .get(index)
                    .and_then(|member| read_focus(&member["focus"]));
                if let Some(panel) = party.native.panel.take() {
                    native_panel::close(&mut commands, panel, &mut focus, &parents);
                }
                redraw = false;
            }
            Action::Cue(index) => {
                party.destination = party
                    .native
                    .cues
                    .get(index)
                    .and_then(|cue| read_focus(&cue["cue"]));
                if let Some(panel) = party.native.panel.take() {
                    native_panel::close(&mut commands, panel, &mut focus, &parents);
                }
                redraw = false;
            }
            Action::Create => {
                match picker
                    .party_credentials()
                    .ok_or("Sign in and open a GitHub comparison first.".into())
                    .and_then(|(target, token)| {
                        comparison(&repo.0, &target).map(|comparison| (comparison, token))
                    }) {
                    Ok((comparison, token)) => start(
                        &mut party.native,
                        Task::Create,
                        "POST",
                        "/party/rooms".into(),
                        token,
                        Some(json!({"comparison":comparison})),
                    ),
                    Err(error) => party.native.status = error,
                }
            }
            Action::Join => {
                match invitation(&invite_input).and_then(|(id, invitation)| {
                    picker
                        .party_token()
                        .map(|token| (id, invitation, token))
                        .ok_or("Sign in with GitHub before joining.".into())
                }) {
                    Ok((id, invitation, token)) => start(
                        &mut party.native,
                        Task::Join,
                        "POST",
                        format!("/party/rooms/{id}/join"),
                        token,
                        Some(json!({"invite":invitation})),
                    ),
                    Err(error) => party.native.status = error,
                }
            }
            action => {
                let (suffix, task, body) = match action {
                    Action::Invite => ("invites", Task::Invite, json!({"login":login.trim()})),
                    Action::RevokeInvite => match invitation(&party.native.invite) {
                        Ok((_, token)) => {
                            ("revoke-invite", Task::RevokeInvite, json!({"invite":token}))
                        }
                        Err(error) => {
                            party.native.status = error;
                            return;
                        }
                    },
                    Action::Leave => ("leave", Task::Leave, json!({})),
                    Action::Close => ("close", Task::Close, json!({})),
                    Action::Remove(index) => (
                        "remove",
                        Task::Remove,
                        json!({"member_id":party.native.members.get(index).and_then(|member| member["id"].as_str())}),
                    ),
                    _ => unreachable!(),
                };
                if let Some(path) = room_path(&party.native, suffix) {
                    if matches!(action, Action::Leave | Action::Close) {
                        party.native.media = None;
                        party.native.media_ready = false;
                    }
                    let token = party.native.token.clone();
                    start(&mut party.native, task, "POST", path, token, Some(body));
                }
            }
        }
    }
    let media_events = party
        .native
        .media
        .as_ref()
        .map(media::Client::events)
        .unwrap_or_default();
    for event in media_events {
        match event {
            media::Event::Send(value) => {
                if party.native.outgoing.len() < 128 {
                    party.native.outgoing.push_back(value);
                }
            }
            media::Event::Ready => {
                party.native.media_ready = true;
                party.native.outgoing.push_back(json!({"type":"voice","enabled":true,"speaking":false,"agent_input":false,"share_with_agents":false}));
                if let Some(media) = &party.native.media {
                    media.command(media::Command::Members(party.native.members.clone()));
                }
                party.native.status = format!(
                    "Voice enabled. Hold {talk_label} to speak. Agent audio forwarding unavailable."
                );
                redraw = true;
            }
            media::Event::Error(error) => {
                party.native.media = None;
                party.native.media_ready = false;
                party.native.outgoing.push_back(json!({"type":"voice","enabled":false,"speaking":false,"agent_input":false,"share_with_agents":false}));
                party.native.status = error;
                redraw = true;
            }
        }
    }
    if party.native.media_ready && windows.single().is_ok_and(|window| !window.focused) {
        party.native.media = None;
        party.native.media_ready = false;
        party.native.talking = false;
        party.native.outgoing.push_back(json!({"type":"voice","enabled":false,"speaking":false,"agent_input":false,"share_with_agents":false}));
        party.native.status =
            "Voice stopped when the window lost focus. Enable it again to resume.".into();
        redraw = true;
    }
    let talking = party.native.media_ready
        && party.native.connected()
        && keys.pressed(talk_key)
        && windows.single().is_ok_and(|window| window.focused);
    if talking != party.native.talking {
        party.native.talking = talking;
        if let Some(media) = &party.native.media {
            media.command(media::Command::Talk(talking));
        }
        let enabled = party.native.media_ready;
        party.native.outgoing.push_back(json!({"type":"voice","enabled":enabled,"speaking":talking,"agent_input":false,"share_with_agents":false}));
    }
    if party.native.room.is_some() && party.native.pending.is_none() && now >= party.native.next {
        party.native.next = now + 1.;
        let token = party.native.token.clone();
        if now >= party.native.renew {
            if let (Some(path), Some(github)) =
                (room_path(&party.native, "renew"), picker.party_token())
            {
                start(
                    &mut party.native,
                    Task::Renew,
                    "POST",
                    path,
                    github,
                    Some(json!({})),
                );
            } else {
                party.native.room = None;
                party.native.status = "Room membership ended. Sign in with GitHub again.".into();
            }
        } else if let Some(path) = room_path(&party.native, "events") {
            let after = party.native.cursor;
            let projection = party
                .projection
                .clone()
                .filter(|_| party.native.connected());
            let sharing = party.native.focus_enabled;
            let previous = party.native.last_focus.clone();
            party.native.last_focus = projection
                .as_ref()
                .and_then(|value| sharing.then(|| value.focus.clone()).flatten());
            let focus_changed = previous != party.native.last_focus;
            let shared_focus = party.native.last_focus.clone();
            party.native.pose_seq += 1;
            let pose_seq = party.native.pose_seq;
            let outgoing = party.native.outgoing.drain(..).collect::<Vec<_>>();
            party.native.pending = Some((
                Task::Poll,
                native_github::Job::spawn(move || {
                    for message in outgoing {
                        request("POST", &path, &token, Some(message))?;
                    }
                    if let Some(projection) = projection {
                        if let Some(pose) = projection.pose {
                            request(
                                "POST",
                                &path,
                                &token,
                                Some(
                                    json!({"type":"pose","seq":pose_seq,"position":pose.position,"forward":pose.forward}),
                                ),
                            )?;
                        }
                        if focus_changed {
                            request(
                                "POST",
                                &path,
                                &token,
                                Some(json!({"type":"focus","focus":write_focus(&shared_focus)})),
                            )?;
                        }
                    }
                    request("GET", &format!("{path}?after={after}"), &token, None)
                }),
            ));
        }
    }
    if toggle && party.native.panel.is_some() {
        if let Some(panel) = party.native.panel.take() {
            native_panel::close(&mut commands, panel, &mut focus, &parents);
        }
    } else if (redraw || follow_key_changed) && (toggle || party.native.panel.is_some()) {
        let panel = if let Some(panel) = party.native.panel {
            native_panel::refresh_tablet(&mut commands, panel, "Review party · F3");
            panel
        } else {
            native_panel::panel(&mut commands, "Review party · F3", 580.)
        };
        party.native.panel = Some(panel);
        party.native.fields.clear();
        let state = &mut party.native;
        commands.entity(panel).with_children(|parent| {
            state.status_label = Some(parent.spawn((Text::new(if state.status.is_empty() { "Invite a GitHub account to a pinned comparison. Source and drafts stay private." } else { &state.status }), native_panel::panel_font(), TextColor(native_panel::INK), Node { min_height: px(46.), ..default() })).id());
            if state.room.is_none() {
                native_panel::button::<_, Party>(parent, "Create room for this GitHub comparison", native_panel::INK, Action::Create);
                state.fields.push(native_panel::input(parent, "Invitation link", &invite_input, 0, false));
                state.fields.push(native_panel::input(parent, "GitHub account to invite", &login, 1, false));
                native_panel::button::<_, Party>(parent, "Join invitation", native_panel::INK, Action::Join);
            } else {
                state.fields.push(native_panel::input(parent, "Invitation link", &state.invite, 0, false));
                state.fields.push(native_panel::input(parent, "GitHub account to invite", &login, 1, false));
                native_panel::button::<_, Party>(parent, "Create account-bound invitation", native_panel::INK, Action::Invite);
                native_panel::button::<_, Party>(parent, "Copy invitation link", native_panel::INK, Action::Copy);
                if !state.invite.is_empty() {
                    native_panel::button::<_, Party>(parent, "Revoke invitation", native_panel::INK, Action::RevokeInvite);
                }
                native_panel::button::<_, Party>(parent, if state.focus_enabled { "Share reading focus: on" } else { "Share reading focus: off" }, native_panel::INK, Action::Focus);
                native_panel::button::<_, Party>(parent, "Share current code location", native_panel::INK, Action::ShareCue);
                native_panel::button::<_, Party>(parent, &if state.media.is_some() { format!("Disable voice · hold {talk_label} to talk") } else { "Enable voice · microphone permission".into() }, native_panel::INK, Action::Voice);
                let roster = parent.spawn(Node { flex_direction: FlexDirection::Column, min_height: px(100.), max_height: px(220.), overflow: Overflow::scroll_y(), ..default() }).with_children(|parent| draw_roster(parent, state)).id();
                state.roster = Some(roster);
                native_panel::button::<_, Party>(parent, "Leave room", native_panel::INK, Action::Leave);
                let host = state.room.as_ref().and_then(|room| room["host_id"].as_str()) == Some(state.member_id.as_str());
                if host {
                    native_panel::button::<_, Party>(parent, "Close room", native_panel::INK, Action::Close);
                }
            }
            native_panel::button::<_, Party>(parent, "Back to city", native_panel::INK, Action::Dismiss);
        });
        if let Ok(mut pilot) = camera.single_mut() {
            pilot.captured = false;
        }
    } else if polled && party.native.panel.is_some() {
        if let Some(roster) = party.native.roster {
            commands
                .entity(roster)
                .despawn_children()
                .with_children(|parent| draw_roster(parent, &party.native));
        }
        if let Some(label) = party.native.status_label
            && let Ok(mut text) = labels.get_mut(label)
        {
            **text = party.native.status.clone();
        }
    }
}

fn draw_roster(parent: &mut ChildSpawnerCommands, state: &State) {
    let host = state
        .room
        .as_ref()
        .and_then(|room| room["host_id"].as_str())
        == Some(state.member_id.as_str());
    for (index, member) in state.members.iter().enumerate() {
        let login = member["login"].as_str().unwrap_or("Reviewer");
        let speaking = if member["voice"]["speaking"] == true {
            " · speaking"
        } else {
            ""
        };
        let label = member["focus"]["path"].as_str().map_or_else(
            || format!("@{login} · flying{speaking}"),
            |path| format!("@{login} · {path}{speaking}"),
        );
        native_panel::button::<_, Party>(parent, &label, native_panel::INK, Action::Fly(index));
        if member["id"].as_str() != Some(state.member_id.as_str()) {
            native_panel::button::<_, Party>(
                parent,
                if state.muted.contains(member["id"].as_str().unwrap_or("")) {
                    "Unmute reviewer"
                } else {
                    "Mute reviewer"
                },
                native_panel::INK,
                Action::Mute(index),
            );
            if host {
                native_panel::button::<_, Party>(
                    parent,
                    &format!("Remove @{login}"),
                    native_panel::INK,
                    Action::Remove(index),
                );
            }
        }
    }
    for (index, cue) in state.cues.iter().enumerate() {
        native_panel::button::<_, Party>(
            parent,
            cue["cue"]["path"].as_str().unwrap_or("Shared location"),
            native_panel::INK,
            Action::Cue(index),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invite_is_a_semantic_room_reference_not_an_arbitrary_request_url() {
        let id = "12345678-1234-1234-1234-123456789abc";
        let (parsed, token) = invitation(&format!(
            "https://repo-city.meta-uber-engineer.dev/?party={id}&invite=secret"
        ))
        .unwrap();
        assert_eq!(parsed, id);
        assert_eq!(token, "secret");
        let (parsed, token) = invitation(&format!(
            "https://repo-city.meta-uber-engineer.dev/?party={id}#invite=private"
        ))
        .unwrap();
        assert_eq!(parsed, id);
        assert_eq!(token, "private");
        assert!(invitation("file:///etc/passwd").is_err());
        assert!(invitation("?party=../../secret&invite=x").is_err());
    }
}
