use std::collections::BTreeMap;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;

use super::super::*;

mod librarian;
mod room;

const SHELF_SIZE: usize = 8;
const SCROLL_ROWS: usize = 22;

#[derive(Resource, Default)]
struct Library {
    catalog: Catalog,
    reader: Option<Reader>,
    input: Option<Input>,
    walk_pose: Option<Transform>,
    arrival: Option<usize>,
}

#[derive(Default)]
struct Catalog {
    query: String,
    files: Vec<usize>,
    cabinets: Vec<(String, usize)>,
    selected: usize,
}

impl Catalog {
    fn index(&mut self, repo: &Repository) {
        let mut directories = BTreeMap::new();
        for file in &repo.files {
            *directories.entry(file.directory().to_owned()).or_insert(0) += 1;
        }
        self.cabinets = directories.into_iter().collect();
    }

    fn cabinet(&self, name: &str) -> Option<(usize, &str, usize)> {
        let index = self
            .cabinets
            .binary_search_by(|(path, _)| path.as_str().cmp(name))
            .ok()?;
        let (path, files) = &self.cabinets[index];
        Some((index, path, *files))
    }

    fn collect(&mut self, repo: &Repository, keep: Option<usize>) {
        let query = self.query.to_lowercase();
        let mut matches: Vec<_> = repo
            .files
            .iter()
            .enumerate()
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
    column: usize,
    target: Option<usize>,
}

impl Reader {
    fn new(line: usize) -> Self {
        Self {
            first: line.saturating_sub(1),
            column: 0,
            target: Some(line),
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
}

pub(super) fn install(app: &mut App, args: &[String]) {
    let arrival = args.iter().any(|arg| arg == "--file").then(|| {
        args.windows(2)
            .find(|pair| pair[0] == "--line")
            .and_then(|pair| pair[1].parse::<usize>().ok())
            .unwrap_or(1)
            .max(1)
    });
    app.insert_resource(Library {
        arrival,
        ..default()
    })
    .init_resource::<room::Unrolling>()
    .init_resource::<city_navigation::Navigation>()
    .add_systems(Startup, room::setup)
    .add_systems(
        Update,
        (
            #[cfg(not(target_arch = "wasm32"))]
            repository_picker::open_repository,
            #[cfg(not(target_arch = "wasm32"))]
            live_worktree::update,
            #[cfg(target_arch = "wasm32")]
            browser_switch,
            progress::restore,
            refresh_repository,
            #[cfg(target_arch = "wasm32")]
            browser_refresh.run_if(repository_picker::has_city),
            controls,
            room::draw,
            room::arrange_cabinets,
            librarian::animate,
            progress::sync.run_if(repository_picker::has_city),
        )
            .chain(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.init_resource::<file_picker::FilePicker>()
        .init_resource::<live_worktree::Live>()
        .add_systems(Update, native_panel::keyboard_navigation)
        .add_systems(
            Update,
            native_panel::focus_outline.after(native_panel::keyboard_navigation),
        );
    #[cfg(target_arch = "wasm32")]
    app.add_systems(First, browser_touch::viewport);
}

fn refresh_repository(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut library: ResMut<Library>,
    mut scope: Local<Option<(PathBuf, String, Option<String>)>>,
    mut camera: Query<&mut Transform, With<room::Camera>>,
) {
    if !repo.is_changed() {
        return;
    }
    let fresh = scope.as_ref().is_none_or(|(root, revision, baseline)| {
        *root != repo.0.root || *revision != repo.0.revision || *baseline != repo.0.baseline
    });
    if fresh {
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
        }
    }
    library.catalog.index(&repo.0);
    library.catalog.collect(&repo.0, Some(explorer.selected));
    if library.reader.is_some() && repo.0.files.is_empty() {
        library.reader = None;
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
    mut typing: MessageReader<KeyboardInput>,
    mut camera: Query<&mut Transform, With<room::Camera>>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] mut menu_find: ResMut<file_picker::FilePicker>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if picker.blocks_controls() {
        typing.clear();
        return;
    }
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    let library = &mut *library;
    let Some(file) = repo.0.files.get(explorer.selected) else {
        typing.clear();
        return;
    };
    if let Some(line) = library.arrival.take() {
        library.walk_pose = Some(*camera);
        library.reader = Some(Reader::new(line));
        progress.visit(explorer.selected, file, false);
    }
    let enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    let escape = keys.just_pressed(KeyCode::Escape);
    if let Some(input) = &mut library.input {
        if escape || enter {
            match library.input.take().expect("active library input") {
                Input::Catalog => {
                    if escape {
                        library.catalog.query.clear();
                        library.catalog.collect(&repo.0, Some(explorer.selected));
                    }
                }
                Input::Line(line) if enter => {
                    if let Ok(line) = line.parse::<usize>()
                        && let Some(reader) = &mut library.reader
                    {
                        reader.first = line
                            .saturating_sub(1)
                            .min(explorer.highlighted.len().saturating_sub(1));
                        reader.target = Some(reader.first + 1);
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
            };
            if edit(value, digits, typing.read()) && !digits {
                library.catalog.collect(&repo.0, None);
            }
        }
    } else {
        typing.clear();
    }

    if let Some(reader) = &mut library.reader {
        if escape {
            library.reader = None;
            if let Some(pose) = library.walk_pose.take() {
                *camera = pose;
            }
            return;
        }
        if library.input.is_some() {
            return;
        }
        let rows = explorer.highlighted.len();
        if keys.just_pressed(KeyCode::KeyJ) {
            library.input = Some(Input::Line(String::new()));
        } else if keys.just_pressed(KeyCode::Home) {
            reader.first = 0;
            reader.target = None;
        } else if keys.just_pressed(KeyCode::End) {
            reader.first = rows.saturating_sub(SCROLL_ROWS);
            reader.target = None;
        } else if keys.just_pressed(KeyCode::PageUp) {
            reader.step(-(SCROLL_ROWS as isize - 1), rows);
        } else if keys.just_pressed(KeyCode::PageDown) {
            reader.step(SCROLL_ROWS as isize - 1, rows);
        } else if keys.just_pressed(KeyCode::ArrowUp) || wheel.delta.y > 0. {
            reader.step(-1, rows);
        } else if keys.just_pressed(KeyCode::ArrowDown) || wheel.delta.y < 0. {
            reader.step(1, rows);
        } else if keys.just_pressed(KeyCode::ArrowLeft) {
            reader.column = reader.column.saturating_sub(12);
        } else if keys.just_pressed(KeyCode::ArrowRight) {
            let max = explorer.highlighted[reader.page(rows)]
                .iter()
                .map(|line| line.text.chars().count())
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
    if library.input.is_none()
        && (keys.just_pressed(KeyCode::Slash) || (command && keys.just_pressed(KeyCode::KeyP)))
    {
        library.catalog.query.clear();
        library.catalog.collect(&repo.0, Some(explorer.selected));
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
    if (enter || (library.input.is_none() && keys.just_pressed(KeyCode::KeyE)))
        && let Some(&index) = library.catalog.files.get(library.catalog.selected)
    {
        explorer.open(&repo.0, index);
        progress.visit(index, &repo.0.files[index], false);
        library.walk_pose = Some(*camera);
        library.input = None;
        library.reader = Some(Reader::new(1));
    } else if library.input.is_none() && !command {
        room::walk(
            &mut camera,
            &keys,
            time.delta_secs(),
            buttons.pressed(MouseButton::Right).then_some(motion.delta),
        );
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
        catalog.collect(&repo, Some(1));
        assert_eq!(catalog.shelf(), &[2, 0, 1]);
        catalog.query = "src/main".into();
        catalog.collect(&repo, None);
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
        assert_eq!(catalog.cabinet("/"), Some((0, "/", 1)));
        assert_eq!(catalog.cabinet("src"), Some((1, "src", 2)));
        assert_eq!(catalog.cabinet("src/ui"), Some((2, "src/ui", 1)));
    }
}
