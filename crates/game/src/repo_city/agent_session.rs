//! Native Codex dialogue and opt-in voice, scoped to the currently inspected checkout.
//! Lifecycle/transcript conventions adapted from bobishh/ecky at
//! c48e376f676663272ee3a06622cd81bacb3f24d7; Tauri/CAD services are not copied.

use super::agent_engine::{self, Command, Dialogue, Engine, Event, Projection};
mod controls;
mod voice_hud;
pub(super) use controls::settings;

use super::*;
use bevy::{input_focus::InputFocus, text::EditableText};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Session>()
            .add_systems(Startup, |mut session: ResMut<Session>| {
                session.selected = load_provider();
            })
            .add_systems(Startup, voice_hud::spawn)
            .add_systems(Update, voice_hud::update.after(update))
            .add_systems(Update, update.after(review_tablet::update))
            .add_systems(Update, presence.after(update));
        controls::register(app);
    }
}

#[derive(Component, Clone, Copy)]
enum Action {
    Connect,
    Send,
    RestoreMessage,
    Voice,
    Stop,
    Disconnect,
    Follow,
    Approve,
    Reject,
    SendKey,
    ApprovalMode,
    FollowKey,
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
    token: String,
    title: String,
}

#[derive(Resource, Default)]
pub(super) struct Session {
    root: Option<PathBuf>,
    selected: agent_engine::Provider,
    interaction: controls::Settings,
    client: Option<Box<dyn Engine>>,
    projection: Projection,
    prompt: String,
    pending_send: Option<String>,
    stopping: bool,
    restored_messages: HashSet<String>,
    action: Option<Action>,
    approvals: Vec<Approval>,
    speaking: Arc<AtomicBool>,
    connected: bool,
    follow_agent: bool,
    voice_connected: bool,
    voice_waiting: bool,
    dirty: bool,
    saved_at: f64,
    status: String,
    voice_error: Option<String>,
    version: u64,
}

impl Session {
    pub(super) fn timeline(&self, root: &Path) -> Option<&agent_engine::activity::Timeline> {
        (self.root.as_deref() == Some(root)).then_some(&self.projection.timeline)
    }

    pub(super) fn activity_owner(&self, thread: &str) -> String {
        if self
            .projection
            .dialogue
            .thread
            .as_deref()
            .is_none_or(|primary| primary == thread)
        {
            self.selected.label.into()
        } else {
            format!("Subagent {}", thread.chars().take(8).collect::<String>())
        }
    }

    fn record_engine_event(&mut self, event: &Event) {
        use agent_engine::activity::{Details, Kind, Phase};
        let owner = self
            .projection
            .dialogue
            .thread
            .as_deref()
            .unwrap_or("local-session");
        let (thread, key, title, detail, phase) = match event {
            Event::Connected(thread) => (
                thread.as_str(),
                "connection".into(),
                "Connected to provider",
                "",
                Phase::Completed,
            ),
            Event::Replaced { conversation, .. } => (
                conversation.as_str(),
                "connection".into(),
                "Conversation replaced",
                "",
                Phase::Completed,
            ),
            Event::Delivery { id, result } => match result {
                Ok(()) => (
                    owner,
                    format!("delivery:{id}"),
                    "Message delivered",
                    "",
                    Phase::Completed,
                ),
                Err(error) => (
                    owner,
                    format!("delivery:{id}"),
                    "Message delivery failed",
                    error.as_str(),
                    Phase::Failed,
                ),
            },
            Event::Approval { token, title } => (
                owner,
                format!("request:{token}"),
                "Permission requested",
                title.as_str(),
                Phase::Running,
            ),
            Event::Voice { id, active: true } => (
                owner,
                format!("voice:{id}"),
                "Voice connected",
                "",
                Phase::Running,
            ),
            Event::Voice { id, active: false } => (
                owner,
                format!("voice:{id}"),
                "Voice disconnected",
                "",
                Phase::Ended,
            ),
            Event::VoiceWaiting { id } => (
                owner,
                format!("voice:{id}"),
                "Voice waiting for the active turn",
                "",
                Phase::Running,
            ),
            Event::VoiceStarting { id } => (
                owner,
                format!("voice:{id}"),
                "Voice connecting",
                "",
                Phase::Running,
            ),
            Event::StopUnconfirmed(error) => (
                owner,
                "stop-error".into(),
                "Work stop unconfirmed",
                error.as_str(),
                Phase::Failed,
            ),
            Event::Error(error) => (
                owner,
                "session-error".into(),
                "Session error",
                error.as_str(),
                Phase::Failed,
            ),
            Event::VoiceError(error) => (
                owner,
                "voice-error".into(),
                "Voice error",
                error.as_str(),
                Phase::Failed,
            ),
            Event::Closed => (
                owner,
                "connection".into(),
                "Provider disconnected",
                "",
                Phase::Ended,
            ),
            Event::Changed { .. } => return,
        };
        let mut details = Details::new(Kind::Session, title);
        details.body = detail.into();
        self.projection
            .timeline
            .record(thread, &key, details, phase);
    }

