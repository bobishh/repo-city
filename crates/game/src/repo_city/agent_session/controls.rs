//! Keyboard controls and persisted interaction preferences for the agent session.
use super::*;
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::FocusedInput,
    picking::Pickable,
    text::{LineBreak, TextEdit},
    ui_widgets::{ControlOrientation, ScrollArea, Scrollbar, ScrollbarThumb},
};

pub(super) fn register(app: &mut App) {
    app.add_observer(prompt_input)
        .add_systems(Startup, spawn_banner)
        .add_systems(Update, banner.after(super::update))
        .add_systems(
            PostUpdate,
            target_banner.before(bevy::ui::UiSystems::Prepare),
        );
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

pub(crate) fn settings(parent: &mut ChildSpawnerCommands, provider: agent_engine::Provider) {
    models::settings(parent);
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
    if provider == agent_engine::CODEX {
        let button = native_panel::button::<Action, Session>(
            parent,
            "Enable full access",
            native_panel::INK,
            Action::FullAccess,
        );
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
struct BannerRoot;
#[derive(Component)]
struct Banner;
#[derive(Component)]
struct BannerText;
#[derive(Component)]
struct BannerCode;
#[derive(Component)]
struct BannerDetails;

pub(super) fn spawn_banner(mut commands: Commands) {
    commands
        .spawn((
            BannerRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Vh(8.),
                width: percent(100.),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Start,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                Banner,
                Node {
                    width: px(780.),
                    max_width: Val::Vw(94.),
                    max_height: Val::Vh(84.),
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10.),
                    padding: UiRect::all(px(12.)),
                    border: UiRect::all(px(1.)),
                    ..default()
                },
                GlobalZIndex(1_200),
                BackgroundColor(Color::srgb_u8(10, 29, 36)),
                BorderColor::all(Color::srgb_u8(239, 192, 121)),
            ))
            .with_children(banner_content);
        });
}

fn target_banner(
    mut commands: Commands,
    canvas: Option<Res<canvas::Canvas>>,
    banners: Query<(Entity, Option<&bevy::ui::UiTargetCamera>), With<BannerRoot>>,
) {
    let target = canvas.as_ref().and_then(|canvas| canvas.overlay_camera());
    for (entity, current) in &banners {
        if current.map(|camera| camera.0) == target {
            continue;
        }
        if let Some(camera) = target {
            commands
                .entity(entity)
                .insert(bevy::ui::UiTargetCamera(camera));
        } else {
            commands.entity(entity).remove::<bevy::ui::UiTargetCamera>();
        }
    }
}

