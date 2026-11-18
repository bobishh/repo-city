//! Persistent native shell and Alacritty screen displayed inside VibePad.
mod session;

use super::*;
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    picking::events::PointerPress,
};
use session::{Session, Size};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const IMMEDIATE_EXIT: Duration = Duration::from_secs(2);
const RETRY_DELAY: Duration = Duration::from_millis(200);
const MAX_IMMEDIATE_EXITS: u8 = 5;

#[derive(Default)]
struct Recovery {
    failures: u8,
    started: Option<Instant>,
    retry_at: Option<Instant>,
}

impl Recovery {
    fn ended(&mut self, now: Instant) {
        let immediate = self
            .started
            .take()
            .is_none_or(|started| now.duration_since(started) < IMMEDIATE_EXIT);
        self.failures = if immediate {
            self.failures.saturating_add(1)
        } else {
            0
        };
        self.retry_at = (self.failures < MAX_IMMEDIATE_EXITS).then_some(
            now + if immediate {
                RETRY_DELAY
            } else {
                Duration::ZERO
            },
        );
    }
    fn reopen(&mut self, now: Instant) {
        if self.failures >= MAX_IMMEDIATE_EXITS {
            self.retry_at = Some(now);
        }
        self.failures = 0;
    }
}

#[derive(Resource, Default)]
pub(super) struct Terminal {
    root: Option<PathBuf>,
    pending: Option<native_github::Job<Mutex<Session>>>,
    session: Option<Mutex<Session>>,
    image: Option<Handle<Image>>,
    error: String,
    status: String,
    action: Option<Action>,
    paste: Option<ClipboardRead>,
    pub active: bool,
    dirty: bool,
    recovery: Recovery,
    pad_open: bool,
}

#[derive(Component)]
pub(super) struct View;
#[derive(Component)]
pub(super) struct Status;
#[derive(Component, Clone, Copy)]
enum Action {
    Copy,
    Paste,
    Restart,
}
impl native_panel::Actions<Action> for Terminal {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl Terminal {
    fn maintain(&mut self, root: &std::path::Path, now: Instant, pad_open: bool) {
        if pad_open && !self.pad_open {
            self.recovery.reopen(now);
        }
        self.pad_open = pad_open;
        if let Some(result) = self.pending.as_ref().and_then(native_github::Job::take) {
            self.pending = None;
            match result {
                Ok(session) => {
                    self.session = Some(session);
                    self.recovery.started = Some(now);
                    self.status = "Shell ready".into();
                }
                Err(error) => {
                    self.error = error;
                    self.recovery.ended(now);
                    self.exit_status();
                }
            }
            self.dirty = true;
        }
        let exited = if let Some(session) = self.session_mut() {
            let changed = session.poll();
            let exited = session.exited;
            self.dirty |= changed;
            exited
        } else {
            false
        };
        if exited {
            self.session = None;
            self.recovery.ended(now);
            self.exit_status();
        }
        if self.root.as_deref() == Some(root)
            && self.session.is_none()
            && self.pending.is_none()
            && self.recovery.retry_at.is_some_and(|retry| now >= retry)
        {
            self.launch(root.to_owned());
        }
    }

    fn exit_status(&mut self) {
        self.status = if self.recovery.failures >= MAX_IMMEDIATE_EXITS {
            format!("Shell stopped after 5 immediate exits. Close and reopen VibePad, or restart the shell. {}", self.error).trim().to_owned()
        } else if self.error.is_empty() {
            "Shell exited. Starting a new session…".into()
        } else {
            format!("{}. Retrying…", self.error)
        };
    }

