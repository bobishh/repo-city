//! Pick a local checkout and replace the city only after successfully reading its snapshot.
use std::path::Path;

use bevy::{
    ecs::system::NonSendMarker,
    ui_widgets::{Activate, observe},
};

use super::*;

#[derive(Resource)]
pub(super) struct RepositoryPicker {
    requested: bool,
    status: String,
}

#[derive(Component)]
pub(super) struct RepositoryStatus;

impl RepositoryPicker {
    pub(super) fn new(status: String) -> Self {
        Self {
            requested: false,
            status,
        }
    }
}

pub(super) fn empty() -> Repository {
    Repository {
        root: PathBuf::new(),
        revision: String::new(),
        baseline: None,
        files: vec![],
        districts: vec![],
    }
}

pub(super) fn load(path: &Path, revision: &str, base: Option<&str>) -> Result<Repository, String> {
    let repo = Repository::open_with_base(path, revision, base)
        .map_err(|error| format!("Could not open {}\n{error}", path.display()))?;
    if repo.files.is_empty() {
        return Err(format!("{} has no committed files.", repo.root.display()));
    }
    Ok(repo)
}

pub(super) fn window_title(set: &WorldSet, repo: &Repository) -> String {
    match repo.root.file_name() {
        Some(name) => format!("{} · {}", set.name, name.to_string_lossy()),
        None => "Repo City".into(),
    }
}

pub(super) fn has_city(repo: Res<Repo>) -> bool {
    !repo.0.files.is_empty()
}

fn settings_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut directory = PathBuf::from(home);
    if cfg!(target_os = "macos") {
        directory.push("Library/Application Support");
    } else {
        directory = std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| directory.join(".config"), PathBuf::from);
    }
    Some(directory.join("Repo City/last-repository.json"))
}

pub(super) fn last_folder() -> Option<PathBuf> {
    serde_json::from_slice(&std::fs::read(settings_file()?).ok()?).ok()
}

fn remember(path: &Path) -> Result<(), String> {
    let file = settings_file().ok_or("Could not locate the settings folder.")?;
    let bytes = serde_json::to_vec(path).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(file.parent().expect("settings directory"))
        .and_then(|()| {
            let temporary = file.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&temporary, bytes)?;
            if let Err(error) = std::fs::rename(&temporary, &file) {
                let _ = std::fs::remove_file(temporary);
                return Err(error);
            }
            Ok(())
        })
        .map_err(|error| format!("Could not remember the repository: {error}"))
}

#[cfg(target_os = "macos")]
fn choose_folder(current: &Path) -> Result<Option<PathBuf>, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};

    let main = MainThreadMarker::new().ok_or("Folder picker needs the macOS main thread.")?;
    let panel = NSOpenPanel::openPanel(main);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setPrompt(Some(&NSString::from_str("Open Repository")));
    if let Some(folder) = current.parent().and_then(NSURL::from_file_path) {
        panel.setDirectoryURL(Some(&folder));
    }
    if panel.runModal() == NSModalResponseOK {
        return panel
            .URL()
            .and_then(|url| url.to_file_path())
            .map(Some)
            .ok_or_else(|| "Could not read the selected folder path.".into());
    }
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn choose_folder(_: &Path) -> Result<Option<PathBuf>, String> {
    Err("Launch with --repo PATH to choose a repository on this platform.".into())
}

pub(super) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            top: px(77.),
            ..default()
        })
        .with_children(|ui| {
            ui.spawn((
                file_picker::button("Open Repository…", font, ink, AlignSelf::Auto),
                observe(|_: On<Activate>, mut picker: ResMut<RepositoryPicker>| {
                    picker.requested = true
                }),
            ));
        });
    commands.spawn((
        RepositoryStatus,
        Text::new(""),
        font.clone(),
        TextColor(ink),
        BackgroundColor(Color::srgba(0.02, 0.04, 0.06, 0.96)),
        Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            top: px(118.),
            max_width: px(470.),
            display: Display::None,
            padding: UiRect::all(px(10.)),
            ..default()
        },
    ));
}

pub(super) type CityEntities<'w, 's> = Query<
    'w,
    's,
    Entity,
    Or<(
        With<CityObject>,
        With<Detail>,
        With<listing::ListingSurface>,
        With<city_navigation::RouteVisual>,
        With<physics::Lifetime>,
    )>,
>;

pub(super) fn replace_city(commands: &mut Commands, old_city: &CityEntities, next: Repository) {
    let mut explorer = Explorer::new(next.initial_file());
    explorer.open(&next, explorer.selected);
    for entity in old_city {
        commands.entity(entity).despawn();
    }
    commands.insert_resource(Repo(next));
    commands.insert_resource(explorer);
    commands.insert_resource(city_navigation::Navigation::default());
    commands.insert_resource(listing::Listing::default());
    commands.insert_resource(file_picker::FilePicker::default());
    commands.insert_resource(Nearest::default());
}

pub(super) fn open_repository(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut picker: ResMut<RepositoryPicker>,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut pilot: Query<&mut Pilot>,
    old_city: CityEntities,
    mut status: Query<(&mut Text, &mut Node), With<RepositoryStatus>>,
    mut file_controls: Query<
        &mut Node,
        (With<file_picker::FileControls>, Without<RepositoryStatus>),
    >,
    _main_thread: NonSendMarker,
) {
    let Ok((mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let shortcut = keys.just_pressed(KeyCode::KeyO)
        && keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let mut opened = false;
    if std::mem::take(&mut picker.requested) || (window.focused && shortcut) {
        if let Ok(mut pilot) = pilot.single_mut() {
            pilot.captured = false;
        }
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        match choose_folder(&repo.0.root)
            .and_then(|path| path.map(|path| load(&path, "HEAD", None)).transpose())
        {
            Ok(Some(next)) => {
                window.title = window_title(&set, &next);
                picker.status = remember(&next.root).err().unwrap_or_default();
                replace_city(&mut commands, &old_city, next);
                opened = true;
            }
            Ok(None) => {}
            Err(error) => picker.status = error,
        }
    }
    let has_city = !repo.0.files.is_empty() || opened;
    for mut node in &mut file_controls {
        let display = if has_city {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    if let Ok((mut text, mut node)) = status.single_mut() {
        let message = if !picker.status.is_empty() {
            picker.status.as_str()
        } else if repo.0.files.is_empty() && !opened {
            "Choose a local Git repository to explore."
        } else {
            ""
        };
        if text.0 != message {
            text.0 = message.into();
        }
        let display = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_folder_is_rejected_with_its_path() {
        let path = std::env::temp_dir().join(format!("repo-city-missing-{}", std::process::id()));
        let result = load(&path, "HEAD", None).unwrap_err();
        assert!(result.contains(&path.display().to_string()));
    }
}
