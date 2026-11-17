//! Pick a local checkout and replace the city only after successfully reading its snapshot.
#[cfg(not(target_arch = "wasm32"))]
use bevy::{
    ecs::system::NonSendMarker,
    input_focus::{FocusCause, InputFocus},
    text::EditableText,
    ui::RelativeCursorPosition,
};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, mpsc};

use bevy::ui_widgets::{Activate, observe};

use super::*;

#[derive(Resource)]
pub(super) struct RepositoryPicker {
    requested: Option<PickerAction>,
    status: String,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<native_github::Job<native_github::Snapshot>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub github: Option<native_github::Target>,
    #[cfg(not(target_arch = "wasm32"))]
    github_file: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    folder: Option<FolderSelection>,
    #[cfg(not(target_arch = "wasm32"))]
    local: Option<native_github::Job<Repository>>,
    #[cfg(not(target_arch = "wasm32"))]
    revisions: Vec<(String, String)>,
    #[cfg(not(target_arch = "wasm32"))]
    revision_job: Option<native_github::Job<Vec<(String, String)>>>,
    #[cfg(not(target_arch = "wasm32"))]
    session: Option<native_github::Session>,
    #[cfg(not(target_arch = "wasm32"))]
    device: Option<native_github::Job<native_github::DeviceCode>>,
    #[cfg(not(target_arch = "wasm32"))]
    login: Option<native_github::Job<native_github::Session>>,
    #[cfg(not(target_arch = "wasm32"))]
    view: Option<RepositoryView>,
    #[cfg(not(target_arch = "wasm32"))]
    action: Option<PanelAction>,
    #[cfg(not(target_arch = "wasm32"))]
    blocked: bool,
    #[cfg(not(target_arch = "wasm32"))]
    sign_in_code: Option<String>,
}

#[derive(Clone, Copy)]
enum PickerAction {
    Folder,
    Revision,
    #[cfg(not(target_arch = "wasm32"))]
    GitHub,
}

#[derive(Component)]
pub(super) struct RepositoryStatus;