    pub(super) fn chat_board(&self) -> Vec<review_party::BoardMessage> {
        let mut recent = self
            .projection
            .dialogue
            .messages
            .iter()
            .rev()
            .filter(|message| matches!(message.role.as_str(), "user" | "assistant"))
            .take(5)
            .map(|message| review_party::BoardMessage {
                login: if message.role == "user" {
                    "You"
                } else {
                    self.selected.label
                }
                .into(),
                text: message.text.clone(),
            })
            .collect::<Vec<_>>();
        recent.reverse();
        recent
    }

    pub(super) fn follows_agent(&self) -> bool {
        self.follow_agent
    }
    pub(super) fn pause_following(&mut self) {
        self.follow_agent = false;
    }
    pub(super) fn file_activity(&self) -> Option<&agent_engine::state::FileActivity> {
        self.projection.activity.as_ref()
    }
    pub(super) fn provider(&self) -> agent_engine::Provider {
        self.client
            .as_ref()
            .map_or(self.selected, |client| client.provider())
    }
    pub(super) fn select_provider(
        &mut self,
        provider: agent_engine::Provider,
    ) -> Result<(), String> {
        if self.selected == provider {
            return Ok(());
        }
        if self.pending_send.is_some() || self.projection.turn.is_some() {
            return Err("Stop the active turn before changing agents.".into());
        }
        self.save()?;
        write_state(
            &provider_path().ok_or("Could not locate agent settings.")?,
            provider.id.as_bytes(),
        )?;
        self.disconnect();
        self.selected = provider;
        self.projection = Projection {
            dialogue: self
                .root
                .as_deref()
                .map_or_else(Dialogue::default, |root| load(root, provider)),
            ..default()
        };
        self.restored_messages.clear();
        self.status = "Disconnected".into();
        self.version = self.version.wrapping_add(1);
        Ok(())
    }
    pub(super) fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::Relaxed)
    }
    pub(super) fn voice_active(&self) -> bool {
        self.projection.voice.is_some()
    }
    fn connect(&mut self) -> Result<(), String> {
        if self.client.is_none() {
            let root = self.root.clone().ok_or("Open a local checkout first.")?;
            self.client = Some(agent_engine::connect(
                self.selected,
                agent_engine::Start {
                    root,
                    conversation: self.projection.dialogue.thread.clone(),
                    approval_mode: self.interaction.approvals,
                    speaking: self.speaking.clone(),
                    diagnostics: repository_picker::settings_file()
                        .map(|path| path.with_file_name("voice-status.json")),
                },
            )?);
            self.status = format!("Connecting to local {}…", self.selected.label);
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
    fn submit_prompt(&mut self, context: String) -> Result<(), String> {
        if !self.can_send() {
            return Ok(());
        }
        let text = self.prompt.trim().to_owned();
        if text.chars().count() > 16_000 {
            return Err("Keep the message within 16000 characters.".into());
        }
        let id = format!("user-awaiting-{}", nonce());
        let prompt = agent_engine::Prompt {
            id: id.clone(),
            text,
            context,
        };
        let previous = self.projection.dialogue.clone();
        self.projection
            .dialogue
            .message(&id, "user", &prompt.text, false);
        self.dirty = true;
        if let Err(error) = self.save() {
            self.projection.dialogue = previous;
            return Err(format!("Could not save the draft for sending: {error}"));
        }
        let command = if self.provider().capabilities.steer && self.projection.turn.is_some() {
            Command::Steer(prompt)
        } else {
            Command::Send(prompt)
        };
        self.pending_send = Some(id.clone());
        if let Err(error) = self.send(command) {
            self.finish_send(&id, Err(error.clone()));
            return Err(error);
        }
        self.prompt.clear();
        Ok(())
    }

    fn can_send(&self) -> bool {
        !self.stopping
            && self.pending_send.is_none()
            && !self.prompt.trim().is_empty()
            && (self.provider().capabilities.steer || self.projection.turn.is_none())
    }

    fn can_stop(&self) -> bool {
        self.client.is_some()
            && !self.stopping
            && (self.pending_send.is_some()
                || self.projection.turn.is_some()
                || self.voice_active())
    }

    fn stop_work(&mut self) -> Result<(), String> {
        if !self.can_stop() {
            return Ok(());
        }
        if self.projection.turn.is_some() || self.pending_send.is_some() {
            self.send(Command::Interrupt)?;
            self.stopping = true;
            self.status = "Stopping work…".into();
        } else {
            self.send(Command::StopVoice)?;
            self.projection.voice = None;
            self.voice_connected = false;
            self.voice_waiting = false;
            self.status = "Voice stop requested".into();
        }
        Ok(())
    }

    fn restorable_message(&self) -> Option<usize> {
        if self.pending_send.is_some() || !self.prompt.trim().is_empty() {
            return None;
        }
        self.projection.dialogue.messages.iter().rposition(|row| {
            row.role == "user"
                && !self.restored_messages.contains(&row.id)
                && ["user-rejected-", "user-awaiting-", "user-pending-"]
                    .iter()
                    .any(|prefix| row.id.starts_with(prefix))
        })
    }

    fn restore_message(&mut self) -> bool {
        let Some(index) = self.restorable_message() else {
            return false;
        };
        let row = &self.projection.dialogue.messages[index];
        self.prompt.clone_from(&row.text);
        self.restored_messages.insert(row.id.clone());
        self.status = "Message restored. Review it before sending again.".into();
        true
    }

    fn finish_send(&mut self, id: &str, result: Result<(), String>) {
        if self.pending_send.as_deref() != Some(id) {
            return;
        }
        self.pending_send = None;
        if let Some(row) = self
            .projection
            .dialogue
            .messages
            .iter_mut()
            .find(|row| row.id == id)
        {
            let prefix = if result.is_ok() {
                "user-sent-"
            } else {
                "user-rejected-"
            };
            row.id = format!("{prefix}{}", id.trim_start_matches("user-awaiting-"));
            self.dirty = true;
        }
        if let Err(error) = result {
            self.status = format!("Message not sent: {error}");
        }
    }

    fn disconnect(&mut self) {
        self.follow_agent = false;
        if std::mem::take(&mut self.stopping) && self.status == "Stopping work…" {
            self.status = "Work stop unconfirmed. Agent disconnected.".into();
        }
        if self.pending_send.take().is_some() {
            self.status = format!("Delivery unconfirmed. {}", self.status);
        }
        self.client = None;
        self.connected = false;
        self.voice_connected = false;
        self.voice_waiting = false;
        self.voice_error = None;
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
        store(root, self.selected, &self.projection.dialogue)?;
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
                    Text::new("F5 · Voice off"),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(133, 175, 180)),
                ));
                for (index, (label, action)) in [
                    ("Connect chat", Action::Connect),
                    ("Start voice", Action::Voice),
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
                min_height: px(38.),
                flex_shrink: 0.,
                align_items: AlignItems::Center,
                column_gap: px(12.),
                ..default()
            })
            .with_children(|bar| {
                let button = native_panel::button::<Action, Session>(
                    bar,
                    "Follow agent",
                    native_panel::INK,
                    Action::Follow,
                );
                bar.commands().entity(button).insert((
                    bevy::input_focus::tab_navigation::TabIndex(24),
                    Node {
                        width: px(170.),
                        min_height: px(32.),
                        ..default()
                    },
                ));
                bar.spawn((
                    Text::new("Show code while following. Esc pauses."),
                    native_panel::panel_font(),
                    TextColor(native_panel::INK),
                ));
            });
            agent_timeline::spawn(body, agent_timeline::View::Chat);
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
                let field = native_panel::input(composer, "", "", 100, false);
                composer.commands().entity(field).insert((
                    Prompt::default(),
                    Name::new("Message"),
                    EditableText {
                        max_characters: Some(6000),
                        ..default()
                    },
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
                            controls::Hint,
                            Text::new("Enter sends · Shift+Enter newline · F5 voice"),
                            TextFont::from_font_size(12.),
                            TextColor(Color::srgb_u8(126, 169, 175)),
                        ));
                        row.spawn(Node {
                            flex_grow: 1.,
                            ..default()
                        });
                        let restore = native_panel::button::<Action, Session>(
                            row,
                            "Restore message",
                            native_panel::INK,
                            Action::RestoreMessage,
                        );
                        row.commands()
                            .entity(restore)
                            .insert(bevy::input_focus::tab_navigation::TabIndex(101));
                        let send = native_panel::button::<Action, Session>(
                            row,
                            "Send",
                            native_panel::INK,
                            Action::Send,
                        );
                        row.commands().entity(send).insert((
                            bevy::input_focus::tab_navigation::TabIndex(102),
                            BackgroundColor(Color::srgb_u8(26, 86, 88)),
                        ));
                    });
            });
        });
    prompt.expect("Agent view always has a message composer")
}

