//! Mouse actions for the file and face under the cursor when the menu opens.
use bevy::{
    picking::hover::Hovered,
    ui_widgets::{Activate, Button, observe},
};

use super::*;

const WIDTH: f32 = 184.;
const ROW_HEIGHT: f32 = 29.;
const GAP: f32 = 3.;
const PADDING: f32 = 6.;

#[derive(Resource, Default)]
pub(super) struct Menu {
    anchor: Option<Vec2>,
    aim: Option<file_facade::Aim>,
    pub(super) destination: Option<file_facade::Aim>,
    dismissed: bool,
}

impl Menu {
    pub(super) fn is_open(&self) -> bool {
        self.anchor.is_some()
    }

    fn close(&mut self) {
        self.anchor = None;
        self.aim = None;
    }
}

pub(super) fn gameplay_enabled(menu: Res<Menu>, review: Res<review_notes::Review>) -> bool {
    !menu.is_open() && !menu.dismissed && !review.blocked
}

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Approach,
    Diff,
    Blame,
    Find,
    Paints,
    Notes,
}

impl Action {
    const ALL: [Self; 6] = [
        Self::Approach,
        Self::Diff,
        Self::Blame,
        Self::Find,
        Self::Paints,
        Self::Notes,
    ];

    fn label(self) -> String {
        match self {
            Self::Approach => "Fly to face    E".into(),
            Self::Diff => "Diff           G".into(),
            Self::Blame => "Blame          B".into(),
            Self::Paints => "Paints         U".into(),
            Self::Notes => "Review notes   J".into(),
            Self::Find => format!("Find file    {}", file_picker::SHORTCUT),
        }
    }

    fn global(self) -> bool {
        matches!(self, Self::Find | Self::Paints | Self::Notes)
    }

    fn choose(
        self,
        menu: &mut Menu,
        picker: &mut file_picker::FilePicker,
        review: &mut review_notes::Review,
    ) {
        if !menu.is_open() || (menu.aim.is_none() && !self.global()) {
            return;
        }
        match self {
            Self::Find => picker.open(),
            Self::Paints => review.request = Some(review_notes::Panel::Paints),
            Self::Notes => review.request = Some(review_notes::Panel::Notes),
            Self::Approach => menu.destination = menu.aim,
            Self::Diff => menu.destination = menu.aim.map(|aim| aim.on_side(listing::Face::Diff)),
            Self::Blame => menu.destination = menu.aim.map(|aim| aim.on_side(listing::Face::Blame)),
        }
        menu.close();
    }
}

#[derive(Component)]
pub(super) struct Overlay;

#[derive(Component)]
pub(super) struct Panel;

pub(super) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands
        .spawn((
            Overlay,
            GlobalZIndex(10),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: percent(100.),
                height: percent(100.),
                ..default()
            },
        ))
        .with_children(|ui| {
            ui.spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100.),
                    height: percent(100.),
                    ..default()
                },
                observe(|_: On<Activate>, mut menu: ResMut<Menu>| menu.close()),
            ));
            ui.spawn((
                Panel,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(GAP),
                    padding: UiRect::all(px(PADDING)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(15, 29, 35)),
            ))
            .with_children(|panel| {
                for action in Action::ALL {
                    panel.spawn((
                        action,
                        file_picker::button(&action.label(), font, ink, AlignSelf::Stretch),
                        observe(move |_: On<Activate>, mut menu: ResMut<Menu>, mut picker: ResMut<file_picker::FilePicker>, mut review: ResMut<review_notes::Review>| {
                            action.choose(&mut menu, &mut picker, &mut review);
                        }),
                    ));
                }
            });
        });
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    nearest: Res<Nearest>,
    mut picker: ResMut<file_picker::FilePicker>,
    navigation: Res<city_navigation::Navigation>,
    listing: Res<listing::Listing>,
    mut menu: ResMut<Menu>,
    mut review: ResMut<review_notes::Review>,
    mut overlay: Query<&mut Node, With<Overlay>>,
    mut panel: Query<&mut Node, (With<Panel>, Without<Overlay>)>,
    mut rows: Query<
        (&Action, &Hovered, &mut Node, &mut BackgroundColor),
        (Without<Overlay>, Without<Panel>),
    >,
) {
    menu.dismissed = false;
    let right_click = mouse.just_pressed(MouseButton::Right);
    if review.blocked || picker.is_open() || navigation.guides_camera() || listing.turning() {
        menu.close();
    } else if menu.is_open() && (keys.just_pressed(KeyCode::Escape) || right_click) {
        menu.close();
        menu.dismissed = true;
    } else if menu.is_open() {
        for (key, action) in [
            (KeyCode::KeyE, Action::Approach),
            (KeyCode::KeyG, Action::Diff),
            (KeyCode::KeyB, Action::Blame),
            (KeyCode::KeyU, Action::Paints),
            (KeyCode::KeyJ, Action::Notes),
        ] {
            if keys.just_pressed(key) {
                action.choose(&mut menu, &mut picker, &mut review);
                break;
            }
        }
    } else if right_click && let Some((point, _)) = nearest.screen {
        menu.anchor = Some(point);
        menu.aim = nearest.file;
    }
    let (Ok(mut overlay), Ok(mut panel)) = (overlay.single_mut(), panel.single_mut()) else {
        return;
    };
    let display = if menu.is_open() {
        Display::Flex
    } else {
        Display::None
    };
    if overlay.display != display {
        overlay.display = display;
    }
    let (Some(anchor), Some((_, viewport))) = (menu.anchor, nearest.screen) else {
        return;
    };
    let count = if menu.aim.is_some() {
        Action::ALL.len()
    } else {
        Action::ALL.iter().filter(|action| action.global()).count()
    } as f32;
    let height = ROW_HEIGHT * count + GAP * (count - 1.) + 2. * PADDING;
    let point = (anchor + Vec2::splat(12.)).clamp(
        Vec2::ZERO,
        (viewport - Vec2::new(WIDTH, height)).max(Vec2::ZERO),
    );
    if panel.left != px(point.x) || panel.top != px(point.y) {
        panel.left = px(point.x);
        panel.top = px(point.y);
    }
    for (action, hovered, mut row, mut background) in &mut rows {
        let display = if menu.aim.is_some() || action.global() {
            Display::Flex
        } else {
            Display::None
        };
        if row.display != display {
            row.display = display;
        }
        background.set_if_neq(BackgroundColor(if hovered.get() {
            Color::srgb_u8(33, 65, 73)
        } else {
            Color::srgb_u8(20, 40, 46)
        }));
    }
}
