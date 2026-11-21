use bevy::input_focus::InputFocus;
use bevy::ui::RelativeCursorPosition;

use super::*;

const FILES_PER_PAGE: usize = 12;
const PAPER: Color = Color::srgb_u8(248, 228, 195);
const GOLD: Color = Color::srgb_u8(222, 175, 115);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Folder(usize),
    Library,
}

#[derive(Clone, Copy, Component)]
enum Choice {
    Folder(usize),
    Scroll(usize),
    Close,
    Move(isize),
}

#[derive(Resource, Default)]
pub(super) struct Map {
    level: Option<Level>,
    cursor: usize,
    panel: Option<Entity>,
    action: Option<Choice>,
    ground: Option<Transform>,
    orbit: f32,
    zoom: f32,
    pub(super) consumed: bool,
}

impl Map {
    pub(super) fn open(&self) -> bool {
        self.level.is_some()
    }

    pub(super) fn birds_eye(&self) -> bool {
        self.level == Some(Level::Library)
    }

    pub(super) fn folder(&self) -> Option<usize> {
        match self.level {
            Some(Level::Folder(index)) => Some(index),
            _ => None,
        }
    }

    pub(super) fn court_center(&self) -> Option<f32> {
        self.level.map(|_| {
            layout::court(self.folder().unwrap_or(self.cursor)) as f32 * layout::COURT_LENGTH
        })
    }

    pub(super) fn focused_scroll(&self, catalog: &Catalog) -> Option<usize> {
        let folder = catalog.folders.get(self.folder()?)?;
        folder.files.get(self.cursor).copied()
    }

    pub(super) fn camera(&self) -> Option<Transform> {
        let (position, focus) = match self.level? {
            Level::Folder(index) => {
                let point = layout::folder(index);
                (
                    Vec3::new(point.x * 0.58 + 5., 24., point.y - 13.),
                    Vec3::new(point.x - 3., 1., point.y),
                )
            }
            Level::Library => {
                let z = layout::court(self.cursor) as f32 * layout::COURT_LENGTH;
                let center = Vec3::new(0., 0., z + 15.);
                let offset = Quat::from_rotation_y(self.orbit)
                    * (Vec3::new(20., 63., -79.) * self.zoom.exp());
                (center + offset, center - Vec3::X * 9.)
            }
        };
        Some(Transform::from_translation(position).looking_at(focus, Vec3::Y))
    }
}

impl native_panel::Actions<Choice> for Map {
    fn activate(&mut self, choice: Choice) {
        self.action = Some(choice);
    }
}