fn banner_content(card: &mut ChildSpawnerCommands) {
    card.spawn((
        Text::new("Agent permission"),
        TextFont::from_font_size(17.),
        TextColor(Color::srgb_u8(239, 192, 121)),
        Node {
            flex_shrink: 0.,
            ..default()
        },
        Pickable::IGNORE,
    ));
    card.spawn(Node {
        min_height: px(0.),
        max_height: px(360.),
        column_gap: px(8.),
        ..default()
    })
    .with_children(|row| {
        let details = row
            .spawn((
                BannerDetails,
                ScrollArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.,
                    min_width: px(0.),
                    width: px(0.),
                    row_gap: px(8.),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|body| {
                body.spawn((
                    BannerText,
                    Text::default(),
                    TextLayout::linebreak(LineBreak::WordOrCharacter),
                    native_panel::panel_font(),
                    TextColor(native_panel::INK),
                    Node {
                        flex_shrink: 0.,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                body.spawn((
                    BannerCode,
                    Text::default(),
                    TextLayout::linebreak(LineBreak::AnyCharacter),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(222, 238, 240)),
                    BackgroundColor(Color::srgb_u8(7, 20, 26)),
                    Node {
                        display: Display::None,
                        flex_shrink: 0.,
                        padding: UiRect::all(px(8.)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            })
            .id();
        row.spawn((
            Scrollbar::new(details, ControlOrientation::Vertical, 22.),
            Node {
                width: px(9.),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(Color::srgb_u8(18, 42, 49)),
        ))
        .with_child((
            ScrollbarThumb::default(),
            BackgroundColor(Color::srgb_u8(73, 132, 135)),
        ));
    });
    card.spawn(Node {
        justify_content: JustifyContent::SpaceBetween,
        flex_wrap: FlexWrap::Wrap,
        flex_shrink: 0.,
        row_gap: px(6.),
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
            Color::srgb_u8(252, 188, 159),
            Action::Reject,
        );
    });
}

fn banner(
    session: Res<Session>,
    mut shown: Local<Option<String>>,
    mut banners: Query<&mut Node, (With<Banner>, Without<BannerCode>)>,
    mut hints: Query<&mut Text, (With<Hint>, Without<BannerText>, Without<BannerCode>)>,
    mut summaries: Query<&mut Text, (With<BannerText>, Without<BannerCode>)>,
    mut code_blocks: Query<(&mut Text, &mut Node), With<BannerCode>>,
    mut details: Query<&mut ScrollPosition, With<BannerDetails>>,
) {
    let approval = session
        .approvals
        .first()
        .filter(|_| session.approval_pending());
    let token = approval.map(|approval| approval.token.as_str());
    for mut node in &mut banners {
        let display = if approval.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut hint in &mut hints {
        hint.set_if_neq(Text::new(if session.interaction.send_with_enter {
            "Enter sends · Shift+Enter newline · F5 voice"
        } else {
            "Cmd/Ctrl+Enter sends · Enter newline · F5 voice"
        }));
    }
    if shown.as_deref() == token {
        return;
    }
    *shown = token.map(str::to_owned);
    for mut scroll in &mut details {
        scroll.0 = Vec2::ZERO;
    }
    for mut text in &mut summaries {
        text.set_if_neq(Text::new(
            approval.map_or("", |approval| approval.summary.as_str()),
        ));
    }
    let command = approval.and_then(|approval| approval.command.as_deref());
    for (mut text, mut node) in &mut code_blocks {
        text.set_if_neq(Text::new(command.unwrap_or_default()));
        node.display = if command.is_some() {
            Display::Flex
        } else {
            Display::None
        };
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
            self.request_approval(Approval {
                token: "smoke-only".into(),
                summary: "Local controls preview. No command will run.".into(),
                command: Some("printf '%s\\n' 'Preview command'".into()),
            })
            .expect("preview request");
        } else {
            self.answer_approval(false).expect("preview rejection");
        }
    }

    pub(crate) fn follow_key(&self) -> KeyCode {
        self.interaction.follow_key.code()
    }
    pub(crate) fn approval_pending(&self) -> bool {
        self.approval_mode() != ApprovalMode::Never && !self.approvals.is_empty()
    }

    pub(super) fn approval_mode(&self) -> ApprovalMode {
        if self.selected != agent_engine::CODEX
            && self.interaction.approvals == ApprovalMode::FullAccess
        {
            ApprovalMode::Ask
        } else {
            self.interaction.approvals
        }
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
        details.body = approval.description();
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
                next.approvals = match self.approval_mode() {
                    ApprovalMode::Ask => ApprovalMode::Automatic,
                    ApprovalMode::Automatic => ApprovalMode::Never,
                    ApprovalMode::Never | ApprovalMode::FullAccess => ApprovalMode::Ask,
                }
            }
            Action::FullAccess if self.selected == agent_engine::CODEX => {
                next.approvals = if next.approvals == ApprovalMode::FullAccess {
                    ApprovalMode::Ask
                } else {
                    ApprovalMode::FullAccess
                };
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
    fn request(token: &str, summary: &str) -> Approval {
        Approval {
            token: token.into(),
            summary: summary.into(),
            command: None,
        }
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
                .request_approval(request("token", "Read file"))
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
                .request_approval(request("retry", "Read file"))
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
            .request_approval(request("token", "Read file"))
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
            .request_approval(request("token", "Read file"))
            .unwrap();
        assert!(receive.try_recv().is_err());
        assert!(session.answer_approval(false).is_err());
        assert_eq!(session.approvals[0].token, "token");
    }
    #[test]
    fn long_approval_scrolls_inside_a_small_window_with_both_actions_visible() {
        use bevy::asset::AssetApp;
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        use bevy::math::Affine2;
        use bevy::ui::CalculatedClip;

        let (mut session, receive) = session(false);
        session.interaction.approvals = ApprovalMode::Ask;
        let summary = "Review this command";
        let detail = format!(
            "printf '%s' '{}';\n  printf '%s' 'last line'",
            "x".repeat(1200)
        );
        session
            .request_approval(Approval {
                command: Some(detail.clone()),
                ..request("first", summary)
            })
            .unwrap();
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::image::ImagePlugin::default(),
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::picking::DefaultPickingPlugins,
            bevy::text::TextPlugin,
            bevy::time::TimePlugin,
            bevy::ui::UiPlugin,
            bevy::ui_widgets::ScrollAreaPlugin,
            bevy::ui_widgets::ScrollbarPlugin,
            WindowPlugin {
                primary_window: None,
                ..default()
            },
        ))
        .init_asset::<TextureAtlasLayout>()
        .insert_resource(session)
        .add_systems(Startup, spawn_banner)
        .add_systems(Update, banner);
        let size = UVec2::new(440, 260);
        app.world_mut().spawn(Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: size,
                    scale_factor: 1.,
                }),
                ..default()
            },
            ..default()
        });
        app.finish();
        app.cleanup();
        load_ui_font(&mut app.world_mut().resource_mut::<Assets<Font>>());
        app.update();
        app.update();

        let world = app.world_mut();
        let summary_text = world
            .query_filtered::<&Text, With<BannerText>>()
            .single(world)
            .unwrap();
        assert_eq!(summary_text.as_str(), summary);
        let (text_entity, text) = world
            .query_filtered::<(Entity, &Text), With<BannerCode>>()
            .single(world)
            .unwrap();
        assert_eq!(text.as_str(), detail);
        let clipping = world.get::<CalculatedClip>(text_entity).unwrap().clone();
        let viewport = world
            .query_filtered::<(Entity, &ScrollPosition, &ComputedNode), With<BannerDetails>>()
            .single(world)
            .unwrap();
        let scroll_area = viewport.0;
        assert!(
            tablet_scroll::range(viewport.2) > 0.,
            "viewport {:?}, content {:?}, code {:?}",
            viewport.2.size(),
            viewport.2.content_size(),
            world.get::<ComputedNode>(text_entity).unwrap().size()
        );
        let bounds = |node: &ComputedNode, transform: &UiGlobalTransform| {
            let center: Affine2 = (*transform).into();
            Rect::from_center_size(center.translation, node.size())
        };
        let actions: Vec<_> = world
            .query::<(&Action, &ComputedNode, &UiGlobalTransform)>()
            .iter(world)
            .map(|(action, node, transform)| (*action, bounds(node, transform)))
            .collect();
        assert_eq!(actions.len(), 2);
        for (_, rect) in &actions {
            assert!(!clipping.contains_point(Vec2::new(rect.center().x, rect.min.y)));
            assert!(rect.max.x <= size.x as f32 && rect.max.y <= size.y as f32);
        }

        world.get_mut::<ScrollPosition>(scroll_area).unwrap().y = 40.;
        app.update();
        assert_eq!(
            app.world().get::<ScrollPosition>(scroll_area).unwrap().y,
            40.
        );
        let mut session = app.world_mut().resource_mut::<Session>();
        session
            .request_approval(request("second", "Review file changes"))
            .unwrap();
        session.answer_approval(false).unwrap();
        assert!(!receive.try_recv().unwrap());
        app.update();
        assert_eq!(
            app.world().get::<ScrollPosition>(scroll_area).unwrap().y,
            0.
        );
        assert_eq!(
            app.world().get::<Node>(text_entity).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<Session>()
            .answer_approval(false)
            .unwrap();
        app.update();
        let world = app.world_mut();
        let node = world
            .query_filtered::<&Node, With<Banner>>()
            .single(world)
            .unwrap();
        assert_eq!(node.display, Display::None);
    }
    #[test]
    fn approval_shortcuts_preserve_typed_y_and_n_and_take_priority_outside_input() {
        let (mut session, _) = session(false);
        session.approvals.push(request("t", "Read"));
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
    pub(super) fn request_approval(&mut self, approval: Approval) -> Result<(), String> {
        self.notice(&format!(
            "Agent permission requested\n{}",
            approval.description()
        ));
        self.approvals.push(approval);
        self.approve_pending()
    }

    pub(super) fn approve_pending(&mut self) -> Result<(), String> {
        if self.approval_mode() == ApprovalMode::Never {
            while !self.approvals.is_empty() {
                self.answer_approval(true)?;
            }
        }
        Ok(())
    }
}