    fn start(&mut self, root: PathBuf) {
        self.recovery = Recovery::default();
        self.launch(root);
    }
    fn launch(&mut self, root: PathBuf) {
        self.session = None;
        self.recovery.started = None;
        self.recovery.retry_at = None;
        self.root = Some(root.clone());
        self.error.clear();
        self.status = "Starting shell…".into();
        self.dirty = true;
        self.pending = Some(native_github::Job::spawn(move || {
            Session::open(&root, None).map(Mutex::new)
        }));
    }
    fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()?.get_mut().ok()
    }
    pub(super) fn draw(
        &mut self,
        parent: &mut ChildSpawnerCommands,
        root: &std::path::Path,
    ) -> Entity {
        if self.root.as_deref() != Some(root) {
            self.start(root.to_owned());
        }
        native_panel::text(parent, &root.to_string_lossy(), native_panel::INK);
        parent
            .spawn(Node {
                column_gap: px(8.),
                ..default()
            })
            .with_children(|tools| {
                let copy = native_panel::button::<Action, Self>(
                    tools,
                    "Copy screen",
                    native_panel::INK,
                    Action::Copy,
                );
                tools.commands().entity(copy).insert(TabIndex(40));
                let paste = native_panel::button::<Action, Self>(
                    tools,
                    "Paste",
                    native_panel::INK,
                    Action::Paste,
                );
                tools.commands().entity(paste).insert(TabIndex(41));
                let restart = native_panel::button::<Action, Self>(
                    tools,
                    "Restart shell",
                    native_panel::INK,
                    Action::Restart,
                );
                tools.commands().entity(restart).insert(TabIndex(42));
            });
        native_panel::text(
            parent,
            "F6 switches shell / controls. Ctrl+Tab puts away VibePad. Tab, Esc and Ctrl+C go to the shell.",
            native_panel::INK,
        );
        parent.spawn((
            Status,
            Text::new(&self.status),
            native_panel::panel_font(),
            TextColor(native_panel::INK),
        ));
        let image = self.image.clone().unwrap_or_default();
        parent
            .spawn((
                View,
                ImageNode::new(image),
                TabIndex(50),
                Node {
                    width: percent(100.),
                    flex_grow: 1.,
                    min_height: px(0.),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(13, 17, 23)),
            ))
            .observe(
                |mut event: On<bevy::input_focus::FocusedInput<KeyboardInput>>| {
                    event.propagate(false)
                },
            )
            .observe(|event: On<PointerPress>, mut focus: ResMut<InputFocus>| {
                focus.set(event.entity, FocusCause::Pressed)
            })
            .id()
    }
}