impl RepositoryPicker {
    pub(super) fn new(status: String) -> Self {
        Self {
            requested: None,
            status,
            #[cfg(not(target_arch = "wasm32"))]
            pending: None,
            #[cfg(not(target_arch = "wasm32"))]
            github: None,
            #[cfg(not(target_arch = "wasm32"))]
            github_file: None,
            #[cfg(not(target_arch = "wasm32"))]
            folder: None,
            #[cfg(not(target_arch = "wasm32"))]
            local: None,
            #[cfg(not(target_arch = "wasm32"))]
            revisions: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            revision_job: None,
            #[cfg(not(target_arch = "wasm32"))]
            session: None,
            #[cfg(not(target_arch = "wasm32"))]
            device: None,
            #[cfg(not(target_arch = "wasm32"))]
            login: None,
            #[cfg(not(target_arch = "wasm32"))]
            view: None,
            #[cfg(not(target_arch = "wasm32"))]
            action: None,
            #[cfg(not(target_arch = "wasm32"))]
            blocked: false,
            #[cfg(not(target_arch = "wasm32"))]
            sign_in_code: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn blocks_controls(&self) -> bool {
        self.blocked
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn publication(&self) -> Option<(native_github::Target, String)> {
        Some((
            self.github.as_ref()?.clone(),
            native_github::token(self.session.as_ref())?,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_github(&mut self, input: &str, file: Option<String>) {
        match native_github::Target::parse(input) {
            Ok(target) => {
                self.status = format!("Opening {}…", target.label());
                self.pending = Some(native_github::open(
                    target,
                    native_github::token(self.session.as_ref()),
                ));
                self.github_file = file;
            }
            Err(error) => self.status = error,
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
    #[cfg(not(target_arch = "wasm32"))]
    let opened = if revision == "WORKTREE" {
        Repository::open_worktree(path)
    } else {
        Repository::open_with_base(path, revision, base)
    };
    #[cfg(target_arch = "wasm32")]
    let opened = Repository::open_with_base(path, revision, base);
    let repo = opened.map_err(|error| format!("Could not open {}\n{error}", path.display()))?;
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

pub(super) fn settings_file() -> Option<PathBuf> {
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

#[cfg(not(target_arch = "wasm32"))]
type FolderSelection = Mutex<mpsc::Receiver<Option<PathBuf>>>;

#[cfg(target_os = "macos")]
fn choose_folder(current: &Path) -> Result<FolderSelection, String> {
    use block2::RcBlock;
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
    let (send, receive) = mpsc::sync_channel(1);
    let opened = panel.clone();
    panel.beginWithCompletionHandler(&RcBlock::new(move |response| {
        let selected = if response == NSModalResponseOK {
            opened.URL().and_then(|url| url.to_file_path())
        } else {
            None
        };
        let _ = send.try_send(selected);
    }));
    Ok(Mutex::new(receive))
}

#[cfg(not(any(target_os = "macos", target_arch = "wasm32")))]
fn choose_folder(_: &Path) -> Result<FolderSelection, String> {
    Err("Launch with --repo PATH to choose a repository on this platform.".into())
}

#[cfg(not(target_arch = "wasm32"))]
enum RepositoryView {
    GitHub {
        panel: Entity,
        input: Entity,
    },
    Revisions {
        panel: Entity,
        input: Entity,
        choices: Entity,
    },
}

#[cfg(not(target_arch = "wasm32"))]
impl RepositoryView {
    fn panel(&self) -> Entity {
        match self {
            Self::GitHub { panel, .. } | Self::Revisions { panel, .. } => *panel,
        }
    }
    fn input(&self) -> Entity {
        match self {
            Self::GitHub { input, .. } | Self::Revisions { input, .. } => *input,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Component)]
enum PanelAction {
    Close,
    Open,
    SignIn,
    Browser,
    Live,
    Revision(usize),
}

#[cfg(not(target_arch = "wasm32"))]
impl native_panel::Actions<PanelAction> for RepositoryPicker {
    fn activate(&mut self, action: PanelAction) {
        self.action = Some(action);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
pub(super) struct GitHubStatus;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
pub(super) struct RevisionList;

#[cfg(not(target_arch = "wasm32"))]
fn draw_panel(commands: &mut Commands, github: bool, default_value: &str) -> RepositoryView {
    use native_panel::{INK, button, input, panel};
    let panel = panel(
        commands,
        if github {
            "Open from GitHub"
        } else {
            "Revisions"
        },
        495.,
    );
    let mut field = None;
    let mut choices = None;
    commands.entity(panel).with_children(|parent| {
        if github {
            field = Some(input(
                parent,
                "Repository or pull request URL",
                default_value,
                0,
                false,
            ));
            parent.spawn((
                GitHubStatus,
                Text::new(""),
                TextFont::from_font_size(14.),
                TextColor(INK),
            ));
        } else {
            field = Some(input(
                parent,
                "Branch, tag, or commit",
                default_value,
                0,
                false,
            ));
            choices = Some(
                parent
                    .spawn((
                        RevisionList,
                        RelativeCursorPosition::default(),
                        Node {
                            height: px(225.),
                            overflow: Overflow::scroll_y(),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3.),
                            ..default()
                        },
                    ))
                    .id(),
            );
        }
        parent
            .spawn(Node {
                column_gap: px(6.),
                flex_wrap: FlexWrap::Wrap,
                row_gap: px(6.),
                ..default()
            })
            .with_children(|buttons| {
                button::<_, RepositoryPicker>(buttons, "Open", INK, PanelAction::Open);
                if github {
                    button::<_, RepositoryPicker>(buttons, "Sign in", INK, PanelAction::SignIn);
                    button::<_, RepositoryPicker>(
                        buttons,
                        "Enter code on GitHub",
                        INK,
                        PanelAction::Browser,
                    );
                } else {
                    button::<_, RepositoryPicker>(buttons, "Live worktree", INK, PanelAction::Live);
                }
                button::<_, RepositoryPicker>(buttons, "Close", INK, PanelAction::Close);
            });
    });
    let input = field.expect("repository input");
    if github {
        RepositoryView::GitHub { panel, input }
    } else {
        RepositoryView::Revisions {
            panel,
            input,
            choices: choices.expect("revision list"),
        }
    }
}

pub(super) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            top: px(77.),
            column_gap: px(8.),
            ..default()
        })
        .with_children(|ui| {
            ui.spawn((
                file_picker::button("Open Repository…", font, ink, AlignSelf::Auto),
                observe(|_: On<Activate>, mut picker: ResMut<RepositoryPicker>| {
                    picker.requested = Some(PickerAction::Folder)
                }),
            ));
            ui.spawn((
                file_picker::button("Revisions…", font, ink, AlignSelf::Auto),
                observe(|_: On<Activate>, mut picker: ResMut<RepositoryPicker>| {
                    picker.requested = Some(PickerAction::Revision)
                }),
            ));
            #[cfg(not(target_arch = "wasm32"))]
            ui.spawn((
                file_picker::button("GitHub…", font, ink, AlignSelf::Auto),
                observe(|_: On<Activate>, mut picker: ResMut<RepositoryPicker>| {
                    picker.requested = Some(PickerAction::GitHub)
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
    (Entity, Has<CityObject>, Option<&'static FileBuilding>),
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
    if !next.files.is_empty() {
        explorer.open(&next, explorer.selected);
    }
    replace_selection(commands, old_city, next, explorer, None);
    commands.insert_resource(listing::Listing::default());
}

pub(super) fn replace_selection(
    commands: &mut Commands,
    old_city: &CityEntities,
    next: Repository,
    explorer: Explorer,
    refreshed_files: Option<&HashSet<usize>>,
) {
    for (entity, city_object, building) in old_city {
        if refreshed_files.is_none_or(|files| {
            !city_object || building.is_some_and(|building| files.contains(&building.0))
        }) {
            commands.entity(entity).despawn();
        }
    }
    commands.insert_resource(Repo(next));
    commands.insert_resource(explorer);
    commands.insert_resource(city_navigation::Navigation::default());
    commands.insert_resource(file_picker::FilePicker::default());
    commands.insert_resource(Nearest::default());
    commands.insert_resource(city_menu::Menu::default());
}

#[cfg(not(target_arch = "wasm32"))]
fn close_view(
    commands: &mut Commands,
    picker: &mut RepositoryPicker,
    focus: &mut InputFocus,
    parents: &Query<&ChildOf>,
) {
    if let Some(view) = picker.view.take() {
        native_panel::close(commands, view.panel(), focus, parents);
    }
    picker.revision_job = None;
    picker.revisions.clear();
}

#[cfg(not(target_arch = "wasm32"))]
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
    live: Res<live_worktree::Live>,
    mut focus: ResMut<InputFocus>,
    (parents, inputs, mut revision_lists, scroll): (
        Query<&ChildOf>,
        Query<&EditableText>,
        Query<(&mut ScrollPosition, &RelativeCursorPosition), With<RevisionList>>,
        Res<AccumulatedMouseScroll>,
    ),
    mut github_status: Query<&mut Text, (With<GitHubStatus>, Without<RepositoryStatus>)>,
    review: Res<review_notes::Review>,
) {
    let Ok((mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let shortcut = keys.just_pressed(KeyCode::KeyO)
        && keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let was_blocked = picker.view.is_some() || picker.folder.is_some();
    let mut opened = false;
    if let Ok((mut position, hover)) = revision_lists.single_mut()
        && hover.cursor_over()
        && scroll.delta.y != 0.
    {
        position.y = (position.y - scroll.delta.y * 12.).max(0.);
    }
    if let Some(result) = picker
        .revision_job
        .as_ref()
        .and_then(native_github::Job::take)
    {
        picker.revision_job = None;
        if let Some(RepositoryView::Revisions { choices, .. }) = picker.view {
            match result {
                Ok(revisions) => {
                    commands.entity(choices).with_children(|list| {
                        for (index, (_, label)) in revisions.iter().enumerate() {
                            native_panel::button::<_, RepositoryPicker>(
                                list,
                                label,
                                native_panel::INK,
                                PanelAction::Revision(index),
                            );
                        }
                    });
                    picker.revisions = revisions;
                }
                Err(error) => picker.status = error,
            }
        }
    }
    if let Some(result) = picker.pending.as_ref().and_then(native_github::Job::take) {
        picker.pending = None;
        match result {
            Ok(snapshot) => {
                let next = snapshot.repository;
                let selected = picker
                    .github_file
                    .take()
                    .and_then(|path| next.files.iter().position(|file| file.path == path))
                    .unwrap_or_else(|| next.initial_file());
                let mut explorer = Explorer::new(selected);
                explorer.open(&next, selected);
                window.title = format!("{} · {}", set.name, snapshot.target.label());
                picker.github = Some(snapshot.target);
                picker.status.clear();
                replace_selection(&mut commands, &old_city, next, explorer, None);
                commands.insert_resource(listing::Listing::default());
                opened = true;
            }
            Err(error) => picker.status = error,
        }
    }
    if let Some(result) = picker.local.as_ref().and_then(native_github::Job::take) {
        picker.local = None;
        match result {
            Ok(next) => {
                picker.github = None;
                window.title = window_title(&set, &next);
                picker.status = remember(&next.root).err().unwrap_or_default();
                replace_city(&mut commands, &old_city, next);
                opened = true;
            }
            Err(error) => picker.status = error,
        }
    }
    if let Some(result) = picker.folder.as_ref().and_then(|receive| {
        match receive.lock().expect("folder picker receiver").try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
        }
    }) {
        picker.folder = None;
        if let Some(path) = result {
            picker.status = "Reading repository…".into();
            picker.local = Some(native_github::Job::spawn(move || load(&path, "HEAD", None)));
        }
    }
    if let Some(result) = picker.device.as_ref().and_then(native_github::Job::take) {
        picker.device = None;
        match result {
            Ok(code) => {
                picker.sign_in_code = Some(code.user_code.clone());
                picker.login = Some(native_github::finish_sign_in(code));
            }
            Err(error) => picker.status = error,
        }
    }
    if let Some(result) = picker.login.as_ref().and_then(native_github::Job::take) {
        picker.login = None;
        picker.sign_in_code = None;
        match result {
            Ok(session) => {
                picker.session = Some(session);
                picker.status.clear();
            }
            Err(error) => picker.status = error,
        }
    }
    let panel_action = picker.action.take().or_else(|| {
        if picker.view.is_none() {
            None
        } else if keys.just_pressed(KeyCode::Escape) {
            Some(PanelAction::Close)
        } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
            Some(PanelAction::Open)
        } else {
            None
        }
    });
    if let Some(action) = panel_action
        && let Some(view) = picker.view.as_ref()
    {
        let github = matches!(view, RepositoryView::GitHub { .. });
        let input = inputs
            .get(view.input())
            .map(|input| input.value().to_string())
            .unwrap_or_default();
        match action {
            PanelAction::SignIn if github && picker.device.is_none() && picker.login.is_none() => {
                picker.status = "Requesting a GitHub sign-in code…".into();
                picker.device = Some(native_github::request_sign_in());
            }
            PanelAction::Browser if github => {
                if picker.sign_in_code.is_some()
                    && let Err(error) = native_github::browser_sign_in()
                {
                    picker.status = error;
                }
            }
            PanelAction::Open if github => {
                picker.open_github(&input, None);
                if picker.pending.is_some() {
                    close_view(&mut commands, &mut picker, &mut focus, &parents);
                }
            }
            PanelAction::Open | PanelAction::Live | PanelAction::Revision(_) if !github => {
                let path = repo.0.root.clone();
                let revision = match action {
                    PanelAction::Revision(index) => picker.revisions[index].0.clone(),
                    PanelAction::Live => "WORKTREE".into(),
                    _ if input.trim().is_empty() => "HEAD".into(),
                    _ => input.trim().to_owned(),
                };
                picker.status = "Reading revision…".into();
                picker.local = Some(native_github::Job::spawn(move || {
                    load(&path, &revision, None)
                }));
                close_view(&mut commands, &mut picker, &mut focus, &parents);
            }
            PanelAction::Close => close_view(&mut commands, &mut picker, &mut focus, &parents),
            _ => {}
        }
    }
    let action = picker
        .requested
        .take()
        .or((window.focused && shortcut).then_some(PickerAction::Folder));
    if !opened
        && picker.folder.is_none()
        && picker.view.is_none()
        && picker.pending.is_none()
        && picker.local.is_none()
        && !review.blocked
        && let Some(action) = action
    {
        if let Ok(mut pilot) = pilot.single_mut() {
            pilot.captured = false;
            pilot.firing = false;
            pilot.shot = false;
            pilot.fire_ready = false;
        }
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        picker.status.clear();
        match action {
            PickerAction::Folder => match choose_folder(&repo.0.root) {
                Ok(folder) => picker.folder = Some(folder),
                Err(error) => picker.status = error,
            },
            PickerAction::Revision if repo.0.files.is_empty() || picker.github.is_some() => {
                picker.status = "Open a local repository to choose a local revision.".into();
            }
            PickerAction::GitHub | PickerAction::Revision => {
                let github = matches!(action, PickerAction::GitHub);
                let current = if github {
                    picker
                        .github
                        .as_ref()
                        .map_or_else(String::new, native_github::Target::url)
                } else {
                    "HEAD".into()
                };
                let view = draw_panel(&mut commands, github, &current);
                focus.set(view.input(), FocusCause::Navigated);
                picker.view = Some(view);
                if !github {
                    let path = repo.0.root.clone();
                    picker.revision_job = Some(native_github::Job::spawn(move || {
                        repo_world::local_revisions(&path)
                    }));
                }
            }
        }
    }
    if let Ok(mut text) = github_status.single_mut() {
        let message = if let Some(code) = &picker.sign_in_code {
            format!("Enter this code on GitHub: {code}")
        } else if !picker.status.is_empty() {
            picker.status.clone()
        } else if let Some(session) = &picker.session {
            format!("Signed in as @{}", session.login)
        } else if native_github::development_token().is_some() {
            "Using the token from your local development environment.".into()
        } else {
            "Sign in for private repositories and review comments.".into()
        };
        text.set_if_neq(Text::new(message));
    }
    picker.blocked = was_blocked || picker.view.is_some() || picker.folder.is_some();
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
        let message = if picker.view.is_none() && !picker.status.is_empty() {
            picker.status.as_str()
        } else if repo.0.files.is_empty() && !opened {
            "Choose a local Git repository to explore."
        } else {
            ""
        };
        #[cfg(not(target_arch = "wasm32"))]
        let message = if message.is_empty() && repo.0.is_worktree() {
            live.status()
        } else {
            message
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
