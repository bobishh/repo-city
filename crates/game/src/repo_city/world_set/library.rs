use std::collections::BTreeMap;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;

use super::super::*;

mod aim;
mod athens;
mod atlas;
mod forms;
mod journey;
mod layout;
mod librarian;
mod map;
mod owl;
mod paper;
mod place;
mod room;
mod seeing;
mod signs;
mod surfaces;
mod tools;

const SHELF_SIZE: usize = 8;
const SCROLL_ROWS: usize = 22;

#[derive(Resource, Default)]
struct Library {
    catalog: Catalog,
    reader: Option<Reader>,
    input: Option<Input>,
    walk_pose: Option<Transform>,
    retreat: Option<room::Retreat>,
    arrival: Option<usize>,
    destination: Option<Destination>,
    history: city_navigation::History,
    route: Option<journey::Route>,
}

struct Destination {
    index: usize,
    before: bool,
    line: usize,
    end: Option<usize>,
    face: listing::Face,
}

#[derive(Default)]
struct Catalog {
    query: String,
    files: Vec<usize>,
    folders: Vec<CatalogFolder>,
    selected: usize,
}

struct CatalogFolder {
    path: String,
    files: Vec<usize>,
}

impl Catalog {
    fn index(&mut self, repo: &Repository) {
        let mut directories = BTreeMap::<String, Vec<usize>>::new();
        for (index, file) in repo.files.iter().enumerate() {
            directories
                .entry(file.directory().to_owned())
                .or_default()
                .push(index);
        }
        self.folders = directories
            .into_iter()
            .map(|(path, mut files)| {
                files.sort_by_cached_key(|&index| {
                    let path = &repo.files[index].path;
                    (path.to_lowercase(), path)
                });
                CatalogFolder { path, files }
            })
            .collect();
    }

    fn folder(&self, name: &str) -> Option<(usize, &str, usize)> {
        let index = self
            .folders
            .binary_search_by(|cabinet| cabinet.path.as_str().cmp(name))
            .ok()?;
        let cabinet = &self.folders[index];
        Some((index, &cabinet.path, cabinet.files.len()))
    }

    fn collect(
        &mut self,
        repo: &Repository,
        keep: Option<usize>,
        focus: &file_facade::ChangesOnly,
    ) {
        let query = self.query.to_lowercase();
        let mut matches: Vec<_> = repo
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| focus.includes(file))
            .filter_map(|(index, file)| {
                if query.is_empty() {
                    Some((0, file.directory(), &file.path, index))
                } else {
                    file_picker::rank(&file.path, &query).map(|rank| (rank, "", &file.path, index))
                }
            })
            .collect();
        matches.sort_unstable();
        self.files = matches.into_iter().map(|(_, _, _, index)| index).collect();
        self.selected = keep
            .and_then(|index| self.files.iter().position(|&file| file == index))
            .unwrap_or(0);
    }

    fn shelf(&self) -> &[usize] {
        let start = self.selected / SHELF_SIZE * SHELF_SIZE;
        &self.files[start..(start + SHELF_SIZE).min(self.files.len())]
    }

    fn step(&mut self, by: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(by)
            .min(self.files.len().saturating_sub(1));
    }
}

struct Reader {
    first: usize,
    arrival: Option<usize>,
    column: usize,
    target: Option<usize>,
    face: listing::Face,
    guidance: Option<(std::ops::RangeInclusive<usize>, f64)>,
    look: Vec2,
    matches: Vec<usize>,
}

impl Reader {
    fn new(line: usize) -> Self {
        Self {
            first: line.saturating_sub(1),
            arrival: (line > 1).then_some(line),
            column: 0,
            target: Some(line),
            face: listing::Face::Source,
            guidance: None,
            look: Vec2::ZERO,
            matches: Vec::new(),
        }
    }

    fn page(&self, rows: usize) -> std::ops::Range<usize> {
        let start = self.first.min(rows.saturating_sub(1));
        start..(start + SCROLL_ROWS).min(rows)
    }

    fn step(&mut self, by: isize, rows: usize) {
        self.first = self
            .page(rows)
            .start
            .saturating_add_signed(by)
            .min(rows.saturating_sub(1));
        self.target = None;
    }
}

enum Input {
    Catalog,
    Line(String),
    Text(String),
}

