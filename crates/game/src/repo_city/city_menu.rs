//! Mouse actions for the file and face under the cursor when the menu opens.
use bevy::{ecs::system::SystemParam, input::mouse::MouseScrollUnit, ui::RelativeCursorPosition};

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
    selected: usize,
    scroll: f32,
}

impl Menu {
    pub(super) fn is_open(&self) -> bool {
        self.anchor.is_some()
    }

    pub(super) fn blocks_controls(&self) -> bool {
        self.is_open() || self.dismissed
    }

    pub(super) fn selected_action(&self, available: &[Action]) -> Option<Action> {
        available.get(self.selected).copied()
    }

    pub(super) fn begin_frame(&mut self) {
        self.dismissed = false;
    }

    pub(super) fn open(&mut self, point: Vec2) {
        self.anchor = Some(point);
    }

    pub(super) fn close(&mut self) {
        self.dismissed = self.is_open();
        self.anchor = None;
        self.aim = None;
        self.selected = 0;
        self.scroll = 0.;
    }

    pub(super) fn select(
        &mut self,
        wheel: &AccumulatedMouseScroll,
        keys: &ButtonInput<KeyCode>,
        count: usize,
    ) {
        let delta = match wheel.unit {
            MouseScrollUnit::Pixel => wheel.delta.y / 40.,
            MouseScrollUnit::Line => wheel.delta.y,
        };
        if delta != 0. && self.scroll.signum() != delta.signum() {
            self.scroll = 0.;
        }
        self.scroll += delta;
        let mut step = isize::from(keys.just_pressed(KeyCode::ArrowDown))
            - isize::from(keys.just_pressed(KeyCode::ArrowUp));
        if self.scroll.abs() >= 1. {
            step -= self.scroll.signum() as isize;
            self.scroll = 0.;
        }
        self.selected =
            (self.selected.min(count - 1) as isize + step).rem_euclid(count as isize) as usize;
    }
}

pub(super) fn gameplay_enabled(menu: Res<Menu>, review: Res<review_notes::Review>) -> bool {
    !menu.is_open() && !menu.dismissed && !review.blocked
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeChangesOnlyRequest)]
    fn browser_filter_requested() -> bool;
}

