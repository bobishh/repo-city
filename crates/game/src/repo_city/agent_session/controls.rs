//! Keyboard controls and persisted interaction preferences for the agent session.
use super::*;
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::FocusedInput,
    text::TextEdit,
};

pub(super) fn register(app: &mut App) {
    app.add_observer(prompt_input)
        .add_systems(Startup, spawn_banner)
        .add_systems(Update, banner.after(super::update));
}

pub(super) use agent_engine::ApprovalMode;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum FollowKey {
    #[default]
    F7,
    F8,
}
impl FollowKey {
    pub(super) fn code(self) -> KeyCode {
        match self {
            Self::F7 => KeyCode::F7,
            Self::F8 => KeyCode::F8,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Settings {
    pub send_with_enter: bool,
    pub approvals: ApprovalMode,
    pub follow_key: FollowKey,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            send_with_enter: true,
            approvals: ApprovalMode::default(),
            follow_key: FollowKey::F7,
        }
    }
}
impl Settings {
    fn path() -> Result<PathBuf, String> {
        repository_picker::settings_file()
            .map(|path| path.with_file_name("agent-interaction.json"))
            .ok_or_else(|| "Could not locate agent settings.".into())
    }
    pub(super) fn load() -> Result<Self, String> {
        match std::fs::read(Self::path()?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("Could not read agent interaction settings: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!(
                "Could not read agent interaction settings: {error}"
            )),
        }
    }
    pub(super) fn save(self) -> Result<(), String> {
        write_state(
            &Self::path()?,
            &serde_json::to_vec_pretty(&self).map_err(|error| error.to_string())?,
        )
    }
}

