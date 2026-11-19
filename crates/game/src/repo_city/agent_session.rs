//! Native Codex dialogue and opt-in voice, scoped to the currently inspected checkout.
//! Lifecycle/transcript conventions adapted from bobishh/ecky at
//! c48e376f676663272ee3a06622cd81bacb3f24d7; Tauri/CAD services are not copied.
mod audio;
mod g711;
mod model;
mod rtc;
mod transport;

use super::*;
use bevy::{input_focus::InputFocus, text::EditableText};
use model::{Dialogue, Projection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use transport::{Client, Command, Event};

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Session>()
            .add_systems(Update, update.after(review_tablet::update))
            .add_systems(Update, presence.after(update));
    }
}

#[derive(Component, Clone, Copy)]
enum Action {
    Connect,
    Send,
    Voice,
    Stop,
    Disconnect,
    Approve,
    Reject,
}
#[derive(Component, Default)]
struct Prompt {
    initialized: bool,
}
#[derive(Component)]
struct HistoryArea {
    follow: bool,
    previous_y: f32,
}
#[derive(Component)]
struct Transcript;
#[derive(Component)]
struct Status;
#[derive(Component)]
struct ApprovalText;
#[derive(Component)]
struct ConnectionLabel;
#[derive(Component)]
struct VoiceLabel;

struct Approval {
    id: Value,
    title: String,
}

#[derive(Resource, Default)]
pub(super) struct Session {
    root: Option<PathBuf>,
    client: Option<Client>,
    projection: Projection,
    prompt: String,
    action: Option<Action>,
    approvals: Vec<Approval>,
    speaking: Arc<AtomicBool>,
    connected: bool,
    voice_connected: bool,
    dirty: bool,
    saved_at: f64,
    status: String,
    version: u64,
}

impl Session {
    pub(super) fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::Relaxed)
    }
    pub(super) fn voice_active(&self) -> bool {
        self.projection.voice.is_some()
    }
    fn connect(&mut self) -> Result<(), String> {
        if self.client.is_none() {
            let root = self.root.clone().ok_or("Open a local checkout first.")?;
            self.client = Some(Client::start(
                root,
                self.projection.dialogue.thread.clone(),
                self.speaking.clone(),
            ));
            self.status = "Connecting to local Codex…".into();
        }
        Ok(())
    }
    fn send(&mut self, command: Command) -> Result<(), String> {
        self.connect()?;
        self.client
            .as_ref()
            .ok_or("Agent is disconnected.")?
            .send(command)
    }
    fn disconnect(&mut self) {
        self.client = None;
        self.connected = false;
        self.voice_connected = false;
        self.projection.voice = None;
        self.projection.turn = None;
        self.projection.presence.clear();
        self.approvals.clear();
        self.speaking.store(false, Ordering::Relaxed);
    }
    fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let root = self.root.as_ref().ok_or("No conversation is open.")?;
        store(root, &self.projection.dialogue)?;
        self.dirty = false;
        Ok(())
    }
}

impl native_panel::Actions<Action> for Session {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.save();
    }
}