pub(super) fn update(
    mut terminal: ResMut<Terminal>,
    tablet: Res<review_tablet::Tablet>,
    repo: Res<Repo>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    wheel: Res<AccumulatedMouseScroll>,
    mut focus: ResMut<InputFocus>,
    mut views: Query<(Entity, &ComputedNode, &mut ImageNode), With<View>>,
    mut labels: Query<&mut Text, With<Status>>,
    mut images: ResMut<Assets<Image>>,
    mut clipboard: ResMut<Clipboard>,
    mut ime: MessageReader<bevy::window::Ime>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let shown = tablet.page_name() == Some("terminal");
    let view = views.iter().next().map(|(entity, _, _)| entity);
    terminal.active = shown && focus.get() == view && view.is_some();
    terminal.maintain(&repo.0.root, Instant::now(), tablet.is_open());
    if let Some(action) = terminal.action.take() {
        match action {
            Action::Restart => terminal.start(repo.0.root.clone()),
            Action::Paste => terminal.paste = Some(clipboard.fetch_text()),
            Action::Copy => {
                if let Some(session) = terminal.session_mut() {
                    let text = session.text();
                    terminal.status = clipboard
                        .set_text(text)
                        .map_or_else(|e| e.to_string(), |_| "Copied screen.".into());
                }
            }
        }
        if let Some(entity) = view {
            focus.set(entity, FocusCause::Navigated);
            terminal.active = shown;
        }
    }
    if terminal.active {
        if let Ok(mut window) = windows.single_mut() {
            window.ime_enabled = true;
        }
        let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let command = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
        if (command || control && shift) && keys.just_pressed(KeyCode::KeyV) {
            terminal.paste = Some(clipboard.fetch_text());
        }
        if (command || control && shift)
            && keys.just_pressed(KeyCode::KeyC)
            && let Some(session) = terminal.session_mut()
        {
            let text = session.text();
            let _ = clipboard.set_text(text);
        }
        let mut input = Vec::new();
        for event in ime.read() {
            if let bevy::window::Ime::Commit { value, .. } = event {
                input.extend(value.as_bytes());
            }
        }
        let application = terminal
            .session_mut()
            .is_some_and(|s| s.application_cursor());
        for event in events
            .read()
            .filter(|event| event.state == ButtonState::Pressed)
        {
            if event.key_code == KeyCode::F6
                || command
                || control && shift && matches!(event.key_code, KeyCode::KeyC | KeyCode::KeyV)
                || control && event.key_code == KeyCode::Tab
            {
                continue;
            }
            input.extend(key_bytes(
                event,
                control,
                keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
                application,
            ));
        }
        if !input.is_empty()
            && let Some(session) = terminal.session_mut()
        {
            session.bottom();
            if let Err(error) = session.send(input) {
                terminal.status = error;
            }
            terminal.dirty = true;
        }
        if wheel.delta.y != 0.
            && let Some(session) = terminal.session_mut()
        {
            session.scroll(
                (wheel.delta.y
                    * if wheel.unit == bevy::input::mouse::MouseScrollUnit::Pixel {
                        0.1
                    } else {
                        3.
                    }) as i32,
            );
            terminal.dirty = true;
        }
    } else {
        events.clear();
        ime.clear();
    }
    if let Some(result) = terminal.paste.as_mut().and_then(ClipboardRead::poll_result) {
        terminal.paste = None;
        match result {
            Ok(text) => {
                if let Some(session) = terminal.session_mut() {
                    if let Err(error) = session.paste(&text) {
                        terminal.status = error;
                    }
                }
            }
            Err(error) => terminal.status = error.to_string(),
        }
    }
    if shown {
        if let Some((_, bounds, _)) = views.iter().next() {
            let size = bounds.size() * bounds.inverse_scale_factor();
            let desired = Size {
                cols: (size.x / 12.).floor().clamp(20., 240.) as u16,
                rows: (size.y / 24.).floor().clamp(6., 100.) as u16,
            };
            if let Some(session) = terminal.session_mut()
                && (session.size.rows != desired.rows || session.size.cols != desired.cols)
            {
                if let Err(error) = session.resize(desired) {
                    terminal.status = error;
                }
                terminal.dirty = true;
            }
        }
        if terminal.dirty {
            if let Some(session) = terminal.session_mut() {
                let frame = session.frame();
                let image = render(&frame);
                if let Some(handle) = &terminal.image {
                    let _ = images.insert(handle.id(), image);
                } else {
                    terminal.image = Some(images.add(image));
                }
                terminal.dirty = false;
            }
        }
        if let Some(image) = &terminal.image {
            for (_, _, mut node) in &mut views {
                if node.image != *image {
                    node.image = image.clone();
                }
            }
        }
        for mut label in &mut labels {
            label.set_if_neq(Text::new(&terminal.status));
        }
    }
}

fn key_bytes(event: &KeyboardInput, control: bool, alt: bool, application: bool) -> Vec<u8> {
    let cursor = if application { 'O' } else { '[' };
    let special = match event.key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => Some("\r".into()),
        KeyCode::Tab => Some("\t".into()),
        KeyCode::Escape => Some("\x1b".into()),
        KeyCode::Backspace => Some("\x7f".into()),
        KeyCode::ArrowUp => Some(format!("\x1b{cursor}A")),
        KeyCode::ArrowDown => Some(format!("\x1b{cursor}B")),
        KeyCode::ArrowRight => Some(format!("\x1b{cursor}C")),
        KeyCode::ArrowLeft => Some(format!("\x1b{cursor}D")),
        KeyCode::Home => Some("\x1b[H".into()),
        KeyCode::End => Some("\x1b[F".into()),
        KeyCode::Delete => Some("\x1b[3~".into()),
        KeyCode::Insert => Some("\x1b[2~".into()),
        KeyCode::PageUp => Some("\x1b[5~".into()),
        KeyCode::PageDown => Some("\x1b[6~".into()),
        _ => None,
    };
    if let Some(text) = special {
        return text.into_bytes();
    }
    if control {
        if let bevy::input::keyboard::Key::Character(text) = &event.logical_key
            && let Some(ch) = text.chars().next().filter(|c| c.is_ascii())
        {
            return vec![(ch.to_ascii_uppercase() as u8) & 0x1f];
        }
        return Vec::new();
    }
    let Some(text) = event.text.as_deref() else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if alt {
        bytes.push(27);
    }
    bytes.extend(text.as_bytes());
    bytes
}