pub(super) fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    aim: Res<aim::Aim>,
    mut library: ResMut<Library>,
    mut map: ResMut<Map>,
    review: Res<review_notes::Review>,
    filter: Res<file_facade::ChangesOnly>,
    mut camera: Query<(Entity, &mut Transform, &Camera), With<room::Camera>>,
    mut focus: ResMut<InputFocus>,
    parents: Query<&ChildOf>,
    (buttons, motion, wheel): (
        Res<ButtonInput<MouseButton>>,
        Res<AccumulatedMouseMotion>,
        Res<AccumulatedMouseScroll>,
    ),
    (windows, pointer): (
        Query<&Window, With<PrimaryWindow>>,
        Query<&RelativeCursorPosition>,
    ),
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
) {
    map.consumed = map.open();
    let previous = (map.level, map.cursor);
    let Ok((camera_entity, mut camera, viewport_camera)) = camera.single_mut() else {
        return;
    };
    let ground = library
        .walk_pose
        .as_ref()
        .or(map.ground.as_ref())
        .unwrap_or(&camera);
    let location = layout::nearest(ground.translation, library.catalog.folders.len());
    let unavailable = library.catalog.folders.is_empty() || (repo.is_changed() && map.open());
    #[cfg(not(target_arch = "wasm32"))]
    let unavailable = unavailable || picker.blocks_controls();
    if unavailable {
        map.level = None;
        map.ground = None;
    } else if library.input.is_none() && !review.blocked && keys.just_pressed(KeyCode::KeyM) {
        let district = match aim.0 {
            Some(aim::Target::Cabinet(index)) => Some(index),
            Some(aim::Target::File(index)) => repo
                .0
                .files
                .get(index)
                .and_then(|file| library.catalog.folder(file.directory()))
                .map(|(index, _, _)| index),
            _ => None,
        };
        (map.level, map.cursor) = match map.level {
            None => {
                map.ground = Some(*camera);
                let index = district.or(location).unwrap_or(0);
                let file = library.catalog.folders[index]
                    .files
                    .iter()
                    .position(|&file| file == explorer.selected)
                    .unwrap_or(0);
                (Some(Level::Folder(index)), file)
            }
            Some(Level::Folder(index)) => {
                map.orbit = 0.;
                map.zoom = 0.;
                (Some(Level::Library), index)
            }
            Some(Level::Library) => (None, 0),
        };
    } else if let Some(level) = map.level {
        let entries = match level {
            Level::Folder(index) => library
                .catalog
                .folders
                .get(index)
                .map_or(&[][..], |cabinet| cabinet.files.as_slice()),
            Level::Library => &[][..],
        };
        let total = if level == Level::Library {
            library.catalog.folders.len()
        } else {
            entries.len()
        };
        let page = page_size(level);
        let mut action = map.action.take();
        if level == Level::Library {
            let over_panel = map
                .panel
                .and_then(|entity| pointer.get(entity).ok())
                .is_some_and(RelativeCursorPosition::cursor_over);
            if !over_panel {
                let scroll = match wheel.unit {
                    bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y,
                    bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y / 40.,
                };
                map.zoom = (map.zoom - scroll * 0.13).clamp(-0.54, 0.52);
                if buttons.pressed(MouseButton::Right) {
                    map.orbit -= motion.delta.x * 0.006;
                }
                if action.is_none()
                    && buttons.just_pressed(MouseButton::Left)
                    && let Ok(window) = windows.single()
                    && let Some(point) = window.cursor_position()
                    && let Ok(ray) =
                        viewport_camera.viewport_to_world(&GlobalTransform::from(*camera), point)
                    && let Some(distance) =
                        ray.intersect_plane(Vec3::new(0., 5.6, 0.), InfinitePlane3d::new(Vec3::Y))
                {
                    let hit = ray.get_point(distance);
                    action = layout::nearest(hit, total)
                        .filter(|&index| {
                            let center = layout::folder(index);
                            (hit.x - center.x.signum() * 11.6).abs() < 5.
                                && (hit.z - center.y).abs() < 7.2
                        })
                        .map(Choice::Folder);
                }
            }
        }
        if keys.any_just_pressed([KeyCode::Escape, KeyCode::Tab]) {
            action = Some(Choice::Close);
        } else if keys.just_pressed(KeyCode::Home) {
            map.cursor = 0;
        } else if keys.just_pressed(KeyCode::End) {
            map.cursor = total.saturating_sub(1);
        } else if keys.any_just_pressed([
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ]) {
            let movement = match level {
                _ if keys.just_pressed(KeyCode::ArrowDown) => 2,
                _ if keys.just_pressed(KeyCode::ArrowUp) => -2,
                _ if keys.just_pressed(KeyCode::ArrowRight) => 1,
                _ if keys.just_pressed(KeyCode::ArrowLeft) => -1,
                _ if keys.just_pressed(KeyCode::PageDown) => page as isize,
                _ => -(page as isize),
            };
            action = Some(Choice::Move(movement));
        } else if action.is_none()
            && !focus.get().is_some_and(|entity| {
                map.panel
                    .is_some_and(|panel| parents.iter_ancestors(entity).any(|p| p == panel))
            })
            && keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])
        {
            action = match level {
                Level::Library => Some(Choice::Folder(map.cursor)),
                Level::Folder(_) => entries.get(map.cursor).copied().map(Choice::Scroll),
            };
        }
        match action {
            Some(Choice::Folder(index)) if index < library.catalog.folders.len() => {
                library.reader = None;
                library.walk_pose = None;
                library.catalog.query.clear();
                let first = library.catalog.folders[index].files.first().copied();
                library.catalog.collect(&repo.0, first, &filter);
                map.ground = Some(layout::view(index));
                (map.level, map.cursor) = (Some(Level::Folder(index)), 0);
            }
            Some(Choice::Scroll(index)) if entries.contains(&index) => {
                if let Level::Folder(folder) = level {
                    *camera = layout::view(folder);
                }
                library.reader = None;
                library.walk_pose = None;
                map.ground = None;
                library.destination = Some(Destination {
                    index,
                    before: repo.0.files[index].after.is_none(),
                    line: 1,
                    end: None,
                    face: listing::Face::Source,
                });
                map.level = None;
            }
            Some(Choice::Move(delta)) => {
                map.cursor = map
                    .cursor
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                focus.clear();
            }
            Some(Choice::Close) => map.level = None,
            _ => {}
        }
    }
    if map.open() {
        map.consumed = true;
    } else if let Some(ground) = map.ground.take() {
        *camera = ground;
    }
    if previous == (map.level, map.cursor) {
        return;
    }
    if let Some(panel) = map.panel.take() {
        native_panel::close(&mut commands, panel, &mut focus, &parents);
    }
    let Some(level) = map.level else {
        #[cfg(target_arch = "wasm32")]
        browser_map("");
        return;
    };
    let title = match level {
        Level::Library => "Archive courtyards".to_owned(),
        Level::Folder(index) => format!("Folder · {}", library.catalog.folders[index].path),
    };
    #[cfg(target_arch = "wasm32")]
    browser_map(&title);
    let panel = native_panel::sidebar(&mut commands, &title, 374., camera_entity);
    commands
        .entity(panel)
        .insert(RelativeCursorPosition::default());
    map.panel = Some(panel);
    let page = page_size(level);
    let start = map.cursor / page * page;
    commands.entity(panel).with_children(|body| {
        body.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|bar| {
            native_panel::text(
                bar,
                match level {
                    Level::Library => "Enter to visit · M to land",
                    Level::Folder(_) => "Enter to read · M whole archive",
                },
                PAPER,
            );
            native_panel::button::<Choice, Map>(bar, "Close", PAPER, Choice::Close);
        });
        match level {
            Level::Library => {
                courtyards(body, &library.catalog, start, map.cursor, location);
                native_panel::text(body, "Wheel to zoom · Right-drag to turn", PAPER);
            }
            Level::Folder(index) => shelves(
                body,
                &library.catalog.folders[index],
                &repo.0,
                &review.notes,
                start,
                map.cursor,
            ),
        }
    });
}

