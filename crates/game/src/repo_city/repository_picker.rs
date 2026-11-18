//! Pick a local checkout and replace the city only after successfully reading its snapshot.
#[cfg(not(target_arch = "wasm32"))]
use bevy::{
    clipboard::Clipboard,
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
#[cfg(not(target_arch = "wasm32"))]
use github_choices::Editor;

#[derive(Resource)]
pub(super) struct RepositoryPicker {
    requested: Option<PickerAction>,
    status: String,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<native_github::Job<native_github::Snapshot>>,
    #[cfg(not(target_arch = "wasm32"))]
    comments: Option<native_github::Job<Vec<review_notes::Note>>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub github: Option<native_github::Target>,
    #[cfg(not(target_arch = "wasm32"))]
    pull_open: bool,
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
    restoring: Option<native_github::Job<Option<native_github::Session>>>,
    #[cfg(not(target_arch = "wasm32"))]
    queued_github: Option<(native_github::Target, String, String, Option<String>)>,
    #[cfg(not(target_arch = "wasm32"))]
    auth_error: String,
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
    #[cfg(not(target_arch = "wasm32"))]
    code_copied: bool,
    #[cfg(not(target_arch = "wasm32"))]
    choices: github_choices::Picker,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(PartialEq, Eq)]
pub(super) enum AuthPrompt {
    SignIn,
    Copy(String),
    Message(String),
}

#[derive(Clone, Copy)]
enum PickerAction {
    Folder,
    Revision,
    #[cfg(not(target_arch = "wasm32"))]
    Menu,
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
            comments: None,
            #[cfg(not(target_arch = "wasm32"))]
            github: None,
            #[cfg(not(target_arch = "wasm32"))]
            pull_open: false,
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
            restoring: None,
            #[cfg(not(target_arch = "wasm32"))]
            queued_github: None,
            #[cfg(not(target_arch = "wasm32"))]
            auth_error: String::new(),
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
            #[cfg(not(target_arch = "wasm32"))]
            code_copied: false,
            #[cfg(not(target_arch = "wasm32"))]
            choices: github_choices::Picker::default(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn blocks_controls(&self) -> bool {
        self.blocked
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn smoke_open(&mut self) {
        self.requested = Some(PickerAction::GitHub);
        self.choices.sample_library();
        self.sign_in_code = Some("ABCD-1234".into());
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn smoke_review(&mut self) {
        self.github =
            native_github::Target::parse("https://github.com/example/repo-city/pull/1").ok();
        self.pull_open = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn restore_session(&mut self) {
        self.restoring = Some(native_github::restore_session());
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn request_sign_in(&mut self) {
        if native_github::token(self.session.as_ref()).is_none()
            && self.device.is_none()
            && self.login.is_none()
            && self.restoring.is_none()
        {
            self.auth_error.clear();
            self.device = Some(native_github::request_sign_in());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn copy_code(&mut self, clipboard: &mut Clipboard) {
        if let Some(code) = &self.sign_in_code {
            match clipboard.set_text(code.as_str()) {
                Ok(()) => self.code_copied = true,
                Err(_) => {
                    self.auth_error = "Couldn't copy the code. Type it in the GitHub tab.".into()
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn auth_prompt(&self) -> AuthPrompt {
        if let Some(code) = &self.sign_in_code {
            let feedback = if self.code_copied {
                "Copied"
            } else {
                "Click to copy"
            };
            AuthPrompt::Copy(format!("{code} | {feedback}"))
        } else if let Some(session) = self.session.as_ref().filter(|session| session.valid()) {
            AuthPrompt::Message(format!("@{}", session.login))
        } else if self.restoring.is_some() {
            AuthPrompt::Message("Restoring GitHub session…".into())
        } else if self.device.is_some() {
            AuthPrompt::Message("Requesting a code…".into())
        } else {
            AuthPrompt::SignIn
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn auth_message(&self) -> &str {
        if !self.auth_error.is_empty() {
            &self.auth_error
        } else if self.sign_in_code.is_some() {
            "Paste the code in the GitHub tab."
        } else {
            ""
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn invalidate_session(&mut self) {
        self.session = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn can_publish(&self) -> bool {
        self.pull_open && self.github.is_some()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn publication(&self) -> Option<(native_github::Target, String)> {
        if !self.can_publish() {
            return None;
        }
        Some((
            self.github.as_ref()?.clone(),
            native_github::token(self.session.as_ref())?,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn compare_commits(&mut self, root: PathBuf, before: String, after: String) {
        self.status = "Loading selected commits…".into();
        self.local = Some(native_github::Job::spawn(move || {
            load(&root, &after, Some(&before))
        }));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_github(
        &mut self,
        input: &str,
        revision: &str,
        base: &str,
        file: Option<String>,
    ) {
        match native_github::Target::parse(input) {
            Ok(target) => self.start_github(target, revision.into(), base.into(), file),
            Err(error) => self.status = error,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn start_github(
        &mut self,
        target: native_github::Target,
        revision: String,
        base: String,
        file: Option<String>,
    ) {
        self.status = format!("Opening {}…", target.label());
        if self.restoring.is_some() {
            self.queued_github = Some((target, revision, base, file));
            return;
        }
        self.pending = Some(native_github::open(
            target,
            native_github::token(self.session.as_ref()),
            revision,
            base,
        ));
        self.github_file = file;
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
    let name = if cfg!(target_arch = "wasm32") {
        set.name.as_str()
    } else {
        "Vibe City"
    };
    match repo.root.file_name() {
        Some(repository) => format!("{name} · {}", repository.to_string_lossy()),
        None => name.into(),
    }
}

pub(super) fn has_city(repo: Res<Repo>) -> bool {
    !repo.0.files.is_empty()
}

pub(super) fn settings_file() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("VIBE_CITY_CONFIG_DIR") {
        return Some(PathBuf::from(directory).join("last-repository.json"));
    }
    settings_directory(std::env::consts::OS, |name| std::env::var_os(name))
        .map(|directory| directory.join("last-repository.json"))
}

fn settings_directory(
    platform: &str,
    get: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let directory = match platform {
        "windows" => get("APPDATA")
            .map(PathBuf::from)
            .or_else(|| get("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Roaming"))),
        "macos" => get("HOME").map(|home| PathBuf::from(home).join("Library/Application Support")),
        _ => get("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| get("HOME").map(|home| PathBuf::from(home).join(".config"))),
    }?;
    Some(directory.join("Repo City"))
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

#[cfg(not(target_arch = "wasm32"))]
enum RepositoryView {
    Menu {
        panel: Entity,
    },
    Folder {
        panel: Entity,
        input: Entity,
        status: Entity,
    },
    GitHub {
        panel: Entity,
        input: Option<Entity>,
        results: Option<Entity>,
        status: Entity,
        auth: Entity,
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
            Self::GitHub { panel, .. }
            | Self::Revisions { panel, .. }
            | Self::Folder { panel, .. }
            | Self::Menu { panel } => *panel,
        }
    }
    fn input(&self) -> Option<Entity> {
        match self {
            Self::GitHub { input, .. } => *input,
            Self::Menu { .. } => None,
            Self::Revisions { input, .. } | Self::Folder { input, .. } => Some(*input),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Component)]
enum PanelAction {
    Close,
    Choose(PickerAction),
    Open,
    SignIn,
    CopyCode,
    SelectRepository,
    ChangeRepository,
    EditGithub(Editor),
    UseRevision,
    GitHubMatch(usize),
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
pub(super) struct RevisionList;

#[cfg(not(target_arch = "wasm32"))]
fn draw_folder(commands: &mut Commands, current: &Path) -> RepositoryView {
    use native_panel::{INK, button, input, panel};
    let panel = panel(commands, "Open local repository", 495.);
    let mut field = None;
    commands.entity(panel).with_children(|parent| {
        let input = input(
            parent,
            "Local Git repository path",
            &current.to_string_lossy(),
            0,
            false,
        );
        let status = parent
            .spawn((
                Text::new("Enter the checkout directory. Git must be on PATH."),
                native_panel::panel_font(),
                TextColor(INK),
                Node {
                    min_height: px(44.),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        field = Some((input, status));
        parent.spawn(github_row()).with_children(|row| {
            button::<_, RepositoryPicker>(row, "Open", INK, PanelAction::Open);
        });
    });
    let (input, status) = field.expect("folder input");
    RepositoryView::Folder {
        panel,
        input,
        status,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_revisions(commands: &mut Commands) -> RepositoryView {
    use native_panel::{INK, button, input, panel};
    let panel = panel(commands, "Revisions", 495.);
    let mut fields = None;
    commands.entity(panel).with_children(|parent| {
        let input = input(parent, "Branch, tag, or commit", "HEAD", 0, false);
        let choices = parent
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
            .id();
        fields = Some((input, choices));
        parent.spawn(github_row()).with_children(|row| {
            button::<_, RepositoryPicker>(row, "Open", INK, PanelAction::Open);
            button::<_, RepositoryPicker>(row, "Live worktree", INK, PanelAction::Live);
            button::<_, RepositoryPicker>(row, "Close", INK, PanelAction::Close);
        });
    });
    let (input, choices) = fields.expect("revision fields");
    RepositoryView::Revisions {
        panel,
        input,
        choices,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn github_row() -> Node {
    Node {
        column_gap: px(6.),
        row_gap: px(6.),
        flex_wrap: FlexWrap::Wrap,
        ..default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn revision_label<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        &value[..7]
    } else {
        value
            .trim_start_matches("refs/heads/")
            .trim_start_matches("refs/tags/")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_github(commands: &mut Commands, choices: &github_choices::Picker) -> RepositoryView {
    use native_panel::{INK, button, input, panel, text};
    let panel = panel(commands, "Open from GitHub", 555.);
    let mut fields = None;
    commands.entity(panel).with_children(|parent| {
        if let Some(target) = &choices.target {
            parent.spawn(github_row()).with_children(|row| {
                text(row, &target.label(), INK);
                button::<_, RepositoryPicker>(row, "Change", INK, PanelAction::ChangeRepository);
            });
        }
        let field = match choices.editor {
            Some(Editor::PullRequests) => Some(input(
                parent,
                "Find a pull request by number or title",
                "",
                0,
                false,
            )),
            Some(Editor::Revision | Editor::Base) => {
                Some(input(parent, "Find a branch, tag, or commit", "", 0, false))
            }
            None if choices.target.is_none() => Some(input(
                parent,
                "Find a repository or paste a PR URL",
                "",
                0,
                false,
            )),
            None => None,
        };
        let results = field.map(|_| {
            parent
                .spawn((
                    RevisionList,
                    RelativeCursorPosition::default(),
                    Node {
                        height: px(160.),
                        overflow: Overflow::scroll_y(),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.),
                        ..default()
                    },
                ))
                .id()
        });
        parent.spawn(github_row()).with_children(|row| {
            if let Some(target) = &choices.target {
                if !target.is_pull() {
                    button::<_, RepositoryPicker>(
                        row,
                        revision_label(&choices.revision, "Default branch"),
                        INK,
                        PanelAction::EditGithub(Editor::Revision),
                    );
                    button::<_, RepositoryPicker>(
                        row,
                        revision_label(&choices.base, "Compare…"),
                        INK,
                        PanelAction::EditGithub(Editor::Base),
                    );
                    button::<_, RepositoryPicker>(
                        row,
                        "Pull requests",
                        INK,
                        PanelAction::EditGithub(Editor::PullRequests),
                    );
                    if matches!(choices.editor, Some(Editor::Revision | Editor::Base)) {
                        button::<_, RepositoryPicker>(
                            row,
                            "Use revision",
                            INK,
                            PanelAction::UseRevision,
                        );
                    }
                }
                button::<_, RepositoryPicker>(row, "Open city", INK, PanelAction::Open);
            } else {
                button::<_, RepositoryPicker>(
                    row,
                    "Select repository",
                    INK,
                    PanelAction::SelectRepository,
                );
            }
            button::<_, RepositoryPicker>(row, "Close", INK, PanelAction::Close);
        });
        let status = parent
            .spawn((
                Text::new(""),
                TextFont::from_font_size(14.),
                TextColor(INK),
                Node {
                    min_height: px(32.),
                    ..default()
                },
            ))
            .id();
        let auth = parent
            .spawn(Node {
                min_height: px(29.),
                ..github_row()
            })
            .id();
        fields = Some((field, results, status, auth));
    });
    let (input, results, status, auth) = fields.expect("GitHub fields");
    RepositoryView::GitHub {
        panel,
        input,
        results,
        status,
        auth,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(super) struct GitHubRender {
    auth: Option<(Entity, String)>,
    status: Option<(Entity, String)>,
    results: Option<(Entity, Vec<github_choices::Choice>, Option<usize>, String)>,
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_github(
    commands: &mut Commands,
    picker: &mut RepositoryPicker,
    inputs: &Query<&EditableText>,
    render: &mut GitHubRender,
) {
    use native_panel::{INK, button, text};
    let Some(RepositoryView::GitHub {
        input,
        results,
        status,
        auth,
        ..
    }) = &picker.view
    else {
        return;
    };
    let (input, results, status, auth) = (*input, *results, *status, *auth);
    let input = input
        .and_then(|input| inputs.get(input).ok())
        .map(|input| input.value().to_string())
        .unwrap_or_default();
    let (rows, hint) = picker
        .choices
        .results(&input, || native_github::token(picker.session.as_ref()));
    if picker.choices.rows != rows {
        picker.choices.active = None;
        picker.choices.rows = rows;
    }
    if let Some(results) = results {
        let key = (
            results,
            picker.choices.rows.clone(),
            picker.choices.active,
            hint,
        );
        if render.results.as_ref() != Some(&key) {
            let mut list = commands.entity(results);
            list.despawn_related::<Children>();
            list.with_children(|children| {
                for (index, option) in key.1.iter().enumerate() {
                    if Some(index) == key.2 {
                        button::<_, RepositoryPicker>(
                            children,
                            &format!("> {}", option.label),
                            INK,
                            PanelAction::GitHubMatch(index),
                        );
                    } else {
                        button::<_, RepositoryPicker>(
                            children,
                            &option.label,
                            INK,
                            PanelAction::GitHubMatch(index),
                        );
                    }
                }
                if !key.3.is_empty() {
                    text(children, &key.3, INK);
                }
            });
            if let Some(index) = key.2 {
                commands.entity(results).insert(ScrollPosition(Vec2::new(
                    0.,
                    index.saturating_sub(3) as f32 * 33.,
                )));
            } else if !key.1.is_empty() {
                commands.entity(results).insert(ScrollPosition::default());
            }
            render.results = Some(key);
        }
    }
    let auth_prompt = picker.auth_prompt();
    let caption = match &auth_prompt {
        AuthPrompt::SignIn => "Sign in with GitHub".to_owned(),
        AuthPrompt::Copy(caption) | AuthPrompt::Message(caption) => caption.clone(),
    };
    let message = if !picker.status.is_empty() {
        picker.status.as_str()
    } else {
        picker.auth_message()
    };
    let status_key = (status, message.to_owned());
    if render.status.as_ref() != Some(&status_key) {
        commands.entity(status).insert(Text::new(&status_key.1));
        render.status = Some(status_key);
    }
    let key = (auth, caption);
    if render.auth.as_ref() != Some(&key) {
        let mut controls = commands.entity(auth);
        controls.despawn_related::<Children>();
        controls.with_children(|children| match auth_prompt {
            AuthPrompt::Copy(_) => {
                button::<_, RepositoryPicker>(children, &key.1, INK, PanelAction::CopyCode);
            }
            AuthPrompt::Message(_) => text(children, &key.1, INK),
            AuthPrompt::SignIn => {
                button::<_, RepositoryPicker>(children, &key.1, INK, PanelAction::SignIn);
            }
        });
        render.auth = Some(key);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn show_github(
    commands: &mut Commands,
    picker: &mut RepositoryPicker,
    focus: &mut InputFocus,
    parents: &Query<&ChildOf>,
) {
    close_view(commands, picker, focus, parents);
    let view = draw_github(commands, &picker.choices);
    if let Some(input) = view.input() {
        focus.set(input, FocusCause::Navigated);
    }
    picker.view = Some(view);
}

pub(super) fn spawn(
    commands: &mut Commands,
    font: &TextFont,
    ink: Color,
    images: &mut Assets<Image>,
) {
    use bevy::{
        asset::RenderAssetUsages,
        image::{CompressedImageFormats, ImageSampler, ImageType},
    };
    let icon = Image::from_buffer(
        include_bytes!("../../../../web/assets/apple-touch-icon.png"),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .expect("project robot icon");
    let icon = images.add(icon);
    commands
        .spawn((
            file_picker::FileButton,
            bevy::ui_widgets::Button,
            bevy::picking::hover::Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(24.),
                top: px(16.),
                column_gap: px(8.),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(8.), px(5.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.07, 0.09, 0.95)),
            observe(|_: On<Activate>, mut picker: ResMut<RepositoryPicker>| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    picker.requested = Some(PickerAction::Menu);
                }
            }),
        ))
        .with_children(|ui| {
            ui.spawn((
                ImageNode::new(icon),
                Node {
                    width: px(30.),
                    height: px(30.),
                    ..default()
                },
                bevy::picking::Pickable::IGNORE,
            ));
            ui.spawn((
                Text::new("Vibe City"),
                TextFont::from_font_size(18.),
                TextColor(ink),
                bevy::picking::Pickable::IGNORE,
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
            top: px(66.),
            max_width: px(470.),
            display: Display::None,
            padding: UiRect::all(px(10.)),
            ..default()
        },
    ));
}

#[cfg(not(target_arch = "wasm32"))]
fn draw_menu(commands: &mut Commands) -> RepositoryView {
    let panel = native_panel::panel(commands, "Vibe City", 390.);
    commands.entity(panel).with_children(|menu| {
        for (label, action) in [
            ("Open local repository…", PickerAction::Folder),
            ("Revisions…", PickerAction::Revision),
            ("GitHub…", PickerAction::GitHub),
        ] {
            native_panel::button::<_, RepositoryPicker>(
                menu,
                label,
                native_panel::INK,
                PanelAction::Choose(action),
            );
        }
        menu.spawn((
            file_picker::button(
                "Find file  Cmd+P",
                &native_panel::panel_font(),
                native_panel::INK,
                AlignSelf::Stretch,
            ),
            observe(
                |_: On<Activate>,
                 mut files: ResMut<file_picker::FilePicker>,
                 mut picker: ResMut<RepositoryPicker>| {
                    files.open();
                    picker.action = Some(PanelAction::Close);
                },
            ),
        ));
        native_panel::button::<_, RepositoryPicker>(
            menu,
            "×",
            native_panel::INK,
            PanelAction::Close,
        );
    });
    RepositoryView::Menu { panel }
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
    (mut github_render, mut clipboard, terminal): (
        Local<GitHubRender>,
        ResMut<Clipboard>,
        Option<Res<terminal::Terminal>>,
    ),
    review: Res<review_notes::Review>,
) {
    let Ok((mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let shortcut = !terminal.is_some_and(|terminal| terminal.active)
        && keys.just_pressed(KeyCode::KeyO)
        && keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let was_blocked = picker.view.is_some() || picker.folder.is_some();
    let mut opened = false;
    if let Some(result) = picker.restoring.as_ref().and_then(native_github::Job::take) {
        picker.restoring = None;
        picker.session = result.ok().flatten();
        if let Some((target, revision, base, file)) = picker.queued_github.take() {
            picker.start_github(target, revision, base, file);
        }
    }

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
    if let Some(result) = picker.comments.as_ref().and_then(native_github::Job::take) {
        picker.comments = None;
        commands.insert_resource(native_github::PullComments::Loaded(result));
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
                window.title = format!("Vibe City · {}", snapshot.target.label());
                picker.comments = native_github::pull_comments(
                    &snapshot.target,
                    &next,
                    native_github::token(picker.session.as_ref()),
                );
                if picker.comments.is_some() {
                    commands.insert_resource(native_github::PullComments::Loading);
                } else {
                    commands.remove_resource::<native_github::PullComments>();
                }
                picker.github = Some(snapshot.target);
                picker.pull_open = snapshot.pull_open;
                picker.status.clear();
                if matches!(picker.view, Some(RepositoryView::GitHub { .. })) {
                    close_view(&mut commands, &mut picker, &mut focus, &parents);
                }
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
                picker.comments = None;
                commands.remove_resource::<native_github::PullComments>();
                picker.github = None;
                picker.pull_open = false;
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
                picker.code_copied = false;
                picker.auth_error = native_github::browser_sign_in().err().unwrap_or_default();
                picker.login = Some(native_github::finish_sign_in(code));
            }
            Err(error) => picker.auth_error = error,
        }
    }
    if let Some(result) = picker.login.as_ref().and_then(native_github::Job::take) {
        picker.login = None;
        picker.sign_in_code = None;
        match result {
            Ok(session) => {
                if let Some(token) = native_github::token(Some(&session)) {
                    picker.choices.load_library(&session.login, token);
                }
                picker.session = Some(session);
                picker.auth_error.clear();
            }
            Err(error) => picker.auth_error = error,
        }
    }
    let github = matches!(picker.view, Some(RepositoryView::GitHub { .. }));
    let searching = github && (picker.choices.editor.is_some() || picker.choices.target.is_none());
    if searching {
        if keys.just_pressed(KeyCode::ArrowDown) {
            picker.choices.step(true);
        } else if keys.just_pressed(KeyCode::ArrowUp) {
            picker.choices.step(false);
        }
    }
    let panel_action = picker.action.take().or_else(|| {
        if picker.view.is_none() {
            None
        } else if keys.just_pressed(KeyCode::Escape) {
            Some(
                picker
                    .choices
                    .editor
                    .filter(|_| github)
                    .map_or(PanelAction::Close, PanelAction::EditGithub),
            )
        } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])
            && !matches!(picker.view, Some(RepositoryView::Menu { .. }))
        {
            if searching {
                picker
                    .choices
                    .active
                    .map(PanelAction::GitHubMatch)
                    .or_else(|| match picker.choices.editor {
                        Some(Editor::PullRequests) => None,
                        Some(Editor::Revision | Editor::Base) => Some(PanelAction::UseRevision),
                        None => Some(PanelAction::SelectRepository),
                    })
            } else {
                Some(PanelAction::Open)
            }
        } else {
            None
        }
    });
    if let Some(action) = panel_action
        && (picker.pending.is_none()
            || matches!(action, PanelAction::Close | PanelAction::CopyCode))
        && let Some(view) = picker.view.as_ref()
    {
        let input = view
            .input()
            .and_then(|input| inputs.get(input).ok())
            .map(|input| input.value().to_string())
            .unwrap_or_default();
        match action {
            PanelAction::SignIn if github => picker.request_sign_in(),
            PanelAction::CopyCode if github => picker.copy_code(&mut clipboard),
            PanelAction::UseRevision
            | PanelAction::SelectRepository
            | PanelAction::GitHubMatch(_)
                if github =>
            {
                let chosen = match action {
                    PanelAction::GitHubMatch(index) => {
                        picker.choices.rows.get(index).map(|row| row.value.clone())
                    }
                    _ => Some(input.trim().to_owned()),
                };
                if let Some(chosen) = chosen {
                    match picker.choices.editor {
                        Some(Editor::Revision) => picker.choices.revision = chosen,
                        Some(Editor::Base) => picker.choices.base = chosen,
                        Some(Editor::PullRequests) | None => {
                            match native_github::Target::parse(&chosen) {
                                Ok(target) => picker.choices.set_target(Some(target)),
                                Err(error) => {
                                    picker.status = error;
                                    sync_github(
                                        &mut commands,
                                        &mut picker,
                                        &inputs,
                                        &mut github_render,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    picker.status.clear();
                    picker.choices.editor = None;
                    picker.choices.reset_rows();
                    show_github(&mut commands, &mut picker, &mut focus, &parents);
                }
            }
            PanelAction::ChangeRepository if github => {
                picker.status.clear();
                picker.choices.set_target(None);
                show_github(&mut commands, &mut picker, &mut focus, &parents);
            }
            PanelAction::EditGithub(editor) if github => {
                if picker.choices.editor == Some(editor) {
                    picker.choices.editor = None;
                    picker.choices.reset_rows();
                } else {
                    let token = native_github::token(picker.session.as_ref());
                    picker.choices.edit(editor, token);
                }
                show_github(&mut commands, &mut picker, &mut focus, &parents);
            }
            PanelAction::Open if github => {
                if let Some(target) = picker.choices.target.clone() {
                    let revision = picker.choices.revision.clone();
                    let base = picker.choices.base.clone();
                    picker.start_github(target, revision, base, None);
                }
            }
            PanelAction::Open if matches!(view, RepositoryView::Folder { .. }) => {
                if input.trim().is_empty() {
                    picker.status = "Enter the path to a local Git repository.".into();
                    if let Some(RepositoryView::Folder { status, .. }) = &picker.view {
                        commands.entity(*status).insert(Text::new(&picker.status));
                    }
                } else {
                    let path = PathBuf::from(input.trim());
                    picker.status = "Reading repository…".into();
                    picker.local = Some(native_github::Job::spawn(move || {
                        load(&path, "WORKTREE", None)
                    }));
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
            PanelAction::Choose(action) => {
                close_view(&mut commands, &mut picker, &mut focus, &parents);
                picker.requested = Some(action);
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
            PickerAction::Menu => picker.view = Some(draw_menu(&mut commands)),
            PickerAction::Folder => {
                #[cfg(target_os = "macos")]
                match choose_folder(&repo.0.root) {
                    Ok(folder) => picker.folder = Some(folder),
                    Err(error) => picker.status = error,
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let view = draw_folder(&mut commands, &repo.0.root);
                    focus.set(view.input().expect("folder input"), FocusCause::Navigated);
                    picker.view = Some(view);
                }
            }
            PickerAction::Revision if repo.0.files.is_empty() || picker.github.is_some() => {
                picker.status = "Open a local repository to choose a local revision.".into();
            }
            PickerAction::GitHub => {
                let target = picker.github.clone();
                if picker.choices.target != target {
                    picker.choices.set_target(target);
                } else {
                    picker.choices.editor = None;
                    picker.choices.reset_rows();
                }
                if let Some(session) = &picker.session
                    && let Some(token) = native_github::token(Some(session))
                {
                    let login = session.login.clone();
                    picker.choices.load_library(&login, token);
                }
                show_github(&mut commands, &mut picker, &mut focus, &parents);
            }
            PickerAction::Revision => {
                let view = draw_revisions(&mut commands);
                focus.set(view.input().expect("revision input"), FocusCause::Navigated);
                picker.view = Some(view);
                let path = repo.0.root.clone();
                picker.revision_job = Some(native_github::Job::spawn(move || {
                    repo_world::local_revisions(&path)
                }));
            }
        }
    }
    sync_github(&mut commands, &mut picker, &inputs, &mut github_render);
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
    fn windows_settings_use_roaming_app_data_without_home() {
        let directory = settings_directory("windows", |name| {
            (name == "APPDATA").then(|| "C:/Users/reviewer/AppData/Roaming".into())
        });
        assert_eq!(
            directory,
            Some(PathBuf::from("C:/Users/reviewer/AppData/Roaming/Repo City"))
        );
    }

    #[test]
    fn linux_settings_accept_xdg_without_home() {
        let directory = settings_directory("linux", |name| {
            (name == "XDG_CONFIG_HOME").then(|| "/reviewer/config".into())
        });
        assert_eq!(directory, Some(PathBuf::from("/reviewer/config/Repo City")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn typed_folder_form_keeps_a_blank_path_editable() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.insert_resource(RepositoryPicker::new(String::new()))
            .insert_resource(Repo(empty()))
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Clipboard>()
            .init_resource::<review_notes::Review>()
            .init_resource::<live_worktree::Live>()
            .add_systems(Update, open_repository);
        app.world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow));
        app.world_mut()
            .run_system_once(
                |mut commands: Commands, mut picker: ResMut<RepositoryPicker>| {
                    picker.view = Some(draw_folder(&mut commands, Path::new("")));
                    picker.action = Some(PanelAction::Open);
                },
            )
            .unwrap();
        app.update();
        let picker = app.world().resource::<RepositoryPicker>();
        assert!(
            picker.local.is_none(),
            "blank path must not open HEAD of the old repository"
        );
        assert!(matches!(picker.view, Some(RepositoryView::Folder { .. })));
        assert_eq!(picker.status, "Enter the path to a local Git repository.");
        let message = picker.status.clone();
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == message),
            "folder validation must be visible inside the form"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn github_picker_updates_matches_as_you_type_and_enter_selects_the_highlighted_repository() {
        let mut app = App::new();
        let mut picker = RepositoryPicker::new(String::new());
        picker.smoke_open();
        app.insert_resource(picker)
            .insert_resource(Repo(empty()))
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Clipboard>()
            .init_resource::<review_notes::Review>()
            .init_resource::<live_worktree::Live>()
            .add_systems(Update, open_repository);
        app.world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow));
        app.update();
        app.update();
        let picker = app.world().resource::<RepositoryPicker>();
        assert_eq!(picker.choices.rows.len(), 2);
        let field = picker.view.as_ref().unwrap().input().unwrap();
        app.world_mut()
            .get_mut::<EditableText>(field)
            .unwrap()
            .editor_mut()
            .set_text("demo привет");
        app.update();
        let picker = app.world().resource::<RepositoryPicker>();
        assert_eq!(picker.choices.rows.len(), 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        let picker = app.world().resource::<RepositoryPicker>();
        assert_eq!(
            picker.choices.target.as_ref().unwrap().label(),
            "example/demo"
        );
        assert!(picker.pending.is_none());
    }

    #[test]
    fn absent_folder_is_rejected_with_its_path() {
        let path = std::env::temp_dir().join(format!("repo-city-missing-{}", std::process::id()));
        let result = load(&path, "HEAD", None).unwrap_err();
        assert!(result.contains(&path.display().to_string()));
    }
}