pub(super) fn apply_filter_shortcut(
    mut focus: ResMut<file_facade::ChangesOnly>,
    mut menu: ResMut<Menu>,
    picker: Res<file_picker::FilePicker>,
    search: Res<listing::find::Search>,
    review: Res<review_notes::Review>,
    #[cfg(not(target_arch = "wasm32"))] keys: Res<ButtonInput<KeyCode>>,
    #[cfg(not(target_arch = "wasm32"))] repository: Option<
        Res<repository_picker::RepositoryPicker>,
    >,
) {
    #[cfg(target_arch = "wasm32")]
    let requested = browser_filter_requested();
    #[cfg(not(target_arch = "wasm32"))]
    let requested = keys.just_pressed(KeyCode::KeyF)
        && !repository.is_some_and(|picker| picker.blocks_controls());
    if requested
        && focus.available
        && !picker.is_open()
        && !search.blocks_controls()
        && !review.blocked
    {
        focus.toggle();
        if menu.is_open() {
            menu.close();
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Approach,
    Diff,
    Blame,
    Find,
    Paints,
    Notes,
    FindText,
    ChangesOnly,
    Folder,
    Preview,
}

impl Action {
    const ALL: [Self; 8] = [
        Self::Approach,
        Self::Diff,
        Self::Blame,
        Self::Find,
        Self::Paints,
        Self::Notes,
        Self::FindText,
        Self::ChangesOnly,
    ];

    fn label(self, filtered: bool, stone: bool) -> String {
        match self {
            Self::Folder => "Folder map     M".into(),
            Self::Preview => "Preview        P".into(),
            Self::Approach if stone => "Open scroll    E".into(),
            Self::Approach => "Fly to face    E".into(),
            Self::Diff => "Diff           G".into(),
            Self::Blame => "Blame          B".into(),
            Self::Paints => "Paints         U".into(),
            Self::Notes => "Review notes   J".into(),
            Self::FindText if stone => "Find on scroll /".into(),
            Self::FindText => "Find in file   /".into(),
            Self::Find if stone => "Find file      /".into(),
            Self::Find => format!("Find file    {}", file_picker::SHORTCUT),
            Self::ChangesOnly if filtered => "Show all files F".into(),
            Self::ChangesOnly => "Only changes   F".into(),
        }
    }

    fn global(self) -> bool {
        matches!(self, Self::Find | Self::Paints | Self::Notes)
    }

    fn available(
        self,
        aimed: bool,
        search: &listing::find::Search,
        can_paint: bool,
        can_filter: bool,
    ) -> bool {
        if matches!(self, Self::ChangesOnly) {
            return can_filter;
        }
        if matches!(self, Self::Paints) && !can_paint {
            return false;
        }
        if matches!(self, Self::FindText) {
            search.available
        } else {
            self.global() || aimed
        }
    }

    fn choose(
        self,
        menu: &mut Menu,
        picker: &mut file_picker::FilePicker,
        review: &mut review_notes::Review,
        search: &mut listing::find::Search,
        focus: &mut file_facade::ChangesOnly,
    ) {
        if !menu.is_open()
            || !self.available(
                menu.aim.is_some(),
                search,
                review.can_paint,
                focus.available,
            )
        {
            return;
        }
        match self {
            Self::Find => picker.open(),
            Self::FindText => search.request = true,
            Self::Paints => review.request = Some(review_notes::Panel::Paints),
            Self::Notes => review.request = Some(review_notes::Panel::Notes),
            Self::Approach => menu.destination = menu.aim,
            Self::Diff => menu.destination = menu.aim.map(|aim| aim.on_side(listing::Face::Diff)),
            Self::Blame => menu.destination = menu.aim.map(|aim| aim.on_side(listing::Face::Blame)),
            Self::ChangesOnly => focus.toggle(),
            Self::Folder | Self::Preview => return,
        }
        menu.close();
    }
}

#[derive(Component)]
pub(super) struct Overlay;

#[derive(Component)]
pub(super) struct Panel(bool);

#[derive(SystemParam)]
pub(super) struct MenuView<'w, 's> {
    overlay: Query<'w, 's, &'static mut Node, With<Overlay>>,
    panel: Query<'w, 's, (&'static mut Node, &'static Panel), Without<Overlay>>,
    rows: Query<
        'w,
        's,
        (
            &'static Action,
            &'static Children,
            &'static mut Node,
            &'static mut BackgroundColor,
            &'static RelativeCursorPosition,
        ),
        (Without<Overlay>, Without<Panel>),
    >,
    labels: Query<'w, 's, &'static mut Text>,
    #[cfg(target_arch = "wasm32")]
    browser_state: Local<'s, (Option<Action>, bool)>,
}

impl MenuView<'_, '_> {
    pub(super) fn hovered(&self) -> Option<Action> {
        self.rows.iter().find_map(|(action, _, node, _, cursor)| {
            (node.display != Display::None && cursor.cursor_over()).then_some(*action)
        })
    }

    pub(super) fn draw(
        &mut self,
        menu: &Menu,
        available: &[Action],
        viewport: Vec2,
        filtered: bool,
    ) {
        let (Ok(mut overlay), Ok((mut panel, style))) =
            (self.overlay.single_mut(), self.panel.single_mut())
        else {
            return;
        };
        let stone = style.0;
        let display = if menu.is_open() {
            Display::Flex
        } else {
            Display::None
        };
        if overlay.display != display {
            overlay.display = display;
        }
        let selected = menu
            .is_open()
            .then(|| available.get(menu.selected).copied())
            .flatten();
        #[cfg(target_arch = "wasm32")]
        if *self.browser_state != (selected, filtered)
            && let Some(canvas) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("repo-city"))
        {
            let _ = canvas.toggle_attribute_with_force("data-changes-only", filtered);
            if let Some(action) = selected {
                let _ = canvas.set_attribute("data-action-menu", &action.label(filtered, stone));
            } else {
                let _ = canvas.remove_attribute("data-action-menu");
            }
            *self.browser_state = (selected, filtered);
        }
        let Some(anchor) = menu.anchor else {
            return;
        };
        let count = (available.len() + 1) as f32;
        let height = ROW_HEIGHT * count + GAP * (count - 1.) + 2. * PADDING;
        let point = (anchor + Vec2::splat(12.)).clamp(
            Vec2::ZERO,
            (viewport - Vec2::new(WIDTH, height)).max(Vec2::ZERO),
        );
        if panel.left != px(point.x) || panel.top != px(point.y) {
            panel.left = px(point.x);
            panel.top = px(point.y);
        }
        for (action, children, mut row, mut background, _) in &mut self.rows {
            let display = if available.contains(action) {
                Display::Flex
            } else {
                Display::None
            };
            if row.display != display {
                row.display = display;
            }
            if *action == Action::ChangesOnly
                && let Ok(mut label) = self.labels.get_mut(children[0])
            {
                let title = action.label(filtered, stone);
                if label.0 != title {
                    label.0 = title;
                }
            }
            let tint = match (stone, selected == Some(*action)) {
                (false, true) => Color::srgb_u8(42, 83, 91),
                (false, false) => Color::srgb_u8(20, 40, 46),
                (true, true) => Color::srgb_u8(105, 72, 46),
                (true, false) => Color::srgb_u8(55, 40, 29),
            };
            background.set_if_neq(BackgroundColor(tint));
        }
    }
}

pub(super) fn keyboard_menu(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::ContextMenu)
        || (keys.just_pressed(KeyCode::F10)
            && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]))
}

