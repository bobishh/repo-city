//! The control tour advances on the same actions the city already uses.
use super::*;

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
    fn start(&mut self) {
        self.step = Step::Look;
    }
    fn stop(&mut self) {
        self.step = Step::Off;
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
    {
        let (version, play) = browser_tour();
        if tour.browser_version != version {
            tour.browser_version = version;
            if play {
                tour.start();
            } else {
                tour.stop();
            }
        }
    }
    let Ok(pilot) = camera.single() else {
        return;
    };
    let Ok((mut text, mut node)) = hint.single_mut() else {
        return;
    };
    if !review.blocked {
        tour.step = match tour.step {
            Step::Look if pilot.captured => Step::Travel,
            Step::Travel if navigation.is_flying() => Step::Arriving,
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
        Step::Wash => "3 / 3   Aim at a line. Hold left click to wash it with water (0).",
        Step::Finish(_) => "Press U to try colored paints. Use ? to replay the tour.",
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
pub(super) use native::{Guide, spawn, update};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use bevy::ui_widgets::{Activate, observe};

    #[derive(Resource)]
    pub(crate) struct Guide {
        open: bool,
        seen: Option<PathBuf>,
    }

    impl Default for Guide {
        fn default() -> Self {
            let seen =
                repository_picker::settings_file().map(|path| path.with_file_name("guide-v1-seen"));
            let smoke = std::env::args().any(|arg| arg == "--smoke-dir")
                || std::env::current_exe()
                    .ok()
                    .and_then(|path| {
                        path.parent()?
                            .parent()
                            .map(|dir| dir.join("Resources/smoke-output.txt").is_file())
                    })
                    .unwrap_or(false);
            let open = !smoke && !seen.as_ref().is_some_and(|path| path.is_file());
            Self { open, seen }
        }
    }

    impl Guide {
        fn dismiss(&mut self) {
            self.open = false;
            if let Some(path) = &self.seen {
                let result = std::fs::create_dir_all(path.parent().expect("settings directory"))
                    .and_then(|()| std::fs::write(path, b"seen"));
                if let Err(error) = result {
                    warn!("Could not save guide preference: {error}");
                }
            }
        }
    }

    #[derive(Component)]
    pub(crate) struct Panel;

    pub(crate) fn update(
        keys: Res<ButtonInput<KeyCode>>,
        mut guide: ResMut<Guide>,
        mut review: ResMut<review_notes::Review>,
        mut panel: Query<&mut Node, With<Panel>>,
    ) {
        if guide.open && keys.just_pressed(KeyCode::Escape) {
            guide.dismiss();
        }
        review.blocked = guide.open;
        if let Ok(mut node) = panel.single_mut() {
            let display = if guide.open {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
    }

    pub(crate) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(20.),
                    top: px(19.),
                    ..default()
                },
                GlobalZIndex(4),
            ))
            .with_children(|ui| {
                ui.spawn((
                    file_picker::button("?", font, ink, AlignSelf::Auto),
                    observe(|_: On<Activate>, mut guide: ResMut<Guide>| guide.open = true),
                ));
            });
        commands.spawn((
            Panel, GlobalZIndex(30),
            Node {
                position_type: PositionType::Absolute, display: Display::None,
                width: percent(100.), height: percent(100.),
                align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                ..default()
            }, BackgroundColor(Color::srgba(0.01, 0.025, 0.04, 0.75)),
        )).with_children(|ui| {
            ui.spawn((
                Node {
                    width: percent(86.), max_width: px(390.), flex_direction: FlexDirection::Column,
                    row_gap: px(13.), padding: UiRect::all(px(19.)), ..default()
                },
                BackgroundColor(Color::srgb_u8(10, 19, 26)),
            )).with_children(|panel| {
                panel.spawn((Text::new("Take a quick tour"), TextFont { font_size: bevy::text::FontSize::Px(21.), ..font.clone() }, TextColor(ink)));
                panel.spawn((Text::new("Open a Git folder. Then learn to look, fly to a file, and wash a line of code."),
                    font.clone(), TextColor(Color::srgb_u8(218, 234, 233)), bevy::picking::Pickable::IGNORE));
                panel.spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: px(7.), ..default() })
                    .with_children(|buttons| {
                        buttons.spawn((file_picker::button("Skip", font, ink, AlignSelf::Auto),
                            observe(|_: On<Activate>, mut guide: ResMut<Guide>, mut tour: ResMut<Tour>| {
                                tour.stop();
                                guide.dismiss();
                            })));
                        buttons.spawn((file_picker::button("Start tour", font, ink, AlignSelf::Auto),
                            observe(|_: On<Activate>, mut guide: ResMut<Guide>, mut tour: ResMut<Tour>, mut review: ResMut<review_notes::Review>| {
                                review.select(0);
                                tour.start();
                                guide.dismiss();
                            })));
                    });
            });
        });
    }
}
