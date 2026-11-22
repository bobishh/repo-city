//! The control tour advances on the same actions the city already uses.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::picking::Pickable;

#[derive(Resource, Default)]
pub(super) struct Tour {
    step: Step,
    browser_version: u32,
}
#[derive(Clone, Copy, Default, PartialEq)]
enum Step {
    #[default]
    Off,
    Look,
    Travel,
    Arriving,
    Wash,
    Finish(f32),
}
impl Tour {
    pub fn active(&self) -> bool {
        self.step != Step::Off
    }
    pub(super) fn start(&mut self) {
        self.step = Step::Look;
    }
    fn stop(&mut self) {
        self.step = Step::Off;
    }

    #[cfg(target_arch = "wasm32")]
    fn sync_browser(&mut self) {
        let (version, play) = browser_tour();
        if self.browser_version != version {
            self.browser_version = version;
            if play {
                self.start();
            } else {
                self.stop();
            }
        }
    }

    pub(super) fn library_hint(
        &mut self,
        now: f32,
        aiming: bool,
        reading: bool,
        ready: bool,
        heating: bool,
    ) -> &'static str {
        #[cfg(target_arch = "wasm32")]
        self.sync_browser();
        self.step = match self.step {
            Step::Look if aiming || reading => Step::Travel,
            Step::Travel if reading => Step::Arriving,
            Step::Arriving if !reading => Step::Travel,
            Step::Arriving if ready => Step::Wash,
            Step::Wash if heating => Step::Finish(now),
            Step::Finish(started) if now > started + 9. => Step::Off,
            step => step,
        };
        match self.step {
            Step::Off => "",
            Step::Look => "Look toward a shelf. Walk with WASD or the move stick.",
            Step::Travel => "Aim at a scroll and press E, or use M to find it on the map.",
            Step::Arriving => "Unrolling the file…",
            Step::Wash => "Look at the lines and press K to turn on heat.",
            Step::Finish(_) => "Tab opens Stone Pad. M cycles the cabinet and library maps.",
        }
    }
}

#[derive(Component)]
pub(super) struct Hint;

pub(super) fn spawn_hint(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands.spawn((
        Hint,
        Text::new(""),
        font.clone(),
        TextColor(ink),
        BackgroundColor(Color::srgba(0.024, 0.066, 0.08, 0.93)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            bottom: px(22.),
            left: px(24.),
            max_width: percent(90.),
            padding: UiRect::axes(px(12.), px(8.)),
            ..default()
        },
        bevy::picking::Pickable::IGNORE,
    ));
}

pub(super) fn advance(
    mut tour: ResMut<Tour>,
    time: Res<Time>,
    camera: Query<&Pilot, PlayerCamera>,
    navigation: Res<city_navigation::Navigation>,
    review: Res<review_notes::Review>,
    mut hint: Query<(&mut Text, &mut Node), With<Hint>>,
) {
    #[cfg(target_arch = "wasm32")]
    tour.sync_browser();
    let Ok(pilot) = camera.single() else {
        return;
    };
    let Ok((mut text, mut node)) = hint.single_mut() else {
        return;
    };
    if !review.blocked {
        tour.step = match tour.step {
            Step::Look if pilot.captured => Step::Travel,
            Step::Travel if navigation.is_flying() && !navigation.overview() => Step::Arriving,
            Step::Arriving if !navigation.is_flying() => Step::Wash,
            Step::Wash if review.washing && review.nozzle > 0.85 => {
                Step::Finish(time.elapsed_secs())
            }
            Step::Finish(started) if time.elapsed_secs() > started + 9. => Step::Off,
            step => step,
        };
    }
    let display = if tour.active() && !review.blocked {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    let message = match tour.step {
        Step::Off => "",
        Step::Look => "1 / 3   Click the scene, then move the mouse to look around.",
        Step::Travel if !pilot.captured => "2 / 3   Click to capture the cursor again.",
        Step::Travel => "2 / 3   Aim at a building and press E to fly there.",
        Step::Arriving => "2 / 3   Flying to the wall…",
        Step::Wash if !pilot.captured => "3 / 3   Click to capture the cursor again.",
        Step::Wash => "3 / 3   Aim at a line. Click to turn heat on or off (0).",
        Step::Finish(_) => {
            if cfg!(target_arch = "wasm32") {
                "U opens paints. Sign in for PR notes. Publishing needs Pull requests: write."
            } else {
                "U opens paints. Your review notes stay on this Mac."
            }
        }
    };
    if text.0 != message {
        text.0 = message.into();
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_tour() -> (u32, bool) {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tourCommand)]
        fn command() -> u32;
    }
    let value = command();
    (value >> 1, value & 1 != 0)
}