pub(super) fn install(app: &mut App, args: &[String]) {
    let arrival = args.iter().any(|arg| arg == "--file").then(|| {
        args.windows(2)
            .find(|pair| pair[0] == "--line")
            .and_then(|pair| pair[1].parse::<usize>().ok())
            .unwrap_or(1)
            .max(1)
    });
    tools::register(app, args);
    app.insert_resource(Library {
        arrival,
        ..default()
    })
    .init_resource::<room::Unrolling>()
    .init_resource::<aim::Aim>()
    .init_resource::<map::Map>()
    .init_resource::<city_navigation::Navigation>()
    .add_systems(Startup, room::setup)
    .add_systems(
        Update,
        (
            (
                #[cfg(not(target_arch = "wasm32"))]
                repository_picker::open_repository,
                #[cfg(not(target_arch = "wasm32"))]
                live_worktree::update,
                #[cfg(target_arch = "wasm32")]
                browser_switch,
                refresh_repository,
                progress::restore,
                #[cfg(target_arch = "wasm32")]
                browser_refresh.run_if(repository_picker::has_city),
                review_notes::refresh,
                #[cfg(target_arch = "wasm32")]
                touch_input,
                city_menu::apply_filter_shortcut.run_if(
                    |library: Res<Library>, map: Res<map::Map>, keys: Res<ButtonInput<KeyCode>>| {
                        library.input.is_none()
                            && !map.open()
                            && !keys.any_pressed([
                                KeyCode::ControlLeft,
                                KeyCode::ControlRight,
                                KeyCode::SuperLeft,
                                KeyCode::SuperRight,
                            ])
                    },
                ),
                refresh_filter,
                tools::requests,
                review_notes::panels,
                #[cfg(not(target_arch = "wasm32"))]
                review_notes::update_color_picker,
                #[cfg(not(target_arch = "wasm32"))]
                review_tablet::update.run_if(|library: Res<Library>, map: Res<map::Map>| {
                    library.input.is_none() && !map.open()
                }),
                map::update,
                tools::destinations,
                trace_import::update,
                journey::update,
            )
                .chain(),
            (
                controls,
                room::draw,
                map::draw_shade,
                owl::fly,
                room::arrange_cabinets,
                aim::update,
                signs::draw,
                seeing::update,
                seeing::share,
                place::save,
                progress::sync.run_if(repository_picker::has_city),
                android::show_tablet,
            )
                .chain(),
        )
            .chain(),
    );
}

fn refresh_repository(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut library: ResMut<Library>,
    mut scope: Local<Option<(PathBuf, String, Option<String>)>>,
    mut listing: ResMut<listing::Listing>,
    mut room: ResMut<review_party::Scene>,
    mut filter: ResMut<file_facade::ChangesOnly>,
    mut progress: ResMut<progress::Progress>,
    world: Res<WorldSet>,
    mut camera: Query<&mut Transform, With<room::Camera>>,
) {
    if !repo.is_changed() {
        return;
    }
    let fresh = scope.as_ref().is_none_or(|(root, revision, baseline)| {
        *root != repo.0.root || *revision != repo.0.revision || *baseline != repo.0.baseline
    });
    if fresh {
        filter.enabled = false;
        listing.clear_progress();
        let arrival = library.arrival.take();
        *library = Library {
            arrival,
            ..default()
        };
        *scope = Some((
            repo.0.root.clone(),
            repo.0.revision.clone(),
            repo.0.baseline.clone(),
        ));
        if let Ok(mut camera) = camera.single_mut() {
            *camera = room::entrance();
            if explorer.resume_on_open
                && library.arrival.is_none()
                && let Some(saved) = progress.world_place::<place::Place>(&repo.0, &world.id)
                && let Some((destination, pose)) = saved.restore(&repo.0)
            {
                library.destination = Some(destination);
                *camera = pose;
            }
        }
    }
    library.catalog.index(&repo.0);
    room.positions.clear();
    for (index, cabinet) in library.catalog.folders.iter().enumerate() {
        let position = room::cabinet_marker(index, cabinet.files.len());
        for &file in &cabinet.files {
            room.positions
                .insert(repo.0.files[file].path.clone(), position);
        }
    }
    filter.measure(&repo.0);
    library
        .catalog
        .collect(&repo.0, Some(explorer.selected), &filter);
    if library.reader.is_some() && repo.0.files.is_empty() {
        library.reader = None;
    }
}