/// The session outlives this view; closing VibePad does not stop conversation or voice.
pub(super) fn spawn(parent: &mut ChildSpawnerCommands) -> Entity {
    let mut prompt = None;
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.,
            min_height: px(0.),
            row_gap: px(8.),
            width: percent(100.),
            ..default()
        })
        .with_children(|body| {
            body.spawn((
                Node {
                    width: percent(100.),
                    min_height: px(38.),
                    flex_shrink: 0.,
                    align_items: AlignItems::Center,
                    column_gap: px(8.),
                    padding: UiRect::axes(px(12.), px(6.)),
                    border: UiRect::all(px(1.)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(12, 33, 41)),
                BorderColor::all(Color::srgb_u8(35, 76, 82)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    ConnectionLabel,
                    Text::new("Codex · disconnected"),
                    native_panel::panel_font(),
                    TextColor(native_panel::INK),
                ));
                bar.spawn(Node {
                    flex_grow: 1.,
                    ..default()
                });
                bar.spawn((
                    VoiceLabel,
                    Text::new("F9 · Voice off"),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(133, 175, 180)),
                ));
                for (index, (label, action)) in [
                    ("Connect", Action::Connect),
                    ("Stop work", Action::Stop),
                    ("Disconnect", Action::Disconnect),
                ]
                .into_iter()
                .enumerate()
                {
                    let button = native_panel::button::<Action, Session>(
                        bar,
                        label,
                        native_panel::INK,
                        action,
                    );
                    bar.commands().entity(button).insert(
                        bevy::input_focus::tab_navigation::TabIndex(20 + index as i32),
                    );
                }
            });
            body.spawn(Node {
                height: px(38.),
                flex_shrink: 0.,
                width: percent(100.),
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .with_children(|area| {
                area.spawn((
                    Status,
                    Text::new(""),
                    TextFont::from_font_size(13.),
                    TextColor(Color::srgb_u8(229, 187, 128)),
                    Node {
                        width: percent(100.),
                        ..default()
                    },
                ));
            });
            body.spawn((
                HistoryArea {
                    follow: true,
                    previous_y: 0.,
                },
                tablet_scroll::Area,
                ScrollPosition::default(),
                Node {
                    flex_grow: 1.,
                    min_height: px(0.),
                    width: percent(100.),
                    overflow: Overflow::scroll_y(),
                    padding: UiRect::all(px(14.)),
                    border: UiRect::all(px(1.)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(8, 20, 28)),
                BorderColor::all(Color::srgb_u8(30, 59, 67)),
            ))
            .with_children(|history| {
                history.spawn((
                    Transcript,
                    Node {
                        width: percent(100.),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(12.),
                        ..default()
                    },
                ));
            });
            body.spawn(Node {
                max_height: px(110.),
                overflow: Overflow::scroll_y(),
                flex_shrink: 0.,
                width: percent(100.),
                ..default()
            })
            .with_children(|area| {
                area.spawn((
                    ApprovalText,
                    Text::new(""),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(239, 192, 121)),
                    Node {
                        width: percent(100.),
                        ..default()
                    },
                ));
            });
            body.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6.),
                flex_shrink: 0.,
                ..default()
            })
            .with_children(|row| {
                for (index, (label, action)) in [
                    ("Approve once", Action::Approve),
                    ("Reject", Action::Reject),
                ]
                .into_iter()
                .enumerate()
                {
                    let button = native_panel::button::<Action, Session>(
                        row,
                        label,
                        native_panel::INK,
                        action,
                    );
                    row.commands().entity(button).insert(
                        bevy::input_focus::tab_navigation::TabIndex(90 + index as i32),
                    );
                }
            });
            body.spawn((
                Node {
                    width: percent(100.),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.,
                    row_gap: px(8.),
                    padding: UiRect::all(px(12.)),
                    border: UiRect::all(px(1.)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(12, 29, 38)),
                BorderColor::all(Color::srgb_u8(40, 85, 91)),
            ))
            .with_children(|composer| {
                let field = native_panel::input(composer, "", "", 100, true);
                composer.commands().entity(field).insert((
                    Prompt::default(),
                    Name::new("Message"),
                    Node {
                        width: percent(100.),
                        height: px(76.),
                        padding: UiRect::all(px(8.)),
                        ..default()
                    },
                ));
                prompt = Some(field);
                composer
                    .spawn(Node {
                        width: percent(100.),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Cmd/Ctrl+Enter sends · F9 toggles voice · H help"),
                            TextFont::from_font_size(12.),
                            TextColor(Color::srgb_u8(126, 169, 175)),
                        ));
                        row.spawn(Node {
                            flex_grow: 1.,
                            ..default()
                        });
                        let send = native_panel::button::<Action, Session>(
                            row,
                            "Send",
                            native_panel::INK,
                            Action::Send,
                        );
                        row.commands().entity(send).insert((
                            bevy::input_focus::tab_navigation::TabIndex(101),
                            BackgroundColor(Color::srgb_u8(26, 86, 88)),
                        ));
                    });
            });
        });
    prompt.expect("Agent view always has a message composer")
}