#[cfg(not(target_arch = "wasm32"))]
const NAVIGATION: &[(&str, &str)] = &[
    ("W A S D", "Fly"),
    ("Arrows", "Look around; Shift+Arrows near a wall"),
    ("Space / Ctrl", "Change altitude"),
    ("O", "Land and walk; O or Space to fly again"),
    ("Shift", "Fly faster"),
    ("E", "Fly to aimed wall"),
    ("M", "Cycle district, project, and return"),
    ("Cmd+O", "Open local repository"),
    ("Alt+0", "Open repository menu"),
    ("Shift+F10", "Open actions menu"),
    ("Cmd+P", "Find a file"),
    ("P", "Preview aimed text or image"),
    ("Tab", "Open or put away VibePad"),
    ("Alt+1–8", "Select a VibePad tab"),
    ("Arrows / Enter", "Move through controls and activate"),
    ("Ctrl+O / Ctrl+I", "Previous or next building"),
    ("L", "Fly to the last changed file"),
];

#[cfg(not(target_arch = "wasm32"))]
const READING: &[(&str, &str)] = &[
    ("F4", "Toggle reading mode; timers pause while off"),
    ("Scroll / PgUp / PgDn", "Read along the wall"),
    ("Home", "Frame the file"),
    ("/", "Find text on the wall"),
    ("N / Shift+N", "Next or previous match"),
    ("G / B", "Open diff or blame"),
    ("Left / Right", "Cycle source, diff, and blame up close"),
    ("Z / X", "Collapse or expand the aimed code block"),
    ("F", "Show only changed files"),
];

#[cfg(not(target_arch = "wasm32"))]
const REVIEW: &[(&str, &str)] = &[
    ("0", "Heat tool"),
    ("K", "Use heat or hold to paint"),
    ("1–9", "Select a paint"),
    ("Cmd+scroll", "Cycle tools"),
    ("U", "Edit paints and templates"),
    ("J", "Open review comments"),
    ("C", "Review changed roofs, smallest buildings first"),
    ("W / ↑ / scroll up", "Next changed building during review"),
    (
        "S / ↓ / scroll down",
        "Previous changed building during review",
    ),
    (
        "[ / ] or hunk buttons",
        "Choose a hunk on the current roof; Previous/Next shows more buttons",
    ),
    ("Q", "Leave the roof tour and keep the current diff"),
    ("V", "Toggle reviewed mark"),
];

#[cfg(not(target_arch = "wasm32"))]
const WORKBENCH: &[(&str, &str)] = &[
    ("F3", "Open or close the cooperative room"),
    ("F10", "Mute or restore all application sound"),
    ("F5", "Start or stop agent voice with the tablet put away"),
    (
        "Tab → Agent",
        "Open the conversation; closing the tablet keeps it active",
    ),
    (
        "Enter / Shift+Enter",
        "Send or steer agent work / insert a new line (configurable in Settings)",
    ),
    ("F7", "Toggle Follow agent (F8 is available in Settings)"),
    (
        "Y / N",
        "Approve or reject an agent request, or accept or dismiss a destination",
    ),
    ("Tab → Terminal", "Open the persistent shell"),
    (
        "F6",
        "Switch between shell/editor input and tablet controls",
    ),
    (
        "Ctrl+Tab",
        "Put away the tablet while the terminal has focus",
    ),
    ("Tab / Esc / Ctrl+C", "Pass through to the focused shell"),
];