fn refresh_filter(
    repo: Res<Repo>,
    mut library: ResMut<Library>,
    explorer: Res<Explorer>,
    focus: Res<file_facade::ChangesOnly>,
) {
    if focus.is_changed() {
        library
            .catalog
            .collect(&repo.0, Some(explorer.selected), &focus);
    }
}

fn controls(
    mut library: ResMut<Library>,
    mut progress: ResMut<progress::Progress>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    (review, aim, map, filter, settings): (
        Res<review_notes::Review>,
        Res<aim::Aim>,
        Res<map::Map>,
        Res<file_facade::ChangesOnly>,
        Res<reading::Settings>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    mut typing: MessageReader<KeyboardInput>,
    mut camera: Query<(&mut Transform, &mut Pilot), With<room::Camera>>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] mut menu_find: ResMut<file_picker::FilePicker>,
    #[cfg(target_arch = "wasm32")] touch: Res<browser_touch::Touch>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if picker.blocks_controls() {
        typing.clear();
        return;
    }
    let Ok((mut camera, mut pilot)) = camera.single_mut() else {
        return;
    };
    if review.blocked || review.tablet_open || map.consumed {
        pilot.captured = false;
        typing.clear();
        return;
    }
    let library = &mut *library;

    let was_captured = pilot.captured;
    let captured = capture_cursor(
        was_captured,
        windows.single().ok(),
        &keys,
        &buttons,
        library.input.is_some(),
    );
    pilot.captured = captured;
    let Some(file) = repo.0.files.get(explorer.selected) else {
        typing.clear();
        return;
    };
    if let Some(line) = library.arrival.take() {
        library.walk_pose = Some(
            library
                .catalog
                .folder(file.directory())
                .map_or(*camera, |(folder, _, _)| layout::view(folder)),
        );
        let mut reader = Reader::new(line);
        reader.first = paper::Rows::new(&explorer, reader.face).locate(line);
        library.reader = Some(reader);
        progress.visit(explorer.selected, file, false);
    }
    if let Some(destination) = library.destination.take() {
        library.retreat = None;
        explorer.open(&repo.0, destination.index);
        library.catalog.query.clear();
        library
            .catalog
            .collect(&repo.0, Some(destination.index), &filter);
        if explorer.before != destination.before {
            explorer.before = destination.before;
            explorer.reload(&repo.0);
        }
        progress.visit(destination.index, &repo.0.files[destination.index], false);
        let saved = library.walk_pose.unwrap_or(*camera);
        library.walk_pose = Some(
            library
                .catalog
                .folder(repo.0.files[destination.index].directory())
                .map_or(saved, |(folder, _, _)| {
                    if layout::nearest(saved.translation, library.catalog.folders.len())
                        == Some(folder)
                        && saved.translation.x.abs() > layout::FRONT
                    {
                        saved
                    } else {
                        layout::view(folder)
                    }
                }),
        );
        library.input = None;
        let mut reader = Reader::new(destination.line);
        reader.face = destination.face;
        reader.first = paper::Rows::new(&explorer, reader.face).locate(destination.line);
        if let Some(end) = destination.end {
            reader.guidance = Some((destination.line..=end, time.elapsed_secs_f64() + 4.));
        }
        library.reader = Some(reader);
        typing.clear();
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let joystick = Vec2::from_array(touch.r#move);
    #[cfg(not(target_arch = "wasm32"))]
    let joystick = Vec2::ZERO;
    let enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    let escape = keys.just_pressed(KeyCode::Escape);
    let find = keys.any_just_pressed([KeyCode::Slash, KeyCode::Backslash]);
    if let Some(retreat) = &mut library.retreat {
        if !retreat.advance(&mut camera, time.delta_secs()) {
            typing.clear();
            return;
        }
        library.retreat = None;
    }
    if let Some(input) = &mut library.input {
        if escape || enter {
            match library.input.take().expect("active library input") {
                Input::Catalog => {
                    if escape {
                        library.catalog.query.clear();
                        library
                            .catalog
                            .collect(&repo.0, Some(explorer.selected), &filter);
                    }
                }
                Input::Line(line) if enter => {
                    if let Ok(line) = line.parse::<usize>()
                        && let Some(reader) = &mut library.reader
                    {
                        reader.first = paper::Rows::new(&explorer, reader.face).locate(line);
                        reader.target = Some(line);
                    }
                    typing.clear();
                    return;
                }
                Input::Text(query) if enter => {
                    if let Some(reader) = &mut library.reader {
                        let content = paper::Rows::new(&explorer, reader.face);
                        let needle = query.trim().to_lowercase();
                        reader.matches = (0..content.len())
                            .filter(|&index| {
                                !needle.is_empty()
                                    && content.row(index).is_some_and(|entry| {
                                        entry.line.text.to_lowercase().contains(&needle)
                                    })
                            })
                            .collect();
                        if let Some(&first) = reader
                            .matches
                            .iter()
                            .find(|&&index| index >= reader.first)
                            .or_else(|| reader.matches.first())
                        {
                            reader.first = first;
                            reader.target = content.row(first).map(|row| row.line.number);
                        } else if !needle.is_empty() {
                            explorer.notice = "No matching text in this scroll.".to_owned();
                        }
                    }
                    typing.clear();
                    return;
                }
                _ => {
                    typing.clear();
                    return;
                }
            }
        } else {
            let (value, digits) = match input {
                Input::Catalog => (&mut library.catalog.query, false),
                Input::Line(line) => (line, true),
                Input::Text(text) => (text, false),
            };
            if edit(value, digits, typing.read()) && matches!(library.input, Some(Input::Catalog)) {
                library.catalog.collect(&repo.0, None, &filter);
            }
        }
    } else {
        typing.clear();
    }

    if let Some(reader) = &mut library.reader {
        let moving = library.input.is_none()
            && !keys.any_pressed([
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
                KeyCode::SuperLeft,
                KeyCode::SuperRight,
                KeyCode::AltLeft,
                KeyCode::AltRight,
            ])
            && (keys.any_just_pressed([
                KeyCode::KeyW,
                KeyCode::KeyA,
                KeyCode::KeyS,
                KeyCode::KeyD,
            ]) || joystick.length_squared() > 0.09);
        if escape || moving {
            library.reader = None;
            if let Some(pose) = library.walk_pose.take() {
                library.retreat = Some(room::Retreat::new(*camera, pose));
            }
            return;
        }
        if library.input.is_some() {
            return;
        }
        if captured || buttons.pressed(MouseButton::Right) {
            reader.look =
                (reader.look - motion.delta * 0.0025).clamp(Vec2::splat(-0.27), Vec2::splat(0.27));
        }
        let rows = paper::Rows::new(&explorer, reader.face);
        // Remote blobs can arrive after the repository and its saved reading position.
        if rows.len() != 0
            && let Some(line) = reader.arrival.take()
        {
            reader.first = rows.locate(line);
        }
        if find
            || (keys.just_pressed(KeyCode::KeyF)
                && keys.any_pressed([
                    KeyCode::ControlLeft,
                    KeyCode::ControlRight,
                    KeyCode::SuperLeft,
                    KeyCode::SuperRight,
                ]))
        {
            library.input = Some(Input::Text(String::new()));
        } else if enter && !reader.matches.is_empty() {
            let back = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            let next = if back {
                reader
                    .matches
                    .iter()
                    .rev()
                    .find(|&&index| index < reader.first)
                    .or_else(|| reader.matches.last())
            } else {
                reader
                    .matches
                    .iter()
                    .find(|&&index| index > reader.first)
                    .or_else(|| reader.matches.first())
            };
            if let Some(&next) = next {
                reader.first = next;
                reader.target = rows.row(next).map(|row| row.line.number);
            }
        } else if keys.any_just_pressed([KeyCode::KeyG, KeyCode::KeyB]) {
            let requested = if keys.just_pressed(KeyCode::KeyG) {
                listing::Face::Diff
            } else {
                listing::Face::Blame
            };
            let previous = rows.row(reader.first).map_or(1, |row| row.line.number);
            reader.guidance = None;
            reader.matches.clear();
            reader.face = if reader.face == requested {
                listing::Face::Source
            } else {
                requested
            };
            let content = paper::Rows::new(&explorer, reader.face);
            reader.first = if reader.face == listing::Face::Diff {
                explorer
                    .comparison
                    .as_ref()
                    .and_then(|comparison| comparison.hunks.first())
                    .map_or(0, |hunk| hunk.row)
            } else {
                content.locate(previous)
            };
            reader.column = 0;
            reader.target = None;
            progress.visit(explorer.selected, file, reader.face == listing::Face::Diff);
        } else if keys.just_pressed(KeyCode::KeyQ) && file.before.is_some() && file.after.is_some()
        {
            let line = rows.row(reader.first).map_or(1, |row| row.line.number);
            reader.matches.clear();
            explorer.before = !explorer.before;
            explorer.reload(&repo.0);
            reader.first = paper::Rows::new(&explorer, reader.face).locate(line);
            reader.target = None;
        } else if keys.just_pressed(KeyCode::KeyJ)
            && !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
        {
            library.input = Some(Input::Line(String::new()));
        } else if reader.face == listing::Face::Source
            && keys.any_just_pressed([KeyCode::KeyZ, KeyCode::KeyX])
        {
            let aimed = match aim.0 {
                Some(aim::Target::Line { file, number, .. }) if file == explorer.selected => number,
                _ => rows.row(reader.first).map_or(1, |row| row.line.number),
            };
            let range = reading::callable_for_line(&explorer, aimed).unwrap_or_else(|| {
                settings.rows(aimed.saturating_sub(1), explorer.highlighted.len())
            });
            let source = &mut *explorer;
            let changed = if keys.just_pressed(KeyCode::KeyZ) {
                source.source_layout.fold(&source.highlighted, range)
            } else {
                source
                    .source_layout
                    .unfold(&source.highlighted, aimed.saturating_sub(1))
            };
            if changed {
                reader.first = paper::Rows::new(source, reader.face).locate(aimed);
                reader.matches.clear();
            }
        } else if reader.face == listing::Face::Diff
            && keys.any_just_pressed([KeyCode::BracketLeft, KeyCode::BracketRight])
            && let Some(diff) = &explorer.comparison
        {
            let hunk = if keys.just_pressed(KeyCode::BracketLeft) {
                diff.hunks.iter().rev().find(|hunk| hunk.row < reader.first)
            } else {
                diff.hunks.iter().find(|hunk| hunk.row > reader.first)
            };
            if let Some(hunk) = hunk {
                reader.first = hunk.row;
                reader.target = None;
            }
        } else if keys.just_pressed(KeyCode::Home) {
            reader.first = 0;
            reader.target = None;
        } else if keys.just_pressed(KeyCode::End) {
            reader.first = rows.len().saturating_sub(SCROLL_ROWS);
            reader.target = None;
        } else if keys.just_pressed(KeyCode::PageUp) {
            reader.step(-(SCROLL_ROWS as isize - 1), rows.len());
        } else if keys.just_pressed(KeyCode::PageDown) {
            reader.step(SCROLL_ROWS as isize - 1, rows.len());
        } else if keys.just_pressed(KeyCode::ArrowUp) || wheel.delta.y > 0. {
            reader.step(-1, rows.len());
        } else if keys.just_pressed(KeyCode::ArrowDown) || wheel.delta.y < 0. {
            reader.step(1, rows.len());
        } else if keys.just_pressed(KeyCode::ArrowLeft) {
            reader.column = reader.column.saturating_sub(12);
        } else if keys.just_pressed(KeyCode::ArrowRight) {
            let max = reader
                .page(rows.len())
                .filter_map(|index| rows.row(index))
                .map(|row| row.line.text.chars().count())
                .max()
                .unwrap_or(0);
            if reader.column.saturating_add(12) < max {
                reader.column += 12;
            }
        }
        return;
    }

    #[cfg(not(target_arch = "wasm32"))]
    if menu_find.is_open() {
        *menu_find = default();
        library.input = Some(Input::Catalog);
    }
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if library.input.is_none() && (find || (command && keys.just_pressed(KeyCode::KeyP))) {
        library.catalog.query.clear();
        library
            .catalog
            .collect(&repo.0, Some(explorer.selected), &filter);
        library.input = Some(Input::Catalog);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        library.catalog.step(-1);
    } else if keys.just_pressed(KeyCode::ArrowDown) {
        library.catalog.step(1);
    } else if keys.just_pressed(KeyCode::PageUp) {
        library.catalog.step(-(SHELF_SIZE as isize));
    } else if keys.just_pressed(KeyCode::PageDown) {
        library.catalog.step(SHELF_SIZE as isize);
    }
    let approach = library.input.is_none()
        && (keys.just_pressed(KeyCode::KeyE)
            || (was_captured && buttons.just_pressed(MouseButton::Left)));
    let mut chosen = library.catalog.files.get(library.catalog.selected).copied();
    if approach {
        match aim.0 {
            Some(aim::Target::File(index)) => chosen = Some(index),
            Some(aim::Target::Cabinet(group)) => {
                if let Some(group) = library.catalog.folders.get(group)
                    && let Some(&first) = group
                        .files
                        .iter()
                        .find(|&&index| filter.includes(&repo.0.files[index]))
                {
                    library.catalog.query.clear();
                    library.catalog.collect(&repo.0, Some(first), &filter);
                }
                return;
            }
            _ => return,
        }
    }
    if (enter || approach)
        && let Some(index) = chosen
    {
        explorer.open(&repo.0, index);
        progress.visit(index, &repo.0.files[index], false);
        library.walk_pose = Some(
            library
                .catalog
                .folder(repo.0.files[index].directory())
                .map_or(*camera, |(cabinet, _, _)| layout::view(cabinet)),
        );
        library.input = None;
        library.reader = Some(Reader::new(1));
    } else if library.input.is_none() && !command {
        let mouse = if captured || buttons.pressed(MouseButton::Right) {
            motion.delta
        } else {
            Vec2::ZERO
        };
        #[cfg(target_arch = "wasm32")]
        let mouse = mouse + Vec2::from_array(touch.look);
        room::walk(&mut camera, &keys, time.delta_secs(), mouse, joystick);
    }
}

#[cfg(target_arch = "wasm32")]
fn touch_input(
    library: Res<Library>,
    mut touch: ResMut<browser_touch::Touch>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: ResMut<AccumulatedMouseScroll>,
) {
    browser_touch::update_library(
        library.reader.is_some(),
        &mut touch,
        &mut keys,
        &mut mouse,
        &mut wheel,
    );
}

fn capture_cursor(
    previous: bool,
    window: Option<&Window>,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    editing: bool,
) -> bool {
    if editing || keys.just_pressed(KeyCode::Escape) {
        #[cfg(target_arch = "wasm32")]
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            document.exit_pointer_lock();
        }
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    return window.is_some_and(|window| window.focused)
        && (previous || buttons.just_pressed(MouseButton::Left));
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (previous, window, buttons);
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.pointer_lock_element())
            .is_some_and(|element| element.id() == "repo-city")
    }
}