fn draw_dialogue(
    parent: &mut ChildSpawnerCommands,
    dialogue: &Dialogue,
    repo: &Repository,
    agent: &str,
    pending_send: Option<&str>,
) {
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
            "user" => (
                message_label(&message.id, pending_send),
                Color::srgb_u8(213, 183, 130),
            ),
            "system" => ("Session", Color::srgb_u8(151, 171, 177)),
            _ => (agent, native_panel::INK),
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
                agent_navigation::draw_links(card, &message.text, repo);
            });
    }
}

fn message_label(id: &str, pending_send: Option<&str>) -> &'static str {
    if pending_send == Some(id) {
        "You · Sending"
    } else if id.starts_with("user-rejected-") {
        "You · Not sent"
    } else if id.starts_with("user-awaiting-") || id.starts_with("user-pending-") {
        "You · Delivery unconfirmed"
    } else {
        "You"
    }
}

fn update(
    mut commands: Commands,
    repo: Res<Repo>,
    time: Res<Time>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
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
    editing: Query<(), Or<(With<EditableText>, With<source_editor::View>)>>,
    blockers: (
        Res<terminal::Terminal>,
        Res<listing::find::Search>,
        Query<&Window, With<PrimaryWindow>>,
    ),
) {
    let root = &repo.0.root;
    if session.root.as_ref() != Some(root) {
        let _ = session.save();
        session.disconnect();
        session.root = Some(root.clone());
        session.projection = Projection {
            dialogue: load(root, session.selected),
            ..default()
        };
        session.prompt.clear();
        session.restored_messages.clear();
        match controls::Settings::load() {
            Ok(settings) => {
                session.interaction = settings;
                session.status = "Disconnected".into();
            }
            Err(error) => {
                session.notice(&error);
                session.status = error;
            }
        }
        session.version = session.version.wrapping_add(1);
        for (_, mut field, _) in &mut prompts {
            field.editor_mut().set_text("");
        }
    }
    for (_, mut field, mut marker) in &mut prompts {
        if !marker.initialized {
            field.editor_mut().set_text(&session.prompt);
            marker.initialized = true;
        }
        session.prompt = field.editor().text().to_string();
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
    if keys.just_pressed(KeyCode::F5) && session.provider().capabilities.voice {
        session.action = Some(Action::Voice);
    }
    let events = {
        let Session {
            client, projection, ..
        } = &mut *session;
        client
            .as_ref()
            .map(|client| client.poll(projection))
            .unwrap_or_default()
    };
    if !events.is_empty() {
        session.version = session.version.wrapping_add(1);
    }
    for event in events {
        session.record_engine_event(&event);
        match event {
            Event::Connected(thread) => {
                session.projection.dialogue.thread = Some(thread);
                session.connected = true;
                session.dirty = true;
                session.status = "Connected".into();
            }
            Event::Replaced {
                conversation: thread,
                writer_conflict,
            } => {
                let notice = if writer_conflict {
                    "The previous Codex conversation is open in another client. City continued in a separate conversation with its saved history."
                } else {
                    "The previous Codex conversation is unavailable. City opened a new conversation. Local history remains above."
                };
                session.projection.dialogue.message(
                    &format!("session-boundary:{thread}"),
                    "system",
                    notice,
                    false,
                );
                session.status = notice.into();
                session.dirty = true;
            }
            Event::Delivery { id, result } => {
                let rejected_start = result.is_err()
                    && session.pending_send.as_deref() == Some(id.as_str())
                    && session.projection.turn.is_none();
                session.finish_send(&id, result);
                if rejected_start {
                    session.stopping = false;
                }
            }
            Event::Changed { status } => {
                session.dirty = true;
                if let Some(status) = status {
                    if session.projection.turn.is_none() && status != "Working" {
                        session.stopping = false;
                        session.approvals.clear();
                    }
                    if status != "Ready" && status != "Working" && status != "Connected" {
                        session.notice(&status);
                    }
                    if !session.stopping {
                        session.status = status;
                    }
                }
            }
            Event::Approval { token, title } => {
                if let Err(error) = session.request_approval(token, title) {
                    session.notice(&error);
                    session.status = error;
                }
            }
            Event::VoiceWaiting { id } => {
                if session.projection.voice.as_deref() == Some(id.as_str()) {
                    session.voice_waiting = true;
                }
            }
            Event::VoiceStarting { id } => {
                if session.projection.voice.as_deref() == Some(id.as_str()) {
                    session.voice_waiting = false;
                    session.voice_connected = false;
                    session.voice_error = None;
                }
            }
            Event::Voice { id, active } => {
                if session.projection.voice.as_deref() == Some(id.as_str()) {
                    session.voice_connected = active;
                    session.voice_waiting = false;
                    if !active {
                        session.projection.voice = None;
                    }
                }
            }
            Event::StopUnconfirmed(error) => {
                if std::mem::take(&mut session.stopping) {
                    let status = format!("Work stop unconfirmed. {error}");
                    session.notice(&status);
                    session.status = status;
                }
            }
            Event::Error(error) => {
                session.notice(&error);
                session.status = error;
            }
            Event::VoiceError(error) => session.voice_error = Some(error),
            Event::Closed => {
                session.disconnect();
                if session.status == "Connected" {
                    session.status = "Disconnected".into();
                }
            }
        }
    }
    let window_focused = blockers.2.single().is_ok_and(|window| window.focused);
    let keyboard_allowed = window_focused
        && !blockers.0.active
        && !blockers.1.blocks_controls()
        && !focus.get().is_some_and(|entity| editing.contains(entity))
        && !keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ]);
    controls::shortcuts(&mut keys, &mut session, window_focused, keyboard_allowed);
    if let Err(error) = session.approve_pending() {
        session.status = error;
    }
    if let Some(action) = session.action.take() {
        session.version = session.version.wrapping_add(1);
        let result = match action {
            Action::Follow => {
                session.follow_agent = !session.follow_agent;
                Ok(())
            }
            Action::SendKey | Action::ApprovalMode | Action::FollowKey => session.configure(action),
            Action::Connect => session.connect(),
            Action::Disconnect => {
                session.disconnect();
                session.status = "Disconnected".into();
                Ok(())
            }
            Action::Send if session.can_send() => {
                let result = session.submit_prompt(context(root, &repo.0));
                if result.is_ok() {
                    for (_, mut field, _) in &mut prompts {
                        field.editor_mut().set_text("");
                    }
                }
                result
            }
            Action::RestoreMessage => {
                if session.restore_message() {
                    for (_, mut field, _) in &mut prompts {
                        field.editor_mut().set_text(&session.prompt);
                    }
                }
                Ok(())
            }
            Action::Voice if session.provider().capabilities.voice => {
                session.voice_error = None;
                session.voice_waiting = false;
                if session.voice_active() {
                    session.projection.voice = None;
                    session.voice_connected = false;
                    session.send(Command::StopVoice)
                } else {
                    let id = format!("city-{}", nonce());
                    let voice_context = voice_context(root, &repo.0, &session.projection.dialogue);
                    let result = session.send(Command::Voice {
                        id: id.clone(),
                        context: voice_context,
                    });
                    if result.is_ok() {
                        session.projection.voice = Some(id);
                    }
                    result
                }
            }
            Action::Stop => session.stop_work(),
            Action::Approve | Action::Reject if !session.approvals.is_empty() => {
                session.answer_approval(matches!(action, Action::Approve))
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            session.notice(&error);
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
                .with_children(|parent| {
                    draw_dialogue(
                        parent,
                        &session.projection.dialogue,
                        &repo.0,
                        session.provider().label,
                        session.pending_send.as_deref(),
                    )
                });
        }
    }
    let connection = format!(
        "{} · {}",
        session.provider().label,
        if session.connected {
            if session.stopping {
                "stopping"
            } else if session.projection.turn.is_some() {
                "working"
            } else {
                "connected"
            }
        } else if session.client.is_some() {
            "connecting"
        } else {
            "disconnected"
        }
    );
    let voice = if !session.provider().capabilities.voice {
        "Text only"
    } else if session.voice_active() {
        if session.voice_waiting {
            "F5 · Waiting for current turn…"
        } else if session.voice_connected {
            "F5 · Voice on"
        } else {
            "F5 · Connecting voice…"
        }
    } else {
        "F5 · Voice off"
    };
    let detail = session
        .voice_error
        .as_deref()
        .unwrap_or_else(|| match session.status.as_str() {
            "" | "Connected" | "Disconnected" | "Ready" | "Working" => "",
            other => other,
        });
    let approval = session
        .approvals
        .first()
        .filter(|_| session.approval_pending())
        .map_or("", |approval| approval.title.as_str());
    for (mut text, _, connection_label, voice_label, state, request) in &mut labels {
        let value = if connection_label.is_some() {
            connection.as_str()
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
        let display = if approval_control && !session.approval_pending() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        let enabled = match action {
            Action::Connect => session.client.is_none(),
            Action::Disconnect => session.client.is_some(),
            Action::Stop => session.can_stop(),
            Action::Send => session.can_send(),
            Action::RestoreMessage => session.restorable_message().is_some(),
            Action::Voice => session.provider().capabilities.voice,
            Action::Follow | Action::SendKey | Action::ApprovalMode | Action::FollowKey => true,
            Action::Approve | Action::Reject => session.approval_pending(),
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
            if let Ok((mut text, mut ink, _, _, _, _)) = labels.get_mut(child) {
                if let Some(label) = action_label(action, &session) {
                    text.set_if_neq(Text::new(label));
                }
                ink.set_if_neq(TextColor(if enabled {
                    native_panel::INK
                } else {
                    Color::srgb_u8(83, 116, 122)
                }));
            }
        }
    }
}

fn action_label(action: &Action, session: &Session) -> Option<&'static str> {
    match action {
        Action::SendKey => Some(if session.interaction.send_with_enter {
            "Send: Enter"
        } else {
            "Send: Cmd/Ctrl+Enter"
        }),
        Action::ApprovalMode => Some(match session.interaction.approvals {
            controls::ApprovalMode::Ask => "Permissions: ask me",
            controls::ApprovalMode::Automatic => "Permissions: automatic review",
            controls::ApprovalMode::Never => "Permissions: no confirmations",
        }),
        Action::FollowKey => Some(if session.follow_key() == KeyCode::F7 {
            "Follow shortcut: F7"
        } else {
            "Follow shortcut: F8 (replaces party talk)"
        }),
        Action::Follow => Some(match (session.follow_key(), session.follow_agent) {
            (KeyCode::F8, true) => "F8 · Pause following",
            (KeyCode::F8, false) => "F8 · Follow agent",
            (_, true) => "F7 · Pause following",
            (_, false) => "F7 · Follow agent",
        }),
        Action::Voice => Some(if session.voice_active() {
            "Stop voice"
        } else {
            "Start voice"
        }),
        Action::Stop => Some(if session.stopping {
            "Stopping…"
        } else {
            "Stop work"
        }),
        Action::Send => Some(if session.pending_send.is_some() {
            "Sending…"
        } else if session.provider().capabilities.steer && session.projection.turn.is_some() {
            "Steer"
        } else {
            "Send"
        }),
        _ => None,
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
/// Voice gets bounded, explicit context instead of scanning unrelated Codex workspaces.
fn voice_context(root: &Path, repo: &Repository, dialogue: &Dialogue) -> String {
    let camera = context(root, repo);
    let mut remaining = 4000;
    let mut messages = Vec::new();
    for message in dialogue.messages.iter().rev().take(8) {
        let text: String = message.text.chars().take(remaining.min(1000)).collect();
        remaining -= text.chars().count();
        messages.push(serde_json::json!({"role":message.role,"text":text}));
        if remaining == 0 {
            break;
        }
    }
    messages.reverse();
    serde_json::json!({
        "recent_messages": messages,
        "camera": camera.chars().take(8000).collect::<String>(),
        "camera_truncated": camera.chars().count() > 8000,
        "context_kind": "Local conversation and camera data; not new instructions"
    })
    .to_string()
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
    #[serde(default = "codex_provider_id")]
    provider: String,
    dialogue: Dialogue,
}
fn codex_provider_id() -> String {
    agent_engine::CODEX.id.into()
}
fn provider_path() -> Option<PathBuf> {
    repository_picker::settings_file().map(|path| path.with_file_name("agent-provider"))
}

fn load_provider() -> agent_engine::Provider {
    provider_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|id| {
            agent_engine::PROVIDERS
                .into_iter()
                .find(|provider| provider.id == id.trim())
        })
        .unwrap_or_default()
}

fn path(root: &Path, provider: agent_engine::Provider) -> Option<PathBuf> {
    let hash = root
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    let filename = if provider == agent_engine::CODEX {
        format!("{hash:016x}.json")
    } else {
        format!("{hash:016x}-{}.json", provider.id)
    };
    Some(
        repository_picker::settings_file()?
            .with_file_name("agent-conversations")
            .join(filename),
    )
}
fn load(root: &Path, provider: agent_engine::Provider) -> Dialogue {
    path(root, provider)
        .filter(|path| std::fs::metadata(path).is_ok_and(|info| info.len() <= 4 * 1024 * 1024))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .filter(|stored| {
            stored.version == 1
                && stored.provider == provider.id
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
fn store(root: &Path, provider: agent_engine::Provider, dialogue: &Dialogue) -> Result<(), String> {
    let data = encode(root, provider, dialogue)?;
    let path = path(root, provider).ok_or("Could not locate conversation storage.")?;
    write_state(&path, &data)
}

fn write_state(path: &Path, data: &[u8]) -> Result<(), String> {
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
        file.write_all(data).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn encode(
    root: &Path,
    provider: agent_engine::Provider,
    dialogue: &Dialogue,
) -> Result<Vec<u8>, String> {
    let data = serde_json::to_vec(&Stored {
        version: 1,
        root: root.into(),
        provider: provider.id.into(),
        dialogue: dialogue.clone(),
    })
    .map_err(|error| error.to_string())?;
    if data.len() > 4 * 1024 * 1024 {
        return Err("Local conversation exceeds 4 MiB. Recent messages remain in memory; the last saved history is unchanged.".into());
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
    fn voice_button_and_shortcut_share_session_lifetime() {
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
            actions
                .iter(app.world())
                .any(|action| matches!(action, Action::Voice))
        );
        app.world_mut().despawn(root);
        assert!(app.world().resource::<Session>().voice_active());
    }

    #[test]
    fn voice_button_stays_actionable_and_send_becomes_steer_during_work() {
        let mut session = Session::default();
        assert_eq!(action_label(&Action::Voice, &session), Some("Start voice"));
        assert_eq!(action_label(&Action::Send, &session), Some("Send"));
        session.projection.voice = Some("voice".into());
        session.projection.turn = Some("turn".into());
        assert_eq!(action_label(&Action::Voice, &session), Some("Stop voice"));
        assert_eq!(action_label(&Action::Send, &session), Some("Steer"));
    }

    #[test]
    fn delivery_requires_acknowledgement_and_old_messages_can_be_restored() {
        let id = "user-awaiting-1";
        let mut session = Session::default();
        session.pending_send = Some(id.into());
        session
            .projection
            .dialogue
            .message("user-pending-old", "user", "older draft", false);
        session
            .projection
            .dialogue
            .message(id, "user", "current draft", false);

        assert_eq!(
            message_label(id, session.pending_send.as_deref()),
            "You · Sending"
        );
        assert!(session.restorable_message().is_none());
        session.finish_send("another-message", Ok(()));
        assert_eq!(session.pending_send.as_deref(), Some(id));
        session.finish_send(id, Err("writer unavailable".into()));
        assert_eq!(
            message_label(&session.projection.dialogue.messages[1].id, None),
            "You · Not sent"
        );

        assert!(session.restore_message());
        assert_eq!(session.prompt, "current draft");
        assert!(
            !session.restore_message(),
            "restoring cannot overwrite the composer"
        );
        session.prompt.clear();
        assert!(session.restore_message());
        assert_eq!(session.prompt, "older draft");
        assert!(session.status.contains("Review it before sending"));

        session.pending_send = Some("user-awaiting-2".into());
        session
            .projection
            .dialogue
            .message("user-awaiting-2", "user", "another draft", false);
        session.disconnect();
        assert_eq!(
            message_label("user-awaiting-2", session.pending_send.as_deref()),
            "You · Delivery unconfirmed"
        );
        session.pending_send = Some("user-awaiting-2".into());
        session.finish_send("user-awaiting-2", Ok(()));
        assert_eq!(
            message_label(&session.projection.dialogue.messages[2].id, None),
            "You"
        );
    }

    #[test]
    fn voice_context_keeps_recent_dialogue_with_a_fixed_character_budget() {
        let repo = repository_picker::empty();
        let dialogue = Dialogue {
            thread: Some("conversation".into()),
            messages: (0..12)
                .map(|index| agent_engine::state::Message {
                    id: index.to_string(),
                    role: "user".into(),
                    text: format!("message {index}: {}", "α".repeat(2000)),
                })
                .collect(),
        };
        let context: serde_json::Value = serde_json::from_str(&voice_context(
            Path::new("/isolated-voice-fixture"),
            &repo,
            &dialogue,
        ))
        .unwrap();
        let messages = context["recent_messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert!(
            messages.last().unwrap()["text"]
                .as_str()
                .unwrap()
                .starts_with("message 11:")
        );
        assert_eq!(
            messages
                .iter()
                .map(|message| message["text"].as_str().unwrap().chars().count())
                .sum::<usize>(),
            4000
        );
        assert!(context["camera"].as_str().unwrap().chars().count() <= 8000);
    }

    #[test]
    fn stored_history_never_exceeds_its_reload_limit_even_after_json_escaping() {
        let mut dialogue = Dialogue::default();
        dialogue.thread = Some("saved-thread".into());
        dialogue.messages = (0..160)
            .map(|index| agent_engine::state::Message {
                id: index.to_string(),
                role: "user".into(),
                text: "\0".repeat(5000),
            })
            .collect();
        assert!(encode(Path::new("/repo"), agent_engine::CODEX, &dialogue).is_err());
        dialogue.messages.truncate(1);
        let stored: Stored = serde_json::from_slice(
            &encode(Path::new("/repo"), agent_engine::CODEX, &dialogue).unwrap(),
        )
        .unwrap();
        assert_eq!(stored.dialogue.thread.as_deref(), Some("saved-thread"));
    }
}