pub(super) fn prompt_input(
    mut event: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut fields: Query<&mut EditableText, With<Prompt>>,
) {
    let Ok(mut field) = fields.get_mut(event.focused_entity) else {
        return;
    };
    if field.is_composing()
        || event.input.logical_key != Key::Enter
        || !event.input.state.is_pressed()
    {
        return;
    }
    event.propagate(false);
    if event.input.repeat {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let command = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    if !shift && (session.interaction.send_with_enter || command) {
        session.action = Some(Action::Send);
    } else {
        field.queue_edit(TextEdit::Insert("\n".into()));
    }
}

pub(super) fn shortcuts(
    keys: &mut ButtonInput<KeyCode>,
    session: &mut Session,
    window_focused: bool,
    keyboard_allowed: bool,
) {
    if !window_focused {
        return;
    }
    if keys.just_pressed(session.follow_key()) {
        session.action = Some(Action::Follow);
    }
    if keyboard_allowed && session.approval_pending() {
        if keys.just_pressed(KeyCode::KeyY) {
            session.action = Some(Action::Approve);
            keys.clear_just_pressed(KeyCode::KeyY);
        } else if keys.just_pressed(KeyCode::KeyN) {
            session.action = Some(Action::Reject);
            keys.clear_just_pressed(KeyCode::KeyN);
        }
    }
}

pub(crate) fn settings(parent: &mut ChildSpawnerCommands) {
    for (label, action) in [
        ("Send: Enter", Action::SendKey),
        ("Permissions: automatic review", Action::ApprovalMode),
        ("Follow shortcut: F7", Action::FollowKey),
    ] {
        let button =
            native_panel::button::<Action, Session>(parent, label, native_panel::INK, action);
        parent.commands().entity(button).insert(Node {
            width: percent(100.),
            min_height: px(34.),
            ..default()
        });
    }
    native_panel::text(
        parent,
        "Permissions applies to every provider. No confirmations answers tool requests automatically. Codex's server policy applies on reconnect. Ask mode uses Y/N.",
        native_panel::INK,
    );
}

#[derive(Component)]
pub(super) struct Hint;
#[derive(Component)]
struct Banner;
#[derive(Component)]
struct BannerText;

pub(super) fn spawn_banner(mut commands: Commands) {
    commands
        .spawn((
            Banner,
            Node {
                position_type: PositionType::Absolute,
                left: percent(23.),
                top: px(86.),
                width: percent(54.),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(8.),
                padding: UiRect::all(px(12.)),
                border: UiRect::all(px(1.)),
                ..default()
            },
            GlobalZIndex(70),
            BackgroundColor(Color::srgb_u8(10, 29, 36)),
            BorderColor::all(Color::srgb_u8(239, 192, 121)),
        ))
        .with_children(|body| {
            body.spawn((
                BannerText,
                Text::new(""),
                native_panel::panel_font(),
                TextColor(native_panel::INK),
                Node {
                    max_height: px(88.),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            body.spawn(Node {
                column_gap: px(8.),
                ..default()
            })
            .with_children(|row| {
                native_panel::button::<Action, Session>(
                    row,
                    "Y · Approve once",
                    native_panel::INK,
                    Action::Approve,
                );
                native_panel::button::<Action, Session>(
                    row,
                    "N · Reject",
                    native_panel::INK,
                    Action::Reject,
                );
            });
        });
}

fn banner(
    session: Res<Session>,
    mut banners: Query<&mut Node, With<Banner>>,
    mut labels: Query<(&mut Text, Option<&Hint>, Option<&BannerText>)>,
) {
    let approval = session
        .approvals
        .first()
        .filter(|_| session.approval_pending());
    for mut node in &mut banners {
        node.display = if approval.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (mut text, hint, banner) in &mut labels {
        if hint.is_some() {
            let value = if session.interaction.send_with_enter {
                "Enter sends · Shift+Enter newline · F5 voice"
            } else {
                "Cmd/Ctrl+Enter sends · Enter newline · F5 voice"
            };
            text.set_if_neq(Text::new(value));
        } else if banner.is_some() {
            let value = approval.map_or(String::new(), |approval| {
                format!(
                    "Agent permission · Y approve · N reject\n{}",
                    approval.title.chars().take(650).collect::<String>()
                )
            });
            text.set_if_neq(Text::new(value));
        }
    }
}

impl Session {
    pub(crate) fn check_smoke_banner(world: &mut World) {
        let pending = world.resource::<Session>().approval_pending();
        let mut query = world.query_filtered::<(&Node, &ComputedNode), With<Banner>>();
        let (node, computed) = query.single(world).expect("approval banner exists");
        println!(
            "Approval banner: pending={pending}, display={:?}, size={:?}",
            node.display,
            computed.size()
        );
        assert!(pending, "preview request disappeared");
        assert_eq!(node.display, Display::Flex);
        assert!(
            computed.size().min_element() > 10.,
            "approval banner has no layout"
        );
    }
    pub(crate) fn smoke_controls(&mut self, root: &Path, pending: bool) {
        struct PreviewEngine;
        impl Engine for PreviewEngine {
            fn provider(&self) -> agent_engine::Provider {
                agent_engine::CODEX
            }
            fn poll(&self, _: &mut Projection) -> Vec<Event> {
                vec![]
            }
            fn send(&self, _: Command) -> Result<(), String> {
                Ok(())
            }
        }
        if pending {
            self.root = Some(root.into());
            self.client = Some(Box::new(PreviewEngine));
            self.connected = true;
            self.interaction.approvals = ApprovalMode::Ask;
            self.prompt = "First line\nSecond line".into();
            self.request_approval(
                "smoke-only".into(),
                "Local controls preview. No command will run.".into(),
            )
            .expect("preview request");
        } else {
            self.answer_approval(false).expect("preview rejection");
        }
    }

    pub(crate) fn follow_key(&self) -> KeyCode {
        self.interaction.follow_key.code()
    }
    pub(crate) fn approval_pending(&self) -> bool {
        self.interaction.approvals != ApprovalMode::Never && !self.approvals.is_empty()
    }
    pub(super) fn notice(&mut self, text: &str) {
        if self
            .projection
            .dialogue
            .messages
            .last()
            .is_some_and(|row| row.role == "system" && row.text == text)
        {
            return;
        }
        self.projection.dialogue.message(
            &format!("session-event-{}", nonce()),
            "system",
            text,
            false,
        );
        self.dirty = true;
        self.version = self.version.wrapping_add(1);
    }
    pub(super) fn answer_approval(&mut self, accept: bool) -> Result<(), String> {
        let Some(approval) = self.approvals.first() else {
            return Ok(());
        };
        self.send(Command::Approval {
            token: approval.token.clone(),
            accept,
        })?;
        let approval = self.approvals.remove(0);
        let thread = self
            .projection
            .dialogue
            .thread
            .as_deref()
            .unwrap_or("local-session");
        let title = if accept {
            "Permission approved"
        } else {
            "Permission rejected"
        };
        let mut details =
            agent_engine::activity::Details::new(agent_engine::activity::Kind::Session, title);
        details.body = approval.title;
        let phase = if accept {
            agent_engine::activity::Phase::Completed
        } else {
            agent_engine::activity::Phase::Failed
        };
        self.projection.timeline.record(
            thread,
            &format!("request:{}", approval.token),
            details,
            phase,
        );
        self.notice(if accept {
            "Approval sent to agent."
        } else {
            "Rejection sent to agent."
        });
        Ok(())
    }
    pub(super) fn configure(&mut self, action: Action) -> Result<(), String> {
        let mut next = self.interaction;
        match action {
            Action::SendKey => next.send_with_enter = !next.send_with_enter,
            Action::ApprovalMode => {
                next.approvals = match next.approvals {
                    ApprovalMode::Ask => ApprovalMode::Automatic,
                    ApprovalMode::Automatic => ApprovalMode::Never,
                    ApprovalMode::Never => ApprovalMode::Ask,
                }
            }
            Action::FollowKey => {
                next.follow_key = if next.follow_key == FollowKey::F7 {
                    FollowKey::F8
                } else {
                    FollowKey::F7
                }
            }
            _ => return Ok(()),
        }
        next.save()?;
        self.interaction = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{input::ButtonState, text::TextEdit};

    fn enter(shift: bool, repeat: bool, send_with_enter: bool) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Session>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_observer(prompt_input);
        app.world_mut()
            .resource_mut::<Session>()
            .interaction
            .send_with_enter = send_with_enter;
        if shift {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ShiftLeft);
        }
        let field = app
            .world_mut()
            .spawn((EditableText::default(), Prompt::default()))
            .id();
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().trigger(FocusedInput::new(
            field,
            KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: ButtonState::Pressed,
                text: Some("\r".into()),
                repeat,
                window,
            },
            window,
        ));
        (app, field)
    }

    #[test]
    fn plain_enter_requests_send_without_inserting_a_newline() {
        let (app, field) = enter(false, false, true);
        assert!(matches!(
            app.world().resource::<Session>().action,
            Some(Action::Send)
        ));
        assert!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .pending_edits
                .is_empty()
        );
    }

    #[test]
    fn shift_enter_inserts_a_newline_without_requesting_send() {
        let (app, field) = enter(true, false, true);
        assert!(app.world().resource::<Session>().action.is_none());
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .pending_edits,
            [TextEdit::Insert("\n".into())]
        );
    }

    #[test]
    fn command_enter_mode_keeps_plain_enter_as_a_newline() {
        let (app, field) = enter(false, false, false);
        assert!(app.world().resource::<Session>().action.is_none());
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .pending_edits,
            [TextEdit::Insert("\n".into())]
        );
    }

    #[test]
    fn holding_enter_does_not_repeat_submission() {
        let (app, field) = enter(false, true, true);
        assert!(app.world().resource::<Session>().action.is_none());
        assert!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .pending_edits
                .is_empty()
        );
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use std::sync::mpsc;

    struct QueueEngine {
        provider: agent_engine::Provider,
        send: mpsc::Sender<bool>,
        fail: bool,
    }
    impl Engine for QueueEngine {
        fn provider(&self) -> agent_engine::Provider {
            self.provider
        }
        fn poll(&self, _: &mut Projection) -> Vec<Event> {
            vec![]
        }
        fn send(&self, command: Command) -> Result<(), String> {
            if self.fail {
                return Err("Queue full".into());
            }
            let Command::Approval { accept, .. } = command else {
                panic!("Unexpected command");
            };
            self.send.send(accept).unwrap();
            Ok(())
        }
    }
    fn session(fail: bool) -> (Session, mpsc::Receiver<bool>) {
        let (send, receive) = mpsc::channel();
        let mut session = Session::default();
        session.client = Some(Box::new(QueueEngine {
            provider: agent_engine::CODEX,
            send,
            fail,
        }));
        (session, receive)
    }
    #[test]
    fn no_confirmation_mode_answers_tool_requests_for_every_provider() {
        for provider in agent_engine::PROVIDERS {
            let (send, receive) = mpsc::channel();
            let mut session = Session::default();
            session.client = Some(Box::new(QueueEngine {
                provider,
                send,
                fail: false,
            }));
            session.interaction = serde_json::from_str(r#"{"approvals":"never"}"#).unwrap();
            session
                .request_approval("token".into(), "Read file".into())
                .unwrap();
            assert!(
                receive.try_recv().unwrap(),
                "{} did not approve",
                provider.label
            );
            assert!(!session.approval_pending());
        }
    }
    #[test]
    fn no_confirmation_mode_retries_failed_delivery_without_capturing_flight_keys() {
        let (mut session, _) = session(true);
        session.interaction.approvals = ApprovalMode::Never;
        assert!(
            session
                .request_approval("retry".into(), "Read file".into())
                .is_err()
        );
        assert_eq!(session.approvals[0].token, "retry");
        assert!(!session.approval_pending());
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyY);
        shortcuts(&mut keys, &mut session, true, true);
        assert!(session.action.is_none());
        assert!(keys.just_pressed(KeyCode::KeyY));
        let (send, receive) = mpsc::channel();
        session.client = Some(Box::new(QueueEngine {
            provider: agent_engine::MUSE,
            send,
            fail: false,
        }));
        session.approve_pending().unwrap();
        assert!(receive.try_recv().unwrap());
        assert!(session.approvals.is_empty());
    }
    #[test]
    fn automatic_review_leaves_provider_fallback_requests_for_the_user() {
        let (mut session, receive) = session(false);
        session.interaction.approvals = ApprovalMode::Automatic;
        session
            .request_approval("token".into(), "Read file".into())
            .unwrap();
        assert!(receive.try_recv().is_err());
        assert!(session.approval_pending());
        session.answer_approval(true).unwrap();
        assert!(receive.try_recv().unwrap());
    }
    #[test]
    fn ask_mode_waits_and_failed_delivery_preserves_the_request() {
        let (mut session, receive) = session(true);
        session.interaction.approvals = ApprovalMode::Ask;
        session
            .request_approval("token".into(), "Read file".into())
            .unwrap();
        assert!(receive.try_recv().is_err());
        assert!(session.answer_approval(false).is_err());
        assert_eq!(session.approvals[0].token, "token");
    }
    #[test]
    fn approval_shortcuts_preserve_typed_y_and_n_and_take_priority_outside_input() {
        let (mut session, _) = session(false);
        session.approvals.push(Approval {
            token: "t".into(),
            title: "Read".into(),
        });
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyY);
        shortcuts(&mut keys, &mut session, true, false);
        assert!(session.action.is_none());
        assert!(keys.just_pressed(KeyCode::KeyY));
        shortcuts(&mut keys, &mut session, true, true);
        assert!(matches!(session.action, Some(Action::Approve)));
        assert!(!keys.just_pressed(KeyCode::KeyY));
    }
    #[test]
    fn follow_shortcut_works_with_the_composer_focused_but_not_in_an_inactive_window() {
        let mut session = Session::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::F7);
        shortcuts(&mut keys, &mut session, false, false);
        assert!(session.action.is_none());
        shortcuts(&mut keys, &mut session, true, false);
        assert!(matches!(session.action, Some(Action::Follow)));
        session.interaction.follow_key = FollowKey::F8;
        assert_eq!(session.follow_key(), KeyCode::F8);
    }
    #[test]
    fn interaction_preferences_round_trip_and_preserve_explicit_confirmation_mode() {
        let settings: Settings = serde_json::from_str(
            r#"{"send_with_enter":false,"approvals":"automatic","follow_key":"F8"}"#,
        )
        .unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&saved).unwrap();
        assert!(!restored.send_with_enter);
        assert!(restored.approvals == ApprovalMode::Automatic);
        assert_eq!(restored.follow_key.code(), KeyCode::F8);
        let default: Settings = serde_json::from_str("{}").unwrap();
        assert!(default.approvals == ApprovalMode::Automatic);
        assert!(default.send_with_enter);
    }
}

impl Session {
    pub(super) fn request_approval(&mut self, token: String, title: String) -> Result<(), String> {
        self.notice(&format!("Agent permission requested\n{title}"));
        self.approvals.push(Approval { token, title });
        self.approve_pending()
    }

    pub(super) fn approve_pending(&mut self) -> Result<(), String> {
        if self.interaction.approvals == ApprovalMode::Never {
            while !self.approvals.is_empty() {
                self.answer_approval(true)?;
            }
        }
        Ok(())
    }
}