pub(super) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    spawn_menu(commands, font, ink, false);
}

pub(super) fn spawn_stone(commands: &mut Commands, font: &TextFont) {
    spawn_menu(commands, font, Color::srgb_u8(249, 225, 188), true);
}

fn spawn_menu(commands: &mut Commands, font: &TextFont, ink: Color, stone: bool) {
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
                Panel(stone),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(WIDTH),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(GAP),
                    padding: UiRect::all(px(PADDING)),
                    ..default()
                },
                BackgroundColor(if stone {
                    Color::srgb_u8(39, 29, 22)
                } else {
                    Color::srgb_u8(15, 29, 35)
                }),
            ))
            .with_children(|panel| {
                for action in Action::ALL.into_iter().chain(
                    [Action::Folder, Action::Preview]
                        .into_iter()
                        .filter(|_| stone),
                ) {
                    panel.spawn((
                        action,
                        RelativeCursorPosition::default(),
                        Node {
                            min_height: px(ROW_HEIGHT),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(10.)),
                            ..default()
                        },
                        BackgroundColor::default(),
                        children![(
                            Text::new(action.label(false, stone)),
                            font.clone(),
                            TextColor(ink)
                        )],
                    ));
                }
                panel.spawn((
                    Text::new(if stone {
                        "↑/↓ · Enter · Esc"
                    } else {
                        "Scroll · click"
                    }),
                    font.clone(),
                    TextColor(if stone {
                        ink
                    } else {
                        Color::srgb_u8(153, 188, 192)
                    }),
                    Node {
                        min_height: px(ROW_HEIGHT),
                        padding: UiRect::axes(px(10.), px(5.)),
                        ..default()
                    },
                ));
            });
        });
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    nearest: Res<Nearest>,
    mut picker: ResMut<file_picker::FilePicker>,
    navigation: Res<city_navigation::Navigation>,
    listing: Res<listing::Listing>,
    mut menu: ResMut<Menu>,
    mut review: ResMut<review_notes::Review>,
    mut search: ResMut<listing::find::Search>,
    mut focus: ResMut<file_facade::ChangesOnly>,
    mut view: MenuView,
) {
    menu.begin_frame();
    let right_click = mouse.just_pressed(MouseButton::Right) || keyboard_menu(&keys);
    if review.blocked || search.blocks_controls() || picker.is_open() {
        menu.close();
    } else if navigation.guides_camera() || listing.turning() {
        menu.close();
    } else if menu.is_open() && (keys.just_pressed(KeyCode::Escape) || right_click) {
        menu.close();
    } else if menu.is_open() {
        let shortcut = [
            (KeyCode::KeyE, Action::Approach),
            (KeyCode::KeyG, Action::Diff),
            (KeyCode::KeyB, Action::Blame),
            (KeyCode::KeyU, Action::Paints),
            (KeyCode::KeyJ, Action::Notes),
            (KeyCode::Slash, Action::FindText),
        ]
        .into_iter()
        .find_map(|(key, action)| keys.just_pressed(key).then_some(action));
        let aimed = menu.aim.is_some();
        let available = Action::ALL
            .into_iter()
            .filter(|action| action.available(aimed, &search, review.can_paint, focus.available));
        menu.select(&wheel, &keys, available.clone().count());
        let confirm = mouse.just_pressed(MouseButton::Left)
            || keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
        if let Some(action) = shortcut.or_else(|| {
            confirm
                .then(|| available.clone().nth(menu.selected))
                .flatten()
        }) {
            action.choose(&mut menu, &mut picker, &mut review, &mut search, &mut focus);
        }
    } else if right_click && let Some((point, _)) = nearest.screen {
        menu.anchor = Some(point);
        menu.aim = nearest.file;
    }
    let available = Action::ALL
        .into_iter()
        .filter(|action| {
            action.available(
                menu.aim.is_some(),
                &search,
                review.can_paint,
                focus.available,
            )
        })
        .collect::<Vec<_>>();
    view.draw(
        &menu,
        &available,
        nearest.screen.map_or(Vec2::ZERO, |(_, viewport)| viewport),
        focus.enabled,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_wraps_and_accumulates_trackpad_motion() {
        let mut menu = Menu::default();
        let keys = ButtonInput::default();
        let mut wheel = AccumulatedMouseScroll {
            unit: MouseScrollUnit::Pixel,
            delta: Vec2::new(0., 24.),
        };
        menu.select(&wheel, &keys, 4);
        assert_eq!(menu.selected, 0);
        menu.select(&wheel, &keys, 4);
        assert_eq!(menu.selected, 3);
        wheel.delta.y = -120.;
        menu.select(&wheel, &keys, 4);
        assert_eq!(menu.selected, 0);
    }
}