fn draw_dialogue(parent: &mut ChildSpawnerCommands, dialogue: &Dialogue) {
    if dialogue.messages.is_empty() {
        parent.spawn((
            Text::new("Ask about the code in front of you."),
            native_panel::panel_font(),
            TextColor(Color::srgb_u8(144, 180, 185)),
        ));
        return;
    }
    for message in dialogue
        .messages
        .iter()
        .rev()
        .take(60)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let (name, accent) = match message.role.as_str() {
            "user" => ("You", Color::srgb_u8(213, 183, 130)),
            "system" => ("Session", Color::srgb_u8(151, 171, 177)),
            _ => ("Codex", native_panel::INK),
        };
        parent
            .spawn((
                Node {
                    width: percent(100.),
                    flex_shrink: 0.,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.),
                    padding: UiRect::axes(px(12.), px(10.)),
                    border: UiRect::left(px(2.)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(12, 28, 37)),
                BorderColor::all(accent),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new(name),
                    TextFont::from_font_size(12.),
                    TextColor(accent),
                ));
                card.spawn((
                    Text::new(&message.text),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(218, 234, 236)),
                    Node {
                        width: percent(100.),
                        ..default()
                    },
                ));
            });
    }
}

fn update(
    mut commands: Commands,
    repo: Res<Repo>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut session: ResMut<Session>,
    mut prompts: Query<(Entity, &mut EditableText, &mut Prompt)>,
    wheel: Res<AccumulatedMouseScroll>,
    mut history_areas: Query<(&mut HistoryArea, &mut ScrollPosition, &ComputedNode)>,
    mut labels: Query<(
        &mut Text,
        &mut TextColor,
        Option<&ConnectionLabel>,
        Option<&VoiceLabel>,
        Option<&Status>,
        Option<&ApprovalText>,
    )>,
    mut controls: Query<(
        Entity,
        &Action,
        &mut Node,
        &Children,
        Option<&bevy::ui::InteractionDisabled>,
    )>,
    transcripts: Query<Entity, With<Transcript>>,
    mut rendered: Local<Option<u64>>,
    mut rendered_entities: Local<HashSet<Entity>>,
) {
    let root = &repo.0.root;
    if session.root.as_ref() != Some(root) {
        let _ = session.save();
        session.disconnect();
        session.root = Some(root.clone());
        session.projection = Projection {
            dialogue: load(root),
            ..default()
        };
        session.prompt.clear();
        session.status = "Disconnected. Connect uses your existing Codex login.".into();
        session.version = session.version.wrapping_add(1);
        for (_, mut field, _) in &mut prompts {
            field.editor_mut().set_text("");
        }
    }
    let mut prompt_focused = false;
    for (entity, mut field, mut marker) in &mut prompts {
        if !marker.initialized {
            field.editor_mut().set_text(&session.prompt);
            marker.initialized = true;
        }
        session.prompt = field.editor().text().to_string();
        prompt_focused |= focus.get() == Some(entity);
    }
    for (mut area, mut position, node) in &mut history_areas {
        let delta = match wheel.unit {
            bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 34.,
            bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
        };
        let limit = tablet_scroll::range(node);
        if (position.y - area.previous_y).abs() > 1. {
            area.follow = position.y >= limit - 8.;
        }
        if delta != 0. {
            position.y = (position.y - delta).clamp(0., limit);
            area.follow = position.y >= limit - 8.;
        }
        if area.follow {
            position.y = limit;
        }
        area.previous_y = position.y;
    }
    if keys.just_pressed(KeyCode::F9) {
        session.action = Some(Action::Voice);
    }
    if prompt_focused
        && keys.just_pressed(KeyCode::Enter)
        && keys.any_pressed([
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
        ])
    {
        session.action = Some(Action::Send);
    }
    let events = session
        .client
        .as_ref()
        .map(Client::poll)
        .unwrap_or_default();
    if !events.is_empty() {
        session.version = session.version.wrapping_add(1);
    }
    for event in events {
        match event {
            Event::Connected(thread) => {
                session.projection.dialogue.thread = Some(thread);
                session.connected = true;
                session.dirty = true;
                session.status = "Connected".into();
            }
            Event::Replaced {
                previous: _,
                thread,
            } => {
                session.projection.message(&format!("session-boundary:{thread}"), "system", "Previous Codex session unavailable; a new conversation started. Earlier local messages remain above.", false);
                session.status = "Previous session unavailable; a new conversation started.".into();
                session.dirty = true;
            }
            Event::Notification(method, params) => {
                if session.projection.apply(root, &method, &params) {
                    session.dirty = true;
                    if matches!(method.as_str(), "turn/started" | "turn/completed") {
                        session.status = session.projection.status.clone();
                    }
                }
                if method == "thread/realtime/error" {
                    session.status = params["message"]
                        .as_str()
                        .unwrap_or("Voice connection failed.")
                        .chars()
                        .take(1500)
                        .collect();
                }
            }
            Event::Approval { id, method, params } => {
                let owner = params["threadId"].as_str();
                if owner == session.projection.dialogue.thread.as_deref()
                    || owner.is_some_and(|id| session.projection.agents.contains_key(id))
                {
                    let reason = params["reason"].as_str().unwrap_or("Permission required");
                    let command = params["command"].as_str().unwrap_or("");
                    let detail = params["itemId"]
                        .as_str()
                        .and_then(|id| session.projection.approvals.get(id))
                        .cloned()
                        .unwrap_or_default();
                    session.approvals.push(Approval {
                        id,
                        title: format!("{method}\n{reason}\n{command}\n{detail}")
                            .chars()
                            .take(8000)
                            .collect(),
                    });
                } else if let Some(client) = &session.client {
                    let _ = client.send(Command::Approval { id, accept: false });
                }
            }
            Event::Voice { id, active } => {
                if session.projection.voice.as_deref() == Some(id.as_str()) {
                    session.voice_connected = active;
                    if !active {
                        session.projection.voice = None;
                    }
                }
            }
            Event::Error(error) => session.status = error,
            Event::Closed => {
                session.disconnect();
                if session.status == "Connected" {
                    session.status = "Disconnected".into();
                }
            }
        }
    }
    if let Some(action) = session.action.take() {
        session.version = session.version.wrapping_add(1);
        let result = match action {
            Action::Connect => session.connect(),
            Action::Disconnect => {
                session.disconnect();
                session.status = "Disconnected".into();
                Ok(())
            }
            Action::Send if !session.prompt.trim().is_empty() => {
                let prompt = session.prompt.trim().to_owned();
                let sent = session.send(Command::Send {
                    prompt: prompt.clone(),
                    context: context(root, &repo.0),
                });
                if sent.is_ok() {
                    session.projection.message(
                        &format!("user-pending-{}", nonce()),
                        "user",
                        &prompt,
                        false,
                    );
                    session.dirty = true;
                    session.prompt.clear();
                    for (_, mut field, _) in &mut prompts {
                        field.editor_mut().set_text("");
                    }
                }
                sent
            }
            Action::Voice => {
                if session.voice_active() {
                    session.projection.voice = None;
                    session.voice_connected = false;
                    session.send(Command::StopVoice)
                } else {
                    let id = format!("city-{}", nonce());
                    let result = session.send(Command::Voice {
                        id: id.clone(),
                        context: context(root, &repo.0),
                    });
                    if result.is_ok() {
                        session.projection.voice = Some(id);
                    }
                    result
                }
            }
            Action::Stop => session.send(Command::Interrupt),
            Action::Approve | Action::Reject if !session.approvals.is_empty() => {
                let approval = session.approvals.remove(0);
                session.send(Command::Approval {
                    id: approval.id,
                    accept: matches!(action, Action::Approve),
                })
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            session.status = error;
        }
    }
    if session.dirty && time.elapsed_secs_f64() - session.saved_at >= 1. {
        session.saved_at = time.elapsed_secs_f64();
        if let Err(error) = session.save() {
            session.status = format!("Conversation remains in memory: {error}");
        }
    }
    if *rendered != Some(session.version)
        || transcripts
            .iter()
            .any(|entity| !rendered_entities.contains(&entity))
    {
        *rendered = Some(session.version);
        rendered_entities.clear();
        for entity in &transcripts {
            rendered_entities.insert(entity);
            commands
                .entity(entity)
                .despawn_children()
                .with_children(|parent| draw_dialogue(parent, &session.projection.dialogue));
        }
    }
    let connection = if session.connected {
        if session.projection.turn.is_some() {
            "Codex · working"
        } else {
            "Codex · connected"
        }
    } else if session.client.is_some() {
        "Codex · connecting"
    } else {
        "Codex · disconnected"
    };
    let voice = if session.voice_active() {
        if session.voice_connected {
            "F9 · Voice on"
        } else {
            "F9 · Connecting voice…"
        }
    } else {
        "F9 · Voice off"
    };
    let detail = match session.status.as_str() {
        ""
        | "Connected"
        | "Disconnected"
        | "Ready"
        | "Working"
        | "Disconnected. Connect uses your existing Codex login." => "",
        other => other,
    };
    let approval = session
        .approvals
        .first()
        .map_or("", |approval| approval.title.as_str());
    for (mut text, _, connection_label, voice_label, state, request) in &mut labels {
        let value = if connection_label.is_some() {
            connection
        } else if voice_label.is_some() {
            voice
        } else if state.is_some() {
            detail
        } else if request.is_some() {
            approval
        } else {
            continue;
        };
        text.set_if_neq(Text::new(value));
    }
    for (entity, action, mut node, children, disabled) in &mut controls {
        let approval_control = matches!(action, Action::Approve | Action::Reject);
        let display = if approval_control && session.approvals.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        let enabled = match action {
            Action::Connect => session.client.is_none(),
            Action::Disconnect | Action::Stop => session.client.is_some(),
            Action::Send => !session.prompt.trim().is_empty(),
            Action::Approve | Action::Reject => !session.approvals.is_empty(),
            _ => true,
        };
        if enabled && disabled.is_some() {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        } else if !enabled && disabled.is_none() {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
        for child in children.iter() {
            if let Ok((_, mut ink, _, _, _, _)) = labels.get_mut(child) {
                ink.set_if_neq(TextColor(if enabled {
                    native_panel::INK
                } else {
                    Color::srgb_u8(83, 116, 122)
                }));
            }
        }
    }
}

fn presence(
    session: Res<Session>,
    repo: Res<Repo>,
    buildings: Query<(&Facade, &Transform)>,
    mut gizmos: Gizmos,
) {
    for (facade, pose) in &buildings {
        if repo.0.files.get(facade.index).is_some_and(|file| {
            session
                .projection
                .presence
                .iter()
                .any(|presence| presence.path == file.path)
        }) {
            let ink = Color::srgb_u8(124, 207, 246);
            let roof = pose.translation + Vec3::Y * (pose.scale.y / 2. + 0.6);
            gizmos.line(roof, roof + Vec3::Y * 6., ink);
            gizmos.cube(
                Transform::from_translation(roof + Vec3::Y * 6.).with_scale(Vec3::splat(0.7)),
                ink,
            );
        }
    }
}

fn context(root: &Path, repo: &Repository) -> String {
    format!(
        "<repo_city_context>\nRepository: {}\nRevision: {}\nBaseline: {}\nCurrent camera projection (may be inactive or stale; get_focus is authoritative): {}\n</repo_city_context>",
        root.display(),
        repo.revision,
        repo.baseline.as_deref().unwrap_or("none"),
        focus_context::read(&root.to_string_lossy())
    )
}
fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos())
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u8,
    root: PathBuf,
    dialogue: Dialogue,
}
fn path(root: &Path) -> Option<PathBuf> {
    let hash = root
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    Some(
        repository_picker::settings_file()?
            .with_file_name("agent-conversations")
            .join(format!("{hash:016x}.json")),
    )
}
fn load(root: &Path) -> Dialogue {
    path(root)
        .filter(|path| std::fs::metadata(path).is_ok_and(|info| info.len() <= 4 * 1024 * 1024))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .filter(|stored| {
            stored.version == 1
                && stored.root == root
                && stored.dialogue.messages.len() <= 160
                && stored
                    .dialogue
                    .messages
                    .iter()
                    .all(|message| message.text.len() <= 64_000)
        })
        .map(|stored| stored.dialogue)
        .unwrap_or_default()
}
fn store(root: &Path, dialogue: &Dialogue) -> Result<(), String> {
    let data = encode(root, dialogue)?;
    let path = path(root).ok_or("Could not locate conversation storage.")?;
    let parent = path
        .parent()
        .ok_or("Could not locate conversation directory.")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&data).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn encode(root: &Path, dialogue: &Dialogue) -> Result<Vec<u8>, String> {
    let data = serde_json::to_vec(&Stored {
        version: 1,
        root: root.into(),
        dialogue: dialogue.clone(),
    })
    .map_err(|error| error.to_string())?;
    if data.len() > 4 * 1024 * 1024 {
        return Err("Local conversation exceeds 4 MiB. Recent messages remain in memory; the last saved history is unchanged. The full Codex conversation remains in Codex.".into());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_agent_widgets_does_not_connect_or_enable_microphone() {
        let mut app = App::new();
        app.init_resource::<Session>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|parent| {
                    spawn(parent);
                });
            });
        app.update();
        let session = app.world().resource::<Session>();
        assert!(session.client.is_none());
        assert!(!session.voice_active());
        let mut actions = app.world_mut().query::<&Action>();
        assert!(
            actions
                .iter(app.world())
                .any(|action| matches!(action, Action::Send))
        );
        let mut fields = app
            .world_mut()
            .query_filtered::<&EditableText, With<Prompt>>();
        assert_eq!(fields.iter(app.world()).count(), 1);
    }

    #[test]
    fn closing_agent_widgets_keeps_voice_and_has_no_mouse_only_talk_control() {
        let mut app = App::new();
        app.init_resource::<Session>();
        app.world_mut().resource_mut::<Session>().projection.voice = Some("active-session".into());
        let root = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .commands()
            .entity(root)
            .with_children(|parent| {
                spawn(parent);
            });
        app.world_mut().flush();
        let mut actions = app.world_mut().query::<&Action>();
        assert!(
            !actions
                .iter(app.world())
                .any(|action| matches!(action, Action::Voice))
        );
        app.world_mut().despawn(root);
        assert!(app.world().resource::<Session>().voice_active());
    }

    #[test]
    fn stored_history_never_exceeds_its_reload_limit_even_after_json_escaping() {
        let mut dialogue = Dialogue::default();
        dialogue.thread = Some("saved-thread".into());
        dialogue.messages = (0..160)
            .map(|index| model::Message {
                id: index.to_string(),
                role: "user".into(),
                text: "\0".repeat(5000),
            })
            .collect();
        assert!(encode(Path::new("/repo"), &dialogue).is_err());
        dialogue.messages.truncate(1);
        let stored: Stored =
            serde_json::from_slice(&encode(Path::new("/repo"), &dialogue).unwrap()).unwrap();
        assert_eq!(stored.dialogue.thread.as_deref(), Some("saved-thread"));
    }
}
