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
            Step::Travel if navigation.is_flying() && !navigation.overview() => Step::Arriving,
            Step::Arriving if !navigation.is_flying() => Step::Wash,
            Step::Wash if review.washing && review.nozzle > 0.85 => {
                #[cfg(target_arch = "wasm32")]
                browser_tour_completed();
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

#[cfg(target_arch = "wasm32")]
fn browser_tour_completed() {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tourCompleted)]
        fn completed();
    }
    completed();
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::{Guide, spawn, update};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use bevy::{
        input::mouse::MouseScrollUnit,
        input_focus::{
            FocusCause, InputFocus, InputFocusVisible,
            tab_navigation::{TabGroup, TabIndex},
        },
        picking::Pickable,
        text::EditableText,
        ui_widgets::{Activate, observe},
    };

    #[derive(Clone, Copy)]
    enum Action {
        Toggle,
        Close,
        StartTour,
    }

    #[derive(Resource, Default)]
    pub(crate) struct Guide {
        open: bool,
        action: Option<Action>,
    }

    impl Guide {
        pub(crate) fn is_open(&self) -> bool {
            self.open
        }

        pub(crate) fn open(&mut self) {
            if !self.open {
                self.action = Some(Action::Toggle);
            }
        }
    }

    #[derive(Component)]
    pub(crate) struct Panel;

    #[derive(Component)]
    pub(crate) struct Controls;

    #[derive(Component)]
    pub(crate) struct HelpAction {
        initial_focus: bool,
    }

    pub(crate) fn update(
        mut keys: ResMut<ButtonInput<KeyCode>>,
        mut guide: ResMut<Guide>,
        mut tour: ResMut<Tour>,
        mut review: ResMut<review_notes::Review>,
        file_picker: Res<file_picker::FilePicker>,
        repository: Res<repository_picker::RepositoryPicker>,
        search: Res<listing::find::Search>,
        mut focus: ResMut<InputFocus>,
        focus_visible: Res<InputFocusVisible>,
        editable: Query<(), With<EditableText>>,
        mut panel: Query<&mut Node, With<Panel>>,
        mut actions: Query<(Entity, &HelpAction, &mut Outline)>,
        mut controls: Query<(&mut ScrollPosition, &ComputedNode), With<Controls>>,
        scroll: Res<AccumulatedMouseScroll>,
        mut pilot: Query<&mut Pilot, PlayerCamera>,
        mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    ) {
        let was_open = guide.open;
        let action = guide.action.take().or_else(|| {
            if was_open && keys.just_pressed(KeyCode::Escape) {
                keys.clear_just_pressed(KeyCode::Escape);
                Some(Action::Close)
            } else if keys.just_pressed(KeyCode::KeyH)
                && !keys.any_pressed([
                    KeyCode::SuperLeft,
                    KeyCode::SuperRight,
                    KeyCode::ControlLeft,
                    KeyCode::ControlRight,
                    KeyCode::AltLeft,
                    KeyCode::AltRight,
                ])
            {
                Some(Action::Toggle)
            } else {
                None
            }
        });
        let can_open = !review.blocked
            && !file_picker.is_open()
            && !repository.blocks_controls()
            && !search.blocks_controls()
            && !focus.get().is_some_and(|entity| editable.contains(entity));
        match action {
            Some(Action::Toggle) if was_open => guide.open = false,
            Some(Action::Toggle) if can_open => guide.open = true,
            Some(Action::Close) => guide.open = false,
            Some(Action::StartTour) if was_open => {
                review.select(0);
                tour.start();
                guide.open = false;
            }
            _ => {}
        }

        if guide.open && !was_open {
            focus.clear();
            if let Some((entity, _, _)) = actions.iter().find(|(_, action, _)| action.initial_focus)
            {
                focus.set(entity, FocusCause::Navigated);
            }
            review.drag = None;
            if let Ok(mut pilot) = pilot.single_mut() {
                pilot.captured = false;
                pilot.fire_ready = false;
                pilot.firing = false;
                pilot.shot = false;
            }
            if let Ok(mut cursor) = cursor.single_mut() {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
        } else if was_open && !guide.open {
            focus.clear();
            keys.clear_just_pressed(KeyCode::Enter);
            keys.clear_just_pressed(KeyCode::NumpadEnter);
            keys.clear_just_pressed(KeyCode::Space);
        }
        review.blocked |= guide.open;
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
        if guide.open {
            for (entity, _, mut outline) in &mut actions {
                let width = if focus_visible.0 && focus.get() == Some(entity) {
                    px(1.)
                } else {
                    px(0.)
                };
                if outline.width != width {
                    outline.width = width;
                }
            }
            if let Ok((mut position, node)) = controls.single_mut() {
                let delta = match scroll.unit {
                    MouseScrollUnit::Line => scroll.delta.y * 30.,
                    MouseScrollUnit::Pixel => scroll.delta.y,
                };
                let max =
                    (node.content_size().y - node.size().y).max(0.) * node.inverse_scale_factor();
                let y = (position.y - delta).clamp(0., max);
                if position.y != y {
                    position.y = y;
                }
            }
        }
    }

    const NAVIGATION: &[(&str, &str)] = &[
        ("W A S D", "Fly"),
        ("Space / Ctrl", "Change altitude"),
        ("Shift", "Fly faster"),
        ("E", "Fly to aimed wall"),
        ("M", "Cycle district, project, and return"),
        ("Cmd+O", "Open local repository"),
        ("Cmd+P", "Find a file"),
        ("Tab", "Open the changed files tablet"),
        ("Ctrl+O / Ctrl+I", "Previous or next building"),
        ("L", "Fly to the last changed file"),
    ];

    const READING: &[(&str, &str)] = &[
        ("Scroll / PgUp / PgDn", "Read along the wall"),
        ("Home", "Frame the file"),
        ("/", "Find text on the wall"),
        ("N / Shift+N", "Next or previous match"),
        ("G / B", "Open diff or blame"),
        ("F", "Show only changed files"),
    ];

    const REVIEW: &[(&str, &str)] = &[
        ("0", "Water tool"),
        ("1–9", "Select a paint"),
        ("Cmd+scroll", "Cycle tools"),
        ("U", "Edit paints and templates"),
        ("J", "Open all review drafts"),
        ("C", "Follow the changed blocks"),
        ("W / S or scroll", "Next or previous review stop"),
        ("Q", "Return to the diff wall"),
        ("V", "Toggle reviewed mark"),
    ];

    const BODY: Color = Color::srgb_u8(216, 234, 235);
    const KEY_BACKGROUND: Color = Color::srgb_u8(22, 48, 54);

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

    pub(crate) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(24.),
                    bottom: px(22.),
                    ..default()
                },
                GlobalZIndex(35),
            ))
            .with_children(|ui| {
                ui.spawn((
                    file_picker::button("H", font, ink, AlignSelf::Auto),
                    observe(|_: On<Activate>, mut guide: ResMut<Guide>| {
                        guide.action = Some(Action::Toggle);
                    }),
                ));
            });

        commands
            .spawn((
                Panel,
                GlobalZIndex(30),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    width: percent(100.),
                    height: percent(100.),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.01, 0.025, 0.04, 0.62)),
            ))
            .with_children(|ui| {
                ui.spawn((
                    TabGroup::modal(),
                    Node {
                        width: px(740.),
                        max_width: percent(91.),
                        max_height: percent(88.),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(14.),
                        padding: UiRect::all(px(22.)),
                        border: UiRect::all(px(1.)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(45, 83, 90)),
                    BackgroundColor(Color::srgb_u8(10, 22, 30)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Controls"),
                        TextFont { font_size: FontSize::Px(21.), ..font.clone() },
                        TextColor(ink),
                        Pickable::IGNORE,
                    ));
                    panel
                        .spawn((
                            Controls,
                            ScrollPosition::default(),
                            Node {
                                min_height: px(0.),
                                column_gap: px(22.),
                                row_gap: px(22.),
                                flex_wrap: FlexWrap::Wrap,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|columns| {
                            let column = Node {
                                flex_direction: FlexDirection::Column,
                                flex_grow: 1.,
                                flex_basis: px(295.),
                                row_gap: px(5.),
                                ..default()
                            };
                            columns.spawn(column.clone()).with_children(|left| {
                                section(left, "Navigation", NAVIGATION, font, ink);
                                left.spawn(Node { height: px(10.), ..default() });
                                section(left, "Reading", READING, font, ink);
                            });
                            columns.spawn(column).with_children(|right| {
                                section(right, "Review", REVIEW, font, ink);
                                right.spawn((
                                    Text::new("Review notes shows a preview of each draft. Use Edit to change one. Publish all sends the drafts for the current pull request to GitHub."),
                                    font.clone(),
                                    TextColor(BODY),
                                    Node { margin: UiRect::top(px(9.)), ..default() },
                                    Pickable::IGNORE,
                                ));
                            });
                        });
                    panel
                        .spawn(Node {
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8.),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (index, (label, action)) in [
                                ("Start tour", Action::StartTour),
                                ("Close", Action::Close),
                            ].into_iter().enumerate() {
                                buttons.spawn((
                                    file_picker::button(label, font, ink, AlignSelf::Auto),
                                    TabIndex(index as i32),
                                    Outline::new(px(0.), px(2.), ink),
                                    HelpAction { initial_focus: index == 1 },
                                    observe(move |_: On<Activate>, mut guide: ResMut<Guide>| {
                                        guide.action = Some(action);
                                    }),
                                ));
                            }
                        });
                });
            });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn help_preserves_text_input_and_releases_camera_controls() {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<AccumulatedMouseScroll>()
                .init_resource::<Guide>()
                .init_resource::<Tour>()
                .init_resource::<review_notes::Review>()
                .init_resource::<file_picker::FilePicker>()
                .init_resource::<listing::find::Search>()
                .init_resource::<InputFocus>()
                .init_resource::<InputFocusVisible>()
                .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
                .add_systems(Update, update);

            let world = app.world_mut();
            let editor = world.spawn(EditableText::default()).id();
            let camera = world
                .spawn((
                    Camera3d::default(),
                    Pilot {
                        captured: true,
                        ..default()
                    },
                ))
                .id();
            let window = world
                .spawn((
                    PrimaryWindow,
                    CursorOptions {
                        visible: false,
                        grab_mode: CursorGrabMode::Locked,
                        ..default()
                    },
                ))
                .id();
            world
                .resource_mut::<InputFocus>()
                .set(editor, FocusCause::Navigated);
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyH);
            app.update();
            assert!(!app.world().resource::<Guide>().is_open());
            assert_eq!(app.world().resource::<InputFocus>().get(), Some(editor));

            let world = app.world_mut();
            world.resource_mut::<InputFocus>().clear();
            let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
            keys.reset(KeyCode::KeyH);
            keys.press(KeyCode::KeyH);
            app.update();
            assert!(app.world().resource::<Guide>().is_open());
            assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
            let cursor = app.world().get::<CursorOptions>(window).unwrap();
            assert_eq!(cursor.grab_mode, CursorGrabMode::None);
            assert!(cursor.visible);

            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset(KeyCode::KeyH);
            keys.press(KeyCode::Escape);
            app.update();
            assert!(!app.world().resource::<Guide>().is_open());
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::Escape)
            );
        }
    }
}