fn render(frame: &session::Frame) -> Image {
    let width = frame.size.cols as u32 * 12;
    let height = frame.size.rows as u32 * 24;
    let mut canvas = code_texture::Canvas::new(width, height);
    for (index, cell) in frame.cells.iter().enumerate() {
        let x = (index % frame.size.cols as usize) as u32 * 12;
        let y = (index / frame.size.cols as usize) as u32 * 24;
        let cursor = frame.cursor
            == Some((
                index / frame.size.cols as usize,
                index % frame.size.cols as usize,
            ));
        let (fg, bg) = if cursor {
            ([13, 17, 23], [170, 228, 222])
        } else {
            (cell.fg, cell.bg)
        };
        canvas.rect(x, y, 12, 24, bg);
        if !cell.text.trim().is_empty() {
            canvas.text(&cell.text, x as f32, y as f32 + 19., 20., |_| {
                fg.map(|c| c as f32 / 255.)
            });
        }
        if cell.underline {
            canvas.rect(x, y + 22, 12, 1, fg);
        }
    }
    canvas.image()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn exiting_the_shell_starts_a_new_session_without_reopening_the_terminal() {
        let root = std::env::temp_dir();
        let mut session = Session::open(&root, Some("/bin/sh")).unwrap();
        session.send(b"exit\r".to_vec()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !session.exited {
            session.poll();
            assert!(Instant::now() < deadline, "Shell did not exit");
            std::thread::sleep(Duration::from_millis(5));
        }
        let now = Instant::now();
        let mut terminal = Terminal {
            root: Some(root.clone()),
            session: Some(Mutex::new(session)),
            ..default()
        };
        terminal.recovery.started = Some(now - Duration::from_secs(3));
        terminal.maintain(&root, now, true);
        assert!(
            terminal.pending.is_some(),
            "Exiting must launch a new shell"
        );
        assert!(terminal.session.is_none());
    }

    #[test]
    fn five_immediate_exits_pause_until_the_tablet_reopens() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        for attempt in 1..=MAX_IMMEDIATE_EXITS {
            recovery.started = Some(now);
            recovery.ended(now + Duration::from_millis(10));
            assert_eq!(recovery.failures, attempt);
            assert_eq!(recovery.retry_at.is_some(), attempt < MAX_IMMEDIATE_EXITS);
        }
        recovery.reopen(now);
        assert_eq!(recovery.failures, 0);
        assert_eq!(recovery.retry_at, Some(now));
        // A failed spawn has no running timestamp and consumes the same retry budget.
        recovery.ended(now);
        assert_eq!(recovery.failures, 1);
    }

    #[test]
    fn a_normal_shell_lifetime_resets_the_consecutive_failure_budget() {
        let now = Instant::now();
        let mut recovery = Recovery {
            failures: 4,
            started: Some(now),
            retry_at: None,
        };
        recovery.ended(now + IMMEDIATE_EXIT);
        assert_eq!(recovery.failures, 0);
        assert_eq!(recovery.retry_at, Some(now + IMMEDIATE_EXIT));
        recovery.started = Some(now + IMMEDIATE_EXIT);
        recovery.ended(now + IMMEDIATE_EXIT + Duration::from_millis(10));
        assert_eq!(recovery.failures, 1);
    }

    #[test]
    fn reopening_vibepad_retries_a_paused_terminal() {
        let root = std::env::temp_dir();
        let now = Instant::now();
        let mut terminal = Terminal {
            root: Some(root.clone()),
            pad_open: true,
            recovery: Recovery {
                failures: MAX_IMMEDIATE_EXITS,
                ..default()
            },
            ..default()
        };
        terminal.maintain(&root, now, true);
        assert!(terminal.pending.is_none());
        terminal.maintain(&root, now, false);
        terminal.maintain(&root, now, true);
        assert!(terminal.pending.is_some());
        assert_eq!(terminal.recovery.failures, 0);
    }
}