#[cfg(not(target_arch = "wasm32"))]
const BODY: Color = Color::srgb_u8(216, 234, 235);
#[cfg(not(target_arch = "wasm32"))]
const KEY_BACKGROUND: Color = Color::srgb_u8(22, 48, 54);

#[cfg(not(target_arch = "wasm32"))]
fn section(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    rows: &[(&str, &str)],
    font: &TextFont,
    ink: Color,
) {
    parent.spawn((
        Text::new(title),
        TextFont {
            font_size: FontSize::Px(15.),
            ..font.clone()
        },
        TextColor(ink),
        Pickable::IGNORE,
    ));
    for &(shortcut, description) in rows {
        parent
            .spawn(Node {
                align_items: AlignItems::Baseline,
                column_gap: px(8.),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(shortcut),
                    font.clone(),
                    TextColor(ink),
                    BackgroundColor(KEY_BACKGROUND),
                    Node {
                        flex_shrink: 0.,
                        padding: UiRect::horizontal(px(4.)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                row.spawn((
                    Text::new(description),
                    font.clone(),
                    TextColor(BODY),
                    Pickable::IGNORE,
                ));
            });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn draw_controls(parent: &mut ChildSpawnerCommands, library: bool) {
    const LIBRARY_NAVIGATION: &[(&str, &str)] = &[
        ("W A S D", "Walk in the gallery"),
        ("Space / O", "Take off or land in front of a reading room"),
        ("Mouse / arrows", "Look around the gallery"),
        (
            "Right-click / Shift+F10",
            "Open actions for the current target",
        ),
        ("E", "Open the aimed scroll or select the aimed cabinet"),
        ("M", "Open the cabinet map, the corridor map, or return"),
        ("Tab", "Open or put away Stone Pad"),
        ("Cmd+O / Cmd+P", "Open a repository or find a file"),
        ("Ctrl+O / Ctrl+I", "Previous or next file"),
        ("Arrows / Enter", "Move through controls and activate"),
    ];
    const LIBRARY_READING: &[(&str, &str)] = &[
        ("Scroll / PgUp / PgDn", "Move along the scroll"),
        ("J / /", "Jump to a line or find text"),
        ("Enter / Shift+Enter", "Next or previous match"),
        ("G / B / Q", "Open diff, blame, or switch snapshot side"),
        ("Z / X", "Fold or unfold the aimed block"),
        ("F4", "Toggle reading mode"),
    ];
    const LIBRARY_REVIEW: &[(&str, &str)] = &[
        ("0 / 1–9", "Select heat or a paint"),
        ("K", "Toggle heat or hold to paint"),
        ("U / Shift+J", "Edit paints or open comments"),
        ("F", "Show only changed files"),
        (
            "C / W / S",
            "Start changed-file review or move between files",
        ),
        ("[ / ]", "Move between hunks"),
    ];
    let font = TextFont::from_font_size(14.);
    let ink = native_panel::INK;
    let column = Node {
        flex_direction: FlexDirection::Column,
        flex_basis: px(310.),
        flex_grow: 1.,
        row_gap: px(5.),
        ..default()
    };
    parent
        .spawn(Node {
            column_gap: px(22.),
            row_gap: px(18.),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|columns| {
            columns.spawn(column.clone()).with_children(|left| {
                section(
                    left,
                    "Navigation",
                    if library {
                        LIBRARY_NAVIGATION
                    } else {
                        NAVIGATION
                    },
                    &font,
                    ink,
                );
                left.spawn(Node {
                    height: px(10.),
                    ..default()
                });
                section(
                    left,
                    "Reading",
                    if library { LIBRARY_READING } else { READING },
                    &font,
                    ink,
                );
            });
            columns.spawn(column).with_children(|right| {
                section(
                    right,
                    "Review",
                    if library { LIBRARY_REVIEW } else { REVIEW },
                    &font,
                    ink,
                );
                right.spawn(Node {
                    height: px(10.),
                    ..default()
                });
                section(right, "Agent and terminal", WORKBENCH, &font, ink);
            });
        });
}
