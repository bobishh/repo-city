//! Find text in the active wall and translate the camera to the matching line.
use bevy::input::keyboard::KeyboardInput;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
struct WallKey {
    file: usize,
    before: bool,
    face: Face,
}

#[derive(Resource, Default)]
pub(crate) struct Search {
    pub request: bool,
    pub available: bool,
    editing: bool,
    suppress: bool,
    key: Option<WallKey>,
    query: String,
    hits: Vec<usize>,
    selected: usize,
    remaining: f32,
    active_wall: bool,
}

impl Search {
    pub fn blocks_controls(&self) -> bool {
        self.editing || self.suppress
    }
    pub fn owns_next(&self) -> bool {
        self.active_wall && (!self.hits.is_empty() || self.editing)
    }
}

pub(crate) fn movement_enabled(search: Res<Search>) -> bool {
    !search.blocks_controls()
}

#[derive(Component)]
pub(crate) struct SearchHud;

pub(crate) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands.spawn((
        SearchHud,
        Text::new(""),
        font.clone(),
        TextColor(ink),
        GlobalZIndex(6),
        BackgroundColor(Color::srgba(0.03, 0.08, 0.10, 0.97)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            bottom: px(24.),
            right: px(24.),
            max_width: percent(80.),
            padding: UiRect::axes(px(12.), px(8.)),
            ..default()
        },
        bevy::picking::Pickable::IGNORE,
    ));
}

fn matching_lines<'a>(lines: impl Iterator<Item = &'a str>, needle: &str) -> Vec<usize> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    lines
        .enumerate()
        .filter_map(|(row, line)| line.to_lowercase().contains(&needle).then_some(row))
        .collect()
}

fn nearest(hits: &[usize], row: usize) -> usize {
    let index = hits.partition_point(|&hit| hit < row);
    if index >= hits.len() { 0 } else { index }
}

fn cycle(index: usize, count: usize, previous: bool) -> usize {
    if count == 0 {
        0
    } else if previous {
        (index + count - 1) % count
    } else {
        (index + 1) % count
    }
}

fn ease(camera: &mut Transform, remaining: &mut f32, dt: f32) {
    let step = if remaining.abs() <= 0.0008 {
        *remaining
    } else {
        *remaining * (1. - (-dt / 0.105).exp())
    };
    camera.translation.y = (camera.translation.y + step).max(CAMERA_FLOOR);
    *remaining -= step;
}

