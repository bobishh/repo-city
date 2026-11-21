use bevy::input_focus::InputFocus;

use super::*;

const ROWS_PER_PAGE: usize = 8;
const PAPER: Color = Color::srgb_u8(248, 228, 195);
const GOLD: Color = Color::srgb_u8(222, 175, 115);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Cabinet(usize),
    Library,
}

#[derive(Clone, Copy, Component)]
enum Choice {
    Cabinet(usize),
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
    pub(super) consumed: bool,
}

impl Map {
    pub(super) fn open(&self) -> bool {
        self.level.is_some()
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
    mut review: ResMut<review_notes::Review>,
    mut focus: ResMut<InputFocus>,
    parents: Query<&ChildOf>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
) {
    map.consumed = map.open();
    let previous = (map.level, map.cursor);
    let unavailable = library.catalog.cabinets.is_empty() || (repo.is_changed() && map.open());
    #[cfg(not(target_arch = "wasm32"))]
    let unavailable = unavailable || picker.blocks_controls();
    if unavailable {
        map.level = None;
    } else if library.input.is_none() && !review.blocked && keys.just_pressed(KeyCode::KeyM) {
        let district = || {
            let file = match aim.0 {
                Some(aim::Target::Cabinet(index)) => return index,
                Some(aim::Target::File(index)) => index,
                _ => explorer.selected,
            };
            repo.0
                .files
                .get(file)
                .and_then(|file| library.catalog.cabinet(file.directory()))
                .map_or(0, |(index, _, _)| index)
        };
        (map.level, map.cursor) = match map.level {
            None => (Some(Level::Cabinet(district())), 0),
            Some(Level::Cabinet(index)) => (Some(Level::Library), index),
            Some(Level::Library) => (None, 0),
        };
    } else if let Some(level) = map.level {
        let entries = match level {
            Level::Cabinet(index) => library
                .catalog
                .cabinets
                .get(index)
                .map_or(&[][..], |cabinet| cabinet.files.as_slice()),
            Level::Library => &[][..],
        };
        let total = if level == Level::Library {
            library.catalog.cabinets.len()
        } else {
            entries.len()
        };
        let page = page_size(level);
        let mut action = map.action.take();
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
                Level::Library => Some(Choice::Cabinet(map.cursor)),
                Level::Cabinet(_) => entries.get(map.cursor).copied().map(Choice::Scroll),
            };
        }
        match action {
            Some(Choice::Cabinet(index)) if index < library.catalog.cabinets.len() => {
                (map.level, map.cursor) = (Some(Level::Cabinet(index)), 0);
            }
            Some(Choice::Scroll(index)) if entries.contains(&index) => {
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
        review.tablet_open = true;
        map.consumed = true;
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
        Level::Library => "Library corridors".to_owned(),
        Level::Cabinet(index) => format!("Cabinet · {}", library.catalog.cabinets[index].path),
    };
    #[cfg(target_arch = "wasm32")]
    browser_map(&title);
    let panel = native_panel::tablet(&mut commands, &title);
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
                    Level::Library => "Stone Pad · M back to the gallery",
                    Level::Cabinet(_) => "Stone Pad · M library corridors",
                },
                PAPER,
            );
            native_panel::button::<Choice, Map>(bar, "Close", PAPER, Choice::Close);
        });
        match level {
            Level::Library => corridors(body, &library.catalog, start, map.cursor),
            Level::Cabinet(index) => shelves(
                body,
                &library.catalog.cabinets[index],
                &repo.0,
                &review.notes,
                start,
                map.cursor,
            ),
        }
    });
}

fn page_size(level: Level) -> usize {
    ROWS_PER_PAGE * if level == Level::Library { 2 } else { 1 }
}

fn corridors(body: &mut ChildSpawnerCommands, catalog: &Catalog, start: usize, cursor: usize) {
    for (row, pair) in catalog.cabinets[start..]
        .chunks(2)
        .take(ROWS_PER_PAGE)
        .enumerate()
    {
        body.spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![
                GridTrack::flex(1.),
                GridTrack::px(24.),
                GridTrack::flex(1.),
            ],
            column_gap: px(8.),
            min_height: px(32.),
            ..default()
        })
        .with_children(|aisle| {
            let index = start + row * 2;
            cabinet_button(aisle, &pair[0], index, cursor);
            aisle.spawn((
                BackgroundColor(Color::srgb_u8(106, 78, 52)),
                Node {
                    width: percent(100.),
                    ..default()
                },
            ));
            if let Some(right) = pair.get(1) {
                cabinet_button(aisle, right, index + 1, cursor);
            }
        });
    }
    pages(
        body,
        start,
        ROWS_PER_PAGE * 2,
        catalog.cabinets.len(),
        cursor,
    );
}

fn cabinet_button(
    aisle: &mut ChildSpawnerCommands,
    cabinet: &CatalogCabinet,
    index: usize,
    cursor: usize,
) {
    let label = format!("{} · {}", cabinet.path, cabinet.files.len());
    native_panel::button::<Choice, Map>(
        aisle,
        &label,
        if index == cursor { GOLD } else { PAPER },
        Choice::Cabinet(index),
    );
}

fn shelves(
    body: &mut ChildSpawnerCommands,
    cabinet: &CatalogCabinet,
    repo: &Repository,
    notes: &[review_notes::Note],
    start: usize,
    cursor: usize,
) {
    let end = (start + ROWS_PER_PAGE).min(cabinet.files.len());
    for (row, pair) in cabinet.files[start..end].chunks(2).enumerate() {
        body.spawn((
            BackgroundColor(Color::srgb_u8(53, 41, 31)),
            BorderColor::all(Color::srgb_u8(112, 84, 58)),
            Node {
                display: Display::Grid,
                grid_template_columns: vec![GridTrack::flex(1.), GridTrack::flex(1.)],
                column_gap: px(8.),
                min_height: px(47.),
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
    pages(body, start, ROWS_PER_PAGE, cabinet.files.len(), cursor);
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