fn page_size(level: Level) -> usize {
    match level {
        Level::Library => layout::FOLDERS_PER_COURT,
        Level::Folder(_) => FILES_PER_PAGE,
    }
}

fn courtyards(
    body: &mut ChildSpawnerCommands,
    catalog: &Catalog,
    start: usize,
    cursor: usize,
    location: Option<usize>,
) {
    let total = catalog.folders.len();
    let courts = total.div_ceil(layout::FOLDERS_PER_COURT);
    native_panel::text(
        body,
        &format!("Courtyard {} / {courts}", layout::court(start) + 1),
        GOLD,
    );
    if let Some(here) = location.and_then(|index| catalog.folders.get(index)) {
        native_panel::text(body, &format!("You are at {}", here.path), PAPER);
    }
    for (row, pair) in catalog.folders[start..].chunks(2).take(2).enumerate() {
        body.spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![
                GridTrack::flex(1.),
                GridTrack::px(14.),
                GridTrack::flex(1.),
            ],
            column_gap: px(5.),
            min_height: px(50.),
            ..default()
        })
        .with_children(|aisle| {
            let index = start + row * 2;
            folder_button(aisle, &pair[0], index, cursor);
            aisle.spawn((
                BackgroundColor(Color::srgb_u8(106, 78, 52)),
                Node {
                    width: percent(100.),
                    ..default()
                },
            ));
            if let Some(right) = pair.get(1) {
                folder_button(aisle, right, index + 1, cursor);
            }
        });
    }
    pages(body, start, layout::FOLDERS_PER_COURT, total, cursor);
    native_panel::text(body, "© OpenStreetMap contributors · ODbL", PAPER);
}