fn edit<'a>(
    value: &mut String,
    digits: bool,
    events: impl Iterator<Item = &'a KeyboardInput>,
) -> bool {
    let mut changed = false;
    for event in events.filter(|event| event.state == ButtonState::Pressed) {
        if event.key_code == KeyCode::Backspace {
            changed |= value.pop().is_some();
        } else if let Some(text) = event.text.as_deref() {
            for ch in text
                .chars()
                .filter(|ch| !ch.is_control() && (!digits || ch.is_ascii_digit()))
            {
                if value.len() < 256 {
                    value.push(ch);
                    changed = true;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scroll_reaches_an_exact_line_and_the_end_of_a_long_file() {
        let mut reader = Reader::new(757);
        assert_eq!(reader.page(900), 756..778);
        reader.step(500, 900);
        assert_eq!(reader.page(900), 899..900);
        reader.step(-2000, 900);
        assert_eq!(reader.page(900), 0..22);
        assert_eq!(reader.page(0), 0..0);
    }

    #[test]
    fn the_catalog_finds_repository_paths_without_city_coordinates() {
        let mut repo = repository_picker::empty();
        for path in ["z.rs", "src/main.rs", "a.rs"] {
            repo.files.push(repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [f32::NAN, f32::NAN],
            });
        }
        let mut catalog = Catalog::default();
        catalog.collect(&repo, Some(1), &default());
        assert_eq!(catalog.shelf(), &[2, 0, 1]);
        catalog.query = "src/main".into();
        catalog.collect(&repo, None, &default());
        assert_eq!(catalog.shelf(), &[1]);
        assert_eq!(catalog.selected, 0);
    }

    #[test]
    fn cabinets_use_the_same_exact_directory_keys_as_city_districts() {
        let mut repo = repository_picker::empty();
        for path in ["a.txt", "src/main.rs", "src/lib.rs", "src/ui/panel.rs"] {
            repo.files.push(repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [f32::NAN, f32::NAN],
            });
        }
        let mut catalog = Catalog::default();
        catalog.index(&repo);
        assert_eq!(catalog.folder("/"), Some((0, "/", 1)));
        assert_eq!(catalog.folder("src"), Some((1, "src", 2)));
        assert_eq!(catalog.folder("src/ui"), Some((2, "src/ui", 1)));
    }
}