pub(crate) fn update(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    keys: Res<ButtonInput<KeyCode>>,
    mut input: MessageReader<KeyboardInput>,
    mut listing: ResMut<Listing>,
    mut search: ResMut<Search>,
    mut eye: Query<&mut Transform, PlayerCamera>,
    picker: Res<file_picker::FilePicker>,
    review: Res<review_notes::Review>,
    menu: Res<city_menu::Menu>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut hud: Query<(&mut Node, &mut Text), With<SearchHud>>,
    time: Res<Time>,
) {
    if repo.is_changed() {
        *search = Search::default();
    }
    search.suppress = false;
    let Ok(mut eye) = eye.single_mut() else {
        return;
    };
    let Ok((mut node, mut text)) = hud.single_mut() else {
        return;
    };
    let file = &repo.0.files[explorer.selected];
    let size = super::block_size(file);
    let side = if Listing::visible(&explorer, &navigation) && !listing.turning() {
        Face::at_camera(file, size, &eye, (size.x * 1.85).max(5.)).and_then(|face| {
            let count = if face == Face::Diff {
                explorer.diff_layout.rows.len()
            } else if face == Face::Source {
                explorer.source_layout.len()
            } else {
                explorer.blame_layout.len()
            };
            let side = WallSide::new(file, count, face, size);
            (count > 0 && side.look_height(&eye) <= side.wall.top).then_some((side, count))
        })
    } else {
        None
    };
    search.available = side.is_some();
    let wall_key = |face| WallKey {
        file: explorer.selected,
        before: explorer.before,
        face,
    };
    search.active_wall = side
        .as_ref()
        .is_some_and(|(side, _)| search.key == Some(wall_key(side.face)));
    let requested = std::mem::take(&mut search.request);
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let focused = window.single().is_ok_and(|window| window.focused);
    let blocked = review.blocked || picker.is_open() || menu.is_open() || !focused;
    if !blocked
        && !search.editing
        && (requested || (!command && keys.just_pressed(KeyCode::Slash)))
        && let Some((side, _)) = &side
    {
        let key = wall_key(side.face);
        if search.key != Some(key) {
            search.query.clear();
            search.hits.clear();
        }
        search.key = Some(key);
        search.active_wall = true;
        search.editing = true;
        search.remaining = 0.;
        listing.scroll_remaining = 0.;
        // Consume the slash that opened the text field.
        input.clear();
    }
    if search.editing && (!search.active_wall || blocked) {
        search.editing = false;
    }
    let mut jump = false;
    let mut direction = None;
    if search.editing {
        search.suppress = true;
        let previous = search.query.clone();
        for event in input.read().filter(|event| event.state.is_pressed()) {
            match event.key_code {
                KeyCode::Escape => {
                    search.editing = false;
                    search.query.clear();
                    search.hits.clear();
                    search.remaining = 0.;
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    search.editing = false;
                    jump = true;
                }
                KeyCode::ArrowDown => direction = Some(false),
                KeyCode::ArrowUp => direction = Some(true),
                KeyCode::KeyA if command => search.query.clear(),
                KeyCode::Backspace => {
                    search.query.pop();
                }
                _ if !command && let Some(value) = &event.text => {
                    let available = 160usize.saturating_sub(search.query.chars().count());
                    search
                        .query
                        .extend(value.chars().filter(|c| !c.is_control()).take(available));
                }
                _ => {}
            }
            if !search.editing {
                break;
            }
        }
        if search.query != previous
            && let Some((side, count)) = &side
        {
            search.hits = if side.face == Face::Diff {
                explorer.comparison.as_ref().map_or_else(Vec::new, |diff| {
                    explorer.diff_layout.find_rows(diff, &search.query)
                })
            } else if side.face == Face::Source {
                explorer
                    .source_layout
                    .find_rows(&explorer.highlighted, &search.query)
            } else {
                explorer
                    .blame_layout
                    .find_rows(&explorer.highlighted, &search.query)
            };
            search.selected = nearest(&search.hits, side.wall.row(side.look_height(&eye), *count));
            jump = true;
        }
    } else {
        input.clear();
        if !blocked
            && search.active_wall
            && !search.hits.is_empty()
            && !command
            && keys.just_pressed(KeyCode::KeyN)
        {
            direction = Some(keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
        }
    }

    if let Some(previous) = direction {
        search.selected = cycle(search.selected, search.hits.len(), previous);
        jump = true;
    }
    if search.active_wall && !blocked {
        if jump
            && let (Some((side, count)), Some(&row)) = (&side, search.hits.get(search.selected))
            && row < *count
        {
            listing.scroll_remaining = 0.;
            search.remaining = side.wall.y(row as f32 + 0.5) - side.look_height(&eye);
        }
        if search.remaining != 0. {
            ease(&mut eye, &mut search.remaining, time.delta_secs());
        }
    } else {
        search.remaining = 0.;
    }

    let displayed = !blocked && search.active_wall && (search.editing || !search.query.is_empty());
    node.display = if displayed {
        Display::Flex
    } else {
        Display::None
    };
    if displayed {
        let result = if search.query.trim().is_empty() {
            "Type to find text".into()
        } else if search.hits.is_empty() {
            "No matching lines".into()
        } else {
            format!(
                "{} / {} matching lines",
                search.selected + 1,
                search.hits.len()
            )
        };
        let hint = if search.editing {
            "Enter keeps results · Esc clears"
        } else {
            "N next · Shift+N prev · / edit"
        };
        text.set_if_neq(Text::new(format!(
            "/ {}{}\n{result} · {hint}",
            search.query,
            if search.editing { "|" } else { "" }
        )));
    }
}

pub(crate) fn mark(
    mut gizmos: Gizmos,
    search: Res<Search>,
    explorer: Res<Explorer>,
    repo: Res<Repo>,
) {
    if !search.active_wall {
        return;
    }
    let (Some(key), Some(&row)) = (search.key, search.hits.get(search.selected)) else {
        return;
    };
    let file = &repo.0.files[key.file];
    let count = match key.face {
        Face::Source => explorer.source_layout.len(),
        Face::Diff => explorer.diff_layout.rows.len(),
        Face::Blame => explorer.blame_layout.len(),
    };
    if count == 0 || row >= count {
        return;
    }
    let side = WallSide::new(file, count, key.face, super::block_size(file));
    let cross = Vec3::Y.cross(side.normal());
    let start = side.top - cross * (side.width * 0.49) + side.normal() * 0.06;
    let line = start.with_y(side.wall.y(row as f32 + 0.5));
    let tip = cross * (side.width * 0.026);
    let spread = Vec3::Y * (side.wall.row_height * 0.29);
    let ink = Color::srgb_u8(249, 218, 126);
    gizmos.line(line + spread, line + tip, ink);
    gizmos.line(line - spread, line + tip, ink);
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive_lines_and_cycles_in_both_directions() {
        let rows = [
            "init",
            "match Line",
            "nothing",
            "MATCH line",
            "last match line",
        ];
        let matches = matching_lines(rows.into_iter(), "match line");
        assert_eq!(matches, [1, 3, 4]);
        assert_eq!(nearest(&matches, 2), 1);
        assert_eq!(cycle(2, matches.len(), false), 0);
        assert_eq!(cycle(0, matches.len(), true), 2);
    }

    #[test]
    fn visiting_a_result_moves_parallel_to_the_wall_without_changing_zoom() {
        let mut pose =
            Transform::from_xyz(3., 8., 5.).looking_to(Vec3::new(0.2, -0.14, -1.), Vec3::Y);
        let original = pose;
        let mut remaining = -5.;
        for _ in 0..120 {
            ease(&mut pose, &mut remaining, 1. / 60.);
        }
        assert_eq!(pose.rotation, original.rotation);
        assert_eq!(pose.translation.xz(), original.translation.xz());
        assert!((pose.translation.y - 3.).abs() < 0.001);
    }
}