fn folder_button(
    aisle: &mut ChildSpawnerCommands,
    cabinet: &CatalogFolder,
    index: usize,
    cursor: usize,
) {
    let label = format!("{} · {}", cabinet.path, cabinet.files.len());
    native_panel::button::<Choice, Map>(
        aisle,
        &label,
        if index == cursor { GOLD } else { PAPER },
        Choice::Folder(index),
    );
}

fn shelves(
    body: &mut ChildSpawnerCommands,
    cabinet: &CatalogFolder,
    repo: &Repository,
    notes: &[review_notes::Note],
    start: usize,
    cursor: usize,
) {
    let end = (start + FILES_PER_PAGE).min(cabinet.files.len());
    let rack = start / room::ROLLS_PER_CABINET + 1;
    let racks = cabinet.files.len().div_ceil(room::ROLLS_PER_CABINET);
    native_panel::text(
        body,
        &format!("Cabinet {rack} / {racks} · {} scrolls", cabinet.files.len()),
        GOLD,
    );
    for (row, pair) in cabinet.files[start..end].chunks(2).enumerate() {
        body.spawn((
            BackgroundColor(Color::srgb_u8(53, 41, 31)),
            BorderColor::all(Color::srgb_u8(112, 84, 58)),
            Node {
                display: Display::Grid,
                grid_template_columns: vec![GridTrack::flex(1.), GridTrack::flex(1.)],
                column_gap: px(8.),
                min_height: px(30.),
                padding: UiRect::horizontal(px(7.)),
                border: UiRect::bottom(px(4.)),
                ..default()
            },
        ))
        .with_children(|shelf| {
            for (column, &index) in pair.iter().enumerate() {
                let slot = start + row * 2 + column;
                let file = &repo.files[index];
                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                let change = match file.change {
                    Change::Added => "+ ",
                    Change::Modified => "± ",
                    Change::Removed => "− ",
                    Change::Unchanged => "",
                };
                let note = if notes.iter().any(|note| {
                    review_notes::Review::on_snapshot(note, repo) && note.anchor.path == file.path
                }) {
                    "  ◆"
                } else {
                    ""
                };
                native_panel::button::<Choice, Map>(
                    shelf,
                    &format!("{change}{name}{note}"),
                    if slot == cursor { GOLD } else { PAPER },
                    Choice::Scroll(index),
                );
            }
        });
    }
    pages(body, start, FILES_PER_PAGE, cabinet.files.len(), cursor);
}

fn pages(body: &mut ChildSpawnerCommands, start: usize, size: usize, total: usize, cursor: usize) {
    if total <= size {
        return;
    }
    body.spawn(Node {
        column_gap: px(10.),
        ..default()
    })
    .with_children(|bar| {
        if start > 0 {
            native_panel::button::<Choice, Map>(
                bar,
                "Previous",
                PAPER,
                Choice::Move(-(size as isize)),
            );
        }
        native_panel::text(
            bar,
            &format!("{}–{} of {total}", start + 1, (start + size).min(total)),
            PAPER,
        );
        if start + size < total {
            native_panel::button::<Choice, Map>(
                bar,
                "Next",
                PAPER,
                Choice::Move((start + size - cursor) as isize),
            );
        }
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryMap)]
    fn browser_map(title: &str);
}
