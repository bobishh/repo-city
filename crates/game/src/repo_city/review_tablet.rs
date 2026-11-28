//! Browse changed files and return to their buildings.
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    input::keyboard::KeyboardInput,
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    text::EditableText,
};

use super::*;
use file_picker::content_search;

const FILES_PER_PAGE: usize = 10;
const TEXT_SEARCH_DELAY: f64 = 0.25;
type FileTimes = HashMap<String, u64>;

#[derive(Resource, Default)]
pub(super) struct Tablet {
    panel: Option<Entity>,
    action: Option<Action>,
    pending: Option<native_github::Job<FileTimes>>,
    times: FileTimes,
    scope: Option<(PathBuf, String, Option<String>)>,
    file_filter: FileFilter,
    file_activity: HashMap<String, agent_timeline::FileActivity>,
    activity_stamp: Option<(agent_engine::Provider, u64)>,
    agy_chat: bool,
    files_scroll: f32,
    file_buttons: Vec<(String, Entity)>,
    page: usize,
    view: Page,
    dates_requested: bool,
    bounds: [String; 4],
    inputs: Option<[Entity; 4]>,
    settings_status: Option<Entity>,
    mode: reading::Mode,
    block_kind: reading::BlockKind,
    auto_advance: bool,
    show_all_source: bool,
    reading_enabled: bool,
    status: String,
    document: Option<Document>,
    links_version: u64,
    image: Option<ImageView>,
    video: Option<super::video_preview::Preview>,
    playing_videos: Vec<super::video_preview::Video>,
    history: Option<History>,
    search: String,
    search_input: Option<Entity>,
    search_list: Option<Entity>,
    search_text: TextSearch,
    trace_input: Option<Entity>,
    trace_follow: Option<Entity>,
    trace_version: u64,
    chat_room: Option<String>,
}

#[derive(Default)]
struct TextSearch {
    due: Option<f64>,
    pending: Option<(String, native_github::Job<content_search::Matches>)>,
    result: Option<Result<content_search::Matches, String>>,
}

impl TextSearch {
    fn update(
        &mut self,
        repo: &Repository,
        query: &str,
        now: f64,
        changed: bool,
        submit: bool,
    ) -> bool {
        let has_query = !query.trim().is_empty();
        if changed {
            self.due = has_query.then_some(now + TEXT_SEARCH_DELAY);
            self.result = None;
        }
        let already_searching = self.due.is_none()
            && self
                .pending
                .as_ref()
                .is_some_and(|(active, _)| active == query);
        let submit = submit && has_query && !already_searching;
        if submit {
            self.due = Some(now);
            self.result = None;
        }
        let mut redraw = changed || submit;
        if let Some((active, result)) = self
            .pending
            .as_ref()
            .and_then(|(active, job)| job.take().map(|result| (active, result)))
        {
            if active == query && self.due.is_none() {
                self.result = Some(result);
                redraw = true;
            }
            self.pending = None;
        }
        if self.pending.is_none() && self.due.is_some_and(|due| now >= due) {
            self.due = None;
            let snapshot = repo.clone();
            let query = query.to_owned();
            self.pending = Some((
                query.clone(),
                native_github::Job::spawn(move || content_search::search(&snapshot, &query)),
            ));
            redraw = true;
        }
        redraw
    }
}

struct History {
    root: PathBuf,
    revision: String,
    skip: usize,
    pending: Option<native_github::Job<Vec<repo_world::history::Commit>>>,
    commits: Result<Vec<repo_world::history::Commit>, String>,
    showing: Option<native_github::Job<String>>,
    detail: Option<Result<String, String>>,
    before: Option<String>,
    after: Option<String>,
}

impl History {
    fn open(repo: &Repository, skip: usize) -> Self {
        let root = repo.root.clone();
        let revision = repo.revision.clone();
        let (path, rev) = (root.clone(), revision.clone());
        Self {
            root,
            revision,
            skip,
            pending: Some(native_github::Job::spawn(move || {
                repo_world::history::commits(&path, &rev, skip)
            })),
            commits: Ok(Vec::new()),
            showing: None,
            detail: None,
            before: repo.baseline.clone(),
            after: (!repo.is_worktree()).then(|| repo.revision.clone()),
        }
    }
    fn show(&mut self, index: usize) {
        if let Some(commit) = self
            .commits
            .as_ref()
            .ok()
            .and_then(|commits| commits.get(index))
        {
            let root = self.root.clone();
            let id = commit.id.clone();
            self.showing = Some(native_github::Job::spawn(move || {
                repo_world::history::show(&root, &id)
            }));
            self.detail = None;
        }
    }
    fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self.pending.as_ref().and_then(native_github::Job::take) {
            self.pending = None;
            self.commits = result;
            changed = true;
        }
        if let Some(result) = self.showing.as_ref().and_then(native_github::Job::take) {
            self.showing = None;
            self.detail = Some(result);
            changed = true;
        }
        changed
    }
}

struct ImageView {
    preview: image_preview::Preview,
    result: Option<Result<Handle<Image>, String>>,
}

#[derive(Default)]
struct Document {
    path: String,
    pending: Option<native_github::Job<LoadedDocument>>,
    result: Option<Result<LoadedDocument, String>>,
    content: Option<Entity>,
    page: usize,
    read_pages: HashSet<usize>,
    before: bool,
    range: std::ops::Range<usize>,
    editing: bool,
    edited_source: Option<String>,
}

struct LoadedDocument {
    source: String,
    mark: markdown_preview::ReadDocument,
    offsets: Vec<usize>,
}

impl Document {
    fn open(
        repository: &Repository,
        index: usize,
        before: bool,
        range: std::ops::Range<usize>,
    ) -> Option<Self> {
        let file = repository.files.get(index)?.clone();
        let path = file.path.clone();
        let file_snapshot = Repository {
            root: repository.root.clone(),
            revision: repository.revision.clone(),
            baseline: repository.baseline.clone(),
            files: vec![file],
            districts: Vec::new(),
        };
        let read_path = path.clone();
        Some(Self {
            path,
            before,
            range,
            pending: Some(native_github::Job::spawn(move || {
                let source = file_snapshot.source(0, before)?;
                let file = &file_snapshot.files[0];
                let blob = if before { &file.before } else { &file.after };
                let blob = blob
                    .as_ref()
                    .ok_or("File does not exist in this snapshot")?
                    .id
                    .clone();
                let read = markdown_preview::ReadDocument {
                    path: read_path,
                    before,
                    blob,
                    rows: source.split_inclusive('\n').count(),
                };
                Ok(LoadedDocument {
                    offsets: markdown_preview::page_offsets(&source),
                    source,
                    mark: read,
                })
            })),
            ..default()
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Files,
    Help,
    Settings,
    Document,
    Connections,
    Image,
    Video,
    Git,
    Search,
    Terminal,
    Trace,
    Agent,
}

impl Page {
    pub(super) const NAVIGATION: [(Self, &'static str); 8] = [
        (Self::Files, "Files"),
        (Self::Search, "Find"),
        (Self::Help, "Help"),
        (Self::Connections, "Connections"),
        (Self::Git, "Git"),
        (Self::Terminal, "Terminal"),
        (Self::Trace, "Trace"),
        (Self::Agent, "Chat"),
    ];
}

#[derive(Component, Clone, Copy)]
enum Action {
    Visit(usize),
    VisitMatch { index: usize, line: usize },
    OpenCanvas,
    ChooseCanvasFiles,
    Filter(FileFilter),
    Page(usize),
    Switch(Page),
    Mode(reading::Mode),
    Block(reading::BlockKind),
    AutoAdvance,
    ShowSource,
    ReadingEnabled,
    MuteAudio,
    GitShow(usize),
    GitPage(usize),
    GitBack,
    GitBefore(usize),
    GitAfter(usize),
    GitCompare,
    StartTour,
    EditDocument,
    DocumentPage(usize),
    Connection(usize),
    TraceImport,
    TraceFollow,
    TraceSelect(usize),
    AgentProvider(agent_engine::Provider),
    Ambience,
    Speech,
    FileChangeSound,
    Voice,
    Radio,
    Volume(f32),
    Close,
}

impl native_panel::Actions<Action> for Tablet {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl Tablet {
    pub(super) fn smoke_set_find_query(&self, commands: &mut Commands, query: String) {
        let input = self.search_input.expect("Find input");
        commands.queue(move |world: &mut World| {
            world
                .get_mut::<EditableText>(input)
                .expect("Find text editor")
                .editor_mut()
                .set_text(&query);
        });
    }

    pub(super) fn smoke_find_ready(&self, repo: &Repository) -> bool {
        self.view == Page::Search
            && !self.search.is_empty()
            && repo
                .files
                .iter()
                .any(|file| file_picker::rank(&file.path, &self.search.to_lowercase()).is_some())
            && self
                .search_text
                .result
                .as_ref()
                .is_some_and(|result| result.as_ref().is_ok_and(|matches| !matches.is_empty()))
    }

    pub(super) fn smoke_open_document(&mut self, repo: &Repository, index: usize, before: bool) {
        self.document = Document::open(repo, index, before, 0..6);
        self.open_page(Page::Document);
    }

    pub(super) fn smoke_large_document(&self) -> Result<(), String> {
        if self.view != Page::Document {
            return Err("VibePad did not switch to the document".into());
        }
        let document = self
            .document
            .as_ref()
            .ok_or("VibePad did not open the document")?;
        let loaded = document
            .result
            .as_ref()
            .ok_or("The document is still loading")?
            .as_ref()
            .map_err(Clone::clone)?;
        if loaded.source.len() > 1024 * 1024
            && loaded.offsets.len() > 2
            && document.content.is_some()
        {
            Ok(())
        } else {
            Err("The document preview does not display multiple pages".into())
        }
    }

    pub(super) fn open_help(&mut self) {
        self.open_page(Page::Help);
    }

    pub(super) fn request_close(&mut self) {
        self.action = Some(Action::Close);
    }

    pub(super) fn open_page(&mut self, page: Page) {
        self.action = Some(Action::Switch(page));
    }

    pub(super) fn open_agy_voice(&mut self) {
        self.open_page(if self.chat_room.is_some() {
            Page::Terminal
        } else {
            Page::Agent
        });
    }

    pub(super) fn is_open(&self) -> bool {
        self.panel.is_some()
    }

    pub(super) fn page_name(&self) -> Option<&'static str> {
        self.is_open().then_some(match self.view {
            Page::Files => "files",
            Page::Connections => "connections",
            Page::Help => "help",
            Page::Settings => "settings",
            Page::Document => "document",
            Page::Image => "image",
            Page::Video => "video",
            Page::Git => "git",
            Page::Search => "search",
            Page::Terminal => "terminal",
            Page::Trace => "trace",
            Page::Agent => "agent",
        })
    }

    fn switch(&mut self, view: Page, reading: &reading::Settings) {
        if view != Page::Document {
            self.document = None;
        }
        if view != self.view && view == Page::Settings {
            let [min, max] = [reading.min_seconds, reading.max_seconds]
                .map(|value| value.map_or_else(String::new, |value| value.to_string()));
            self.bounds = [
                reading.block_lines.to_string(),
                min,
                max,
                reading.fly_to_font_px.to_string(),
            ];
            self.mode = reading.mode;
            self.block_kind = reading.block_kind;
            self.auto_advance = reading.auto_advance;
            self.show_all_source = reading.show_all_source;
            self.reading_enabled = reading.enabled;
            self.status.clear();
        }
        self.view = view;
    }
}

pub(super) fn spawn_help(commands: &mut Commands, font: &TextFont, ink: Color) {
    use bevy::ui_widgets::{Activate, observe};
    commands
        .spawn((
            GlobalZIndex(35),
            Node {
                position_type: PositionType::Absolute,
                left: px(24.),
                bottom: px(22.),
                ..default()
            },
        ))
        .with_children(|controls| {
            controls
                .spawn((
                    file_picker::button("?", font, ink, AlignSelf::Auto),
                    observe(|_: On<Activate>, mut tablet: ResMut<Tablet>| tablet.open_help()),
                ))
                .insert(Node {
                    width: px(32.),
                    height: px(32.),
                    padding: UiRect::ZERO,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                });
        });
}

#[derive(Clone, Copy)]
enum Freshness {
    Watched(f64),
    Saved(u64),
    Committed(u64),
    Unknown,
}

impl Freshness {
    fn order(self) -> (bool, u64) {
        match self {
            Self::Watched(time) => (true, (time * 1000.) as u64),
            Self::Saved(time) | Self::Committed(time) => (false, time),
            Self::Unknown => (false, 0),
        }
    }

    fn label(self, elapsed: f64, now: u64) -> String {
        let (verb, age) = match self {
            Self::Watched(time) => ("Updated", (elapsed - time).max(0.) as u64),
            Self::Saved(time) => ("Saved", now.saturating_sub(time)),
            Self::Committed(time) => ("Committed", now.saturating_sub(time)),
            Self::Unknown => return String::new(),
        };
        let age = match age {
            0..60 => format!("{age}s"),
            60..3600 => format!("{}m", age / 60),
            3600..86400 => format!("{}h", age / 3600),
            _ => format!("{}d", age / 86400),
        };
        format!("{verb} {age} ago")
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum FileFilter {
    #[default]
    Changes,
    Viewed,
    Agent,
}

struct FileRow<'a> {
    index: usize,
    file: &'a repo_world::File,
    freshness: Freshness,
    activity: Option<agent_timeline::FileActivity>,
}

impl FileRow<'_> {
    fn label(&self, filter: FileFilter, elapsed: f64, now: u64) -> String {
        let freshness = self.freshness.label(elapsed, now);
        let detail = match self.activity {
            Some(activity) if filter == FileFilter::Agent || freshness.is_empty() => {
                activity.label(now, true)
            }
            Some(activity) => format!("{freshness} · {}", activity.label(now, false)),
            None if freshness.is_empty() => " ".into(),
            None => freshness,
        };
        format!("{}\n{detail}", self.file.path)
    }
}

fn file_rows<'a>(
    repo: &'a Repository,
    progress: &progress::Progress,
    live: &live_worktree::Live,
    times: &FileTimes,
    filter: FileFilter,
    activity: &HashMap<String, agent_timeline::FileActivity>,
) -> Vec<FileRow<'a>> {
    let mut files = repo
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            let activity = activity.get(&file.path).copied();
            let changed = file.change != Change::Unchanged;
            let included = match filter {
                FileFilter::Changes => changed,
                FileFilter::Viewed => changed && progress.has_visited(index),
                FileFilter::Agent => activity.is_some(),
            };
            if !included {
                return None;
            }
            let freshness = if repo.is_worktree() {
                live.touched_at(&file.path)
                    .map(Freshness::Watched)
                    .or_else(|| times.get(&file.path).copied().map(Freshness::Saved))
            } else {
                times.get(&file.path).copied().map(Freshness::Committed)
            };
            Some(FileRow {
                index,
                file,
                freshness: freshness.unwrap_or(Freshness::Unknown),
                activity,
            })
        })
        .collect::<Vec<_>>();
    files.sort_unstable_by(|left, right| {
        let order = if filter == FileFilter::Agent {
            right
                .activity
                .map(agent_timeline::FileActivity::order)
                .cmp(&left.activity.map(agent_timeline::FileActivity::order))
        } else {
            right.freshness.order().cmp(&left.freshness.order())
        };
        order.then_with(|| left.file.path.cmp(&right.file.path))
    });
    files
}

fn load_times(repo: &Repository) -> native_github::Job<FileTimes> {
    let root = repo.root.clone();
    let revision = repo.revision.clone();
    let baseline = repo.baseline.clone();
    if repo.is_worktree() {
        let paths = repo
            .files
            .iter()
            .filter(|file| file.change != Change::Unchanged && file.after.is_some())
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        native_github::Job::spawn(move || {
            Ok(paths
                .into_iter()
                .filter_map(|path| {
                    let modified = root.join(&path).symlink_metadata().ok()?.modified().ok()?;
                    Some((path, modified.duration_since(UNIX_EPOCH).ok()?.as_secs()))
                })
                .collect())
        })
    } else {
        native_github::Job::spawn(move || {
            repo_world::changed_path_times(&root, baseline.as_deref(), &revision)
        })
    }
}

#[derive(Component)]
#[require(tablet_scroll::Area)]
pub(super) struct FileRows;

pub(super) fn plain_tab(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::Tab) && unmodified(keys)
}

fn unmodified(keys: &ButtonInput<KeyCode>) -> bool {
    !keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ])
}

pub(super) fn update(
    mut commands: Commands,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time>,
    repo: Res<Repo>,
    progress: Res<progress::Progress>,
    live: Res<live_worktree::Live>,
    menu: Res<city_menu::Menu>,
    search: Res<listing::find::Search>,
    mut picker: ResMut<file_picker::FilePicker>,
    mut tablet: ResMut<Tablet>,
    mut review: ResMut<review_notes::Review>,
    mut focus: ResMut<InputFocus>,
    parents: Query<&ChildOf>,
    wheel: Res<AccumulatedMouseScroll>,
    mut rows: Query<(&mut ScrollPosition, &ComputedNode), With<FileRows>>,
    (
        mut reading,
        mut tour,
        mut repository,
        fields,
        mut pilot,
        mut cursor,
        mut markdown,
        mut read,
        layout,
        mut links,
        mut audio,
        mut image_request,
        mut images,
        mut terminal,
        mut trace,
        (editors, mut agent, party, windows, world, mut input, canvas_state),
    ): (
        ResMut<reading::Settings>,
        ResMut<guide::Tour>,
        ResMut<repository_picker::RepositoryPicker>,
        Query<&EditableText>,
        Query<&mut Pilot, PlayerCamera>,
        Query<&mut CursorOptions, With<PrimaryWindow>>,
        Option<ResMut<markdown_preview::Request>>,
        Option<ResMut<markdown_preview::ReadQueue>>,
        Query<&ComputedNode, With<bevy_markup::prelude::HtmlUi>>,
        Option<ResMut<file_links::Links>>,
        Option<ResMut<city_audio::Settings>>,
        Option<ResMut<image_preview::Request>>,
        Option<ResMut<Assets<Image>>>,
        Option<ResMut<terminal::Terminal>>,
        Option<ResMut<trace_import::Trace>>,
        (
            Query<&source_editor::View>,
            Option<ResMut<agent_session::Session>>,
            Option<Res<review_party::Party>>,
            Query<&Window, With<PrimaryWindow>>,
            Option<Res<WorldSet>>,
            MessageReader<KeyboardInput>,
            Option<Res<canvas::Canvas>>,
        ),
    ),
) {
    if canvas::active(canvas_state) {
        review.blocked = true;
        review.tablet_open = tablet.is_open();
        return;
    }
    let editing = focus
        .get()
        .is_some_and(|entity| fields.contains(entity) || editors.contains(entity));
    let chat_room = party.as_deref().and_then(review_party::Party::chat_room);
    let agy_chat = chat_room.is_none()
        && terminal
            .as_deref()
            .is_some_and(|terminal| terminal.agy_voice(&repo.0.root).is_some());
    let terminal_page = tablet.is_open()
        && (tablet.view == Page::Terminal || tablet.view == Page::Agent && tablet.agy_chat);
    let terminal_escape = terminal_page && keys.just_pressed(KeyCode::Escape) && unmodified(&keys);
    let previous_page = tablet.view;
    if reading.is_changed() {
        tablet.reading_enabled = reading.enabled;
    }
    let help = keys.just_pressed(KeyCode::KeyH) && unmodified(&keys) && !editing && !terminal_page;
    let slash = file_picker::search_pressed(&keys, &mut input)
        && world.as_deref().is_none_or(WorldSet::city_controls)
        && !editing
        && !menu.blocks_controls()
        && !search.blocks_controls()
        && !repository.blocks_controls();
    let find_file = !terminal_page
        && (slash
            || keys.just_pressed(KeyCode::KeyP)
                && keys.any_pressed([
                    KeyCode::SuperLeft,
                    KeyCode::SuperRight,
                    KeyCode::ControlLeft,
                    KeyCode::ControlRight,
                ]));
    let action = tablet
        .action
        .take()
        .or_else(|| find_file.then_some(Action::Switch(Page::Search)))
        .or_else(|| {
            if !tablet.is_open()
                || terminal_page
                || !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
            {
                return None;
            }
            [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
            ]
            .iter()
            .position(|key| keys.just_pressed(*key))
            .map(|index| Action::Switch(Page::NAVIGATION[index].0))
        })
        .or_else(|| {
            help.then_some(if tablet.view == Page::Help {
                Action::Close
            } else {
                Action::Switch(Page::Help)
            })
        });
    let markdown = markdown.as_deref_mut().and_then(|request| request.0.take());
    let image = image_request
        .as_deref_mut()
        .and_then(|request| request.0.take());
    let was_open = tablet.panel.is_some();
    let shortcuts_allowed = windows.single().is_ok_and(|window| window.focused)
        && unmodified(&keys)
        && !editing
        && !terminal_page
        && !menu.is_open()
        && !picker.is_open()
        && !search.blocks_controls()
        && !repository.blocks_controls();
    let toggle_reading = matches!(action, Some(Action::ReadingEnabled))
        || (keys.just_pressed(KeyCode::F4) && shortcuts_allowed);
    let toggle_sound = matches!(action, Some(Action::MuteAudio))
        || (keys.just_pressed(KeyCode::F10)
            && unmodified(&keys)
            && windows.single().is_ok_and(|window| window.focused));
    if toggle_reading {
        reading.enabled = !reading.enabled;
        tablet.reading_enabled = reading.enabled;
        if let Err(error) = reading.save() {
            tablet.status = error;
            warn!("Could not save reading mode: {}", tablet.status);
        }
    }
    if toggle_sound && let Some(audio) = audio.as_deref_mut() {
        audio.toggle_mute();
        if let Err(error) = audio.save() {
            tablet.status = error;
            warn!("Could not save sound preferences: {}", tablet.status);
        }
    }
    let mut redraw = !was_open || toggle_reading || toggle_sound;
    if tablet.chat_room.as_deref() != chat_room {
        tablet.chat_room = chat_room.map(str::to_owned);
        redraw |= tablet.view == Page::Agent;
    }
    if tablet.agy_chat != agy_chat {
        tablet.agy_chat = agy_chat;
        redraw |= tablet.view == Page::Agent;
    }
    if let Some(panel) = tablet.panel {
        if tablet.view == Page::Trace
            && tablet
                .scope
                .as_ref()
                .is_some_and(|(root, revision, baseline)| {
                    root == &repo.0.root
                        && revision == &repo.0.revision
                        && baseline == &repo.0.baseline
                })
            && let Some(trace) = trace.as_deref_mut()
            && let Some(input) = tablet
                .trace_input
                .and_then(|entity| fields.get(entity).ok())
        {
            trace.source = input.editor().text().to_string();
            if let Some(follow) = tablet.trace_follow {
                if trace.can_follow() {
                    commands
                        .entity(follow)
                        .remove::<bevy::ui::InteractionDisabled>();
                } else {
                    commands
                        .entity(follow)
                        .insert(bevy::ui::InteractionDisabled);
                }
            }
        }
        if tablet.view == Page::Settings {
            let previous = tablet.bounds.clone();
            tablet.capture_bounds(&fields);
            if previous != tablet.bounds {
                tablet.save_reading(&mut reading);
                if let Some(status) = tablet.settings_status {
                    commands.entity(status).insert(Text::new(&tablet.status));
                }
            }
        }
        let mut same_comparison =
            tablet
                .scope
                .as_ref()
                .is_some_and(|(root, revision, baseline)| {
                    root == &repo.0.root
                        && revision == &repo.0.revision
                        && baseline == &repo.0.baseline
                });
        if terminal_page
            && !same_comparison
            && tablet
                .scope
                .as_ref()
                .is_some_and(|(root, _, _)| root == &repo.0.root)
        {
            tablet.scope = Some((
                repo.0.root.clone(),
                repo.0.revision.clone(),
                repo.0.baseline.clone(),
            ));
            same_comparison = true;
        }
        if same_comparison
            && tablet.view == Page::Document
            && let Some(document) = tablet
                .document
                .as_mut()
                .filter(|document| !document.editing && document.edited_source.is_none())
            && let Some(content) = document.content.and_then(|entity| layout.get(entity).ok())
            && content.size().y > 0.
            && let Some(Ok(loaded)) = &document.result
            && let Ok((scroll, viewport)) = rows.single()
            && viewport.size().y > 0.
            && scroll.y * viewport.inverse_scale_factor().recip() + viewport.size().y + 2.
                >= viewport.content_size().y
        {
            if document.read_pages.insert(document.page)
                && document.read_pages.len() + 1 == loaded.offsets.len()
                && let Some(queue) = &mut read
            {
                queue.0 = Some(loaded.mark.clone());
            }
        }
        let connection = match action {
            Some(Action::Connection(index)) => {
                links.as_deref_mut().is_some_and(|links| links.visit(index))
            }
            _ => false,
        };
        let follow_trace = matches!(action, Some(Action::TraceFollow))
            && trace
                .as_deref_mut()
                .is_some_and(|trace| trace.follow(&repo.0));
        let destination = match action {
            Some(Action::Visit(index)) if !repo.is_changed() => Some(index),
            _ => None,
        };
        let match_destination = if let Some(Action::VisitMatch { index, line }) = action
            && same_comparison
            && !repo.is_changed()
            && let Some(file) = repo.0.files.get(index)
        {
            review.jump_to_source(&repo.0, &file.path, line);
            true
        } else {
            false
        };
        if matches!(action, Some(Action::StartTour)) {
            review.select(0);
            tour.start();
        }
        if matches!(action, Some(Action::Close | Action::StartTour))
            || destination.is_some()
            || match_destination
            || connection
            || follow_trace
            || terminal_escape
            || (!terminal_page && keys.just_pressed(KeyCode::Escape))
            || (!terminal_page && plain_tab(&keys))
            || (terminal_page
                && keys.just_pressed(KeyCode::Tab)
                && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]))
            || (help && tablet.view == Page::Help)
            || !same_comparison
        {
            if terminal_escape {
                keys.clear_just_pressed(KeyCode::Escape);
            }
            native_panel::close(&mut commands, panel, &mut focus, &parents);
            *tablet = Tablet::default();
            picker.destination = destination;
            review.blocked |=
                destination.is_none() && !match_destination && !connection && !follow_trace;
            return;
        }
        match action {
            Some(Action::OpenCanvas) => canvas::open(&mut commands, canvas::Open::Changed),
            Some(Action::ChooseCanvasFiles) => canvas::open(&mut commands, canvas::Open::Choose),
            Some(Action::DocumentPage(page)) => {
                if let Some(document) = tablet.document.as_mut() {
                    document.page = page;
                    document.content = None;
                    redraw = true;
                }
            }
            Some(Action::EditDocument) => {
                if let Some(document) = tablet.document.as_mut() {
                    if document.editing {
                        if let Ok(editor) = editors.single() {
                            document.edited_source = Some(editor.full_value());
                            document.range = editor.range();
                            document.page = 0;
                        }
                    }
                    document.editing = !document.editing;
                    redraw = true;
                }
            }
            Some(Action::TraceSelect(index)) => {
                if let Some(trace) = trace.as_deref_mut()
                    && let Some(group) =
                        trace.report.groups.get(index).map(|group| group.id.clone())
                {
                    trace.select(&group);
                }
                redraw = true;
            }
            Some(Action::TraceImport) => {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.import(&repo.0, trace.source.clone());
                }
                redraw = true;
            }
            Some(Action::TraceFollow) => redraw = true,
            Some(Action::Filter(filter)) => {
                tablet.file_filter = filter;
                tablet.page = 0;
                redraw = true;
            }
            Some(Action::Page(page)) => {
                tablet.page = page;
                redraw = true;
            }
            Some(Action::Switch(page)) if page != tablet.view => {
                tablet.switch(page, &reading);
                if page == Page::Git {
                    tablet.history = Some(History::open(&repo.0, 0));
                }
                redraw = true;
            }
            Some(Action::Switch(Page::Search)) if find_file => {
                if let Some(input) = tablet.search_input {
                    focus.set(input, FocusCause::Navigated);
                }
            }
            Some(Action::GitBefore(index) | Action::GitAfter(index)) => {
                if let Some(history) = tablet.history.as_mut() {
                    if let Some(commit) = history
                        .commits
                        .as_ref()
                        .ok()
                        .and_then(|rows| rows.get(index))
                    {
                        if matches!(action, Some(Action::GitBefore(_))) {
                            history.before = Some(commit.id.clone());
                        } else {
                            history.after = Some(commit.id.clone());
                        }
                    }
                }
                redraw = true;
            }
            Some(Action::GitCompare) => {
                if let Some(history) = tablet.history.as_ref()
                    && let (Some(before), Some(after)) = (&history.before, &history.after)
                {
                    repository.compare_commits(history.root.clone(), before.clone(), after.clone());
                    tablet.status = "Loading selected commits…".into();
                    redraw = true;
                }
            }
            Some(Action::GitShow(index)) => {
                if let Some(history) = tablet.history.as_mut() {
                    history.show(index);
                }
                redraw = true;
            }
            Some(Action::GitPage(skip)) => {
                let selection = tablet
                    .history
                    .as_ref()
                    .map(|history| (history.before.clone(), history.after.clone()));
                tablet.history = Some(History::open(&repo.0, skip));
                if let (Some(history), Some((before, after))) = (tablet.history.as_mut(), selection)
                {
                    history.before = before;
                    history.after = after;
                }
                redraw = true;
            }
            Some(Action::GitBack) => {
                if let Some(history) = tablet.history.as_mut() {
                    history.detail = None;
                    history.showing = None;
                }
                redraw = true;
            }
            Some(Action::ShowSource) => {
                tablet.capture_bounds(&fields);
                tablet.show_all_source = !tablet.show_all_source;
                redraw = true;
            }
            Some(Action::AgentProvider(provider)) => {
                tablet.capture_bounds(&fields);
                if let Some(agent) = agent.as_deref_mut() {
                    tablet.status = match agent.select_provider(provider) {
                        Ok(()) => format!("{} selected. Open Agent to connect.", provider.label),
                        Err(error) => error,
                    };
                }
                redraw = true;
            }
            Some(Action::Block(kind)) => {
                tablet.capture_bounds(&fields);
                tablet.block_kind = kind;
                redraw = true;
            }
            Some(Action::AutoAdvance) => {
                tablet.capture_bounds(&fields);
                tablet.auto_advance = !tablet.auto_advance;
                redraw = true;
            }
            Some(Action::Mode(mode)) => {
                tablet.capture_bounds(&fields);
                tablet.mode = mode;
                tablet.status.clear();
                redraw = true;
            }
            Some(
                Action::Ambience
                | Action::Speech
                | Action::FileChangeSound
                | Action::Volume(_)
                | Action::Voice
                | Action::Radio,
            ) => {
                if let Some(audio) = audio.as_deref_mut() {
                    let previous = audio.clone();
                    match action {
                        Some(Action::Ambience) => audio.ambience = !audio.ambience,
                        Some(Action::Speech) => audio.speech = !audio.speech,
                        Some(Action::FileChangeSound) => audio.file_changes = !audio.file_changes,
                        Some(Action::Radio) => audio.radio = audio.radio.next(),
                        Some(Action::Voice) => {
                            audio.voice = match audio.voice.as_str() {
                                "Daniel" => "Rishi",
                                "Rishi" => "Samantha",
                                _ => "Daniel",
                            }
                            .into()
                        }
                        Some(Action::Volume(volume)) => audio.volume = volume,
                        _ => {}
                    }
                    tablet.status = match audio.save() {
                        Ok(()) => "Saved.".into(),
                        Err(error) => {
                            *audio = previous;
                            error
                        }
                    };
                    tablet.capture_bounds(&fields);
                    redraw = true;
                }
            }
            _ => {}
        }
        if matches!(
            action,
            Some(Action::Mode(_) | Action::Block(_) | Action::AutoAdvance | Action::ShowSource)
        ) {
            tablet.save_reading(&mut reading);
        }
        redraw |= tablet.view == Page::Files && repo.is_changed();
        review.blocked = true;
        review.tablet_open = true;
        if let Ok((mut scroll, viewport)) = rows.single_mut() {
            let delta = match wheel.unit {
                bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 34.,
                bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
            };
            scroll.y = (scroll.y - delta).max(0.);
            if !editing {
                let page = viewport.size().y * viewport.inverse_scale_factor() * 0.9;
                if keys.just_pressed(KeyCode::PageDown) {
                    scroll.y += page;
                }
                if keys.just_pressed(KeyCode::PageUp) {
                    scroll.y = (scroll.y - page).max(0.);
                }
                if keys.just_pressed(KeyCode::End) {
                    scroll.y = viewport.content_size().y * viewport.inverse_scale_factor();
                }
                if keys.just_pressed(KeyCode::Home) {
                    scroll.y = 0.;
                }
            }
        }
    } else {
        let page = match action {
            Some(Action::Switch(page)) => page,
            _ if markdown.as_ref().is_some_and(|(index, _, _, _)| {
                repo.0
                    .files
                    .get(*index)
                    .is_some_and(|file| file_preview::video_extension(&file.path).is_some())
            }) =>
            {
                Page::Video
            }
            _ if markdown.is_some() => Page::Document,
            _ if image.is_some() => Page::Image,
            _ if plain_tab(&keys) && agy_chat => Page::Agent,
            _ if plain_tab(&keys) => Page::Files,
            _ => return,
        };
        if editing
            || review.blocked
            || menu.is_open()
            || picker.is_open()
            || search.blocks_controls()
            || repository.blocks_controls()
        {
            return;
        }
        tablet.scope = Some((
            repo.0.root.clone(),
            repo.0.revision.clone(),
            repo.0.baseline.clone(),
        ));
        tablet.switch(page, &reading);
        if page == Page::Git {
            tablet.history = Some(History::open(&repo.0, 0));
        }
        if page == Page::Document
            && let Some((index, before, range, edit)) = markdown.as_ref()
        {
            tablet.document = Document::open(&repo.0, *index, *before, range.clone());
            if let Some(document) = tablet.document.as_mut() {
                document.editing = *edit;
            }
        }
        if page == Page::Image
            && let Some((index, before)) = image
        {
            match image_preview::Preview::open(&repo.0, index, before) {
                Ok(preview) => {
                    tablet.image = Some(ImageView {
                        preview,
                        result: None,
                    })
                }
                Err(error) => tablet.status = error,
            }
        }
        if page == Page::Video
            && let Some((index, before, _, _)) = markdown
        {
            tablet.video = super::video_preview::Preview::open(&repo.0, index, before);
        }
        review.blocked = true;
        review.tablet_open = true;
        review.drag = None;
        if let Ok(mut pilot) = pilot.single_mut() {
            pilot.captured = false;
            pilot.fire_ready = false;
            pilot.firing = false;
            pilot.shot = false;
        }
        if let Ok(mut cursor) = cursor.single_mut() {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    if tablet.view == Page::Files && !tablet.dates_requested {
        tablet.dates_requested = true;
        tablet.pending = Some(load_times(&repo.0));
    }
    if tablet.view == Page::Files {
        let timeline = agent.as_deref().and_then(|session| {
            session
                .timeline(&repo.0.root)
                .map(|timeline| (session.provider(), timeline))
        });
        let stamp = timeline.map(|(provider, timeline)| (provider, timeline.version));
        if tablet.activity_stamp != stamp {
            tablet.activity_stamp = stamp;
            let activity = timeline.map_or_else(HashMap::new, |(_, timeline)| {
                agent_timeline::files(timeline)
            });
            if tablet.file_activity != activity {
                tablet.file_activity = activity;
                redraw = true;
            }
        }
    }
    if let Some(history) = tablet.history.as_mut() {
        redraw |= history.poll();
    }
    if let Some(result) = tablet.pending.as_ref().and_then(native_github::Job::take) {
        tablet.pending = None;
        tablet.times = result.unwrap_or_default();
        redraw |= tablet.view == Page::Files;
    }
    if let Some(document) = tablet.document.as_mut()
        && let Some(result) = document.pending.as_ref().and_then(native_github::Job::take)
    {
        document.pending = None;
        document.result = Some(result);
        redraw |= tablet.view == Page::Document;
    }
    if let Some(image) = tablet.image.as_mut()
        && let Some(result) = image.preview.take()
    {
        image.result = Some(result.and_then(|image| {
            images
                .as_deref_mut()
                .map(|images| images.add(image))
                .ok_or_else(|| "Image rendering is unavailable.".into())
        }));
        redraw |= tablet.view == Page::Image;
    }
    if tablet.view == Page::Video
        && let Some(video) = tablet.video.as_mut()
        && let Some(result) = video.pending.as_ref().and_then(native_github::Job::take)
    {
        video.pending = None;
        match result {
            Ok(video) => tablet.playing_videos.push(video),
            Err(error) => video.status = Err(error),
        }
        redraw = true;
    }
    if tablet.view == Page::Search && tablet.panel.is_some() {
        let query = tablet
            .search_input
            .and_then(|entity| fields.get(entity).ok())
            .map(|field| field.value().to_string());
        let edited = query.as_ref().is_some_and(|query| query != &tablet.search);
        if let Some(query) = query.filter(|_| edited) {
            tablet.search = query;
        }
        let submit = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])
            && focus.get() == tablet.search_input;
        let changed = edited || repo.is_changed() || previous_page != Page::Search;
        let query = tablet.search.clone();
        let results_changed =
            tablet
                .search_text
                .update(&repo.0, &query, time.elapsed_secs_f64(), changed, submit);
        if results_changed && let Some(list) = tablet.search_list {
            let mut list = commands.entity(list);
            if edited {
                list.insert(ScrollPosition::default());
            }
            list.despawn_children()
                .with_children(|parent| draw_search_results(parent, &repo.0, &tablet));
        }
    }
    if !redraw {
        if tablet.view == Page::Trace {
            redraw = trace
                .as_ref()
                .is_some_and(|trace| tablet.trace_version != trace.version);
        }
        if tablet.view == Page::Connections
            && let Some(links) = &links
        {
            redraw = tablet.links_version != links.version();
        }
    }
    if !redraw {
        return;
    }
    let preserve_file_list = was_open
        && previous_page == Page::Files
        && tablet.view == Page::Files
        && !matches!(action, Some(Action::Filter(_) | Action::Page(_)));
    let focused_file = if preserve_file_list {
        tablet
            .file_buttons
            .iter()
            .find_map(|(path, button)| (focus.get() == Some(*button)).then(|| path.clone()))
    } else {
        None
    };
    tablet.files_scroll = if preserve_file_list {
        rows.single().map_or(0., |(scroll, _)| scroll.y)
    } else {
        0.
    };
    let files = if tablet.view == Page::Files {
        file_rows(
            &repo.0,
            &progress,
            &live,
            &tablet.times,
            tablet.file_filter,
            &tablet.file_activity,
        )
    } else {
        Vec::new()
    };
    tablet.page = tablet
        .page
        .min(files.len().div_ceil(FILES_PER_PAGE).saturating_sub(1));
    let (panel, first) = draw(
        &mut commands,
        &mut tablet,
        &files,
        time.elapsed_secs_f64(),
        links.as_deref(),
        audio.as_deref(),
        terminal.as_deref_mut(),
        &repo.0,
        trace.as_deref(),
        agent.as_deref().map(agent_session::Session::provider),
        party
            .as_deref()
            .is_some_and(review_party::Party::hosts_room),
        world.as_deref(),
    );
    tablet.panel = Some(panel);
    let focus_target = tablet
        .file_buttons
        .iter()
        .find_map(|(path, button)| (focused_file.as_ref() == Some(path)).then_some(*button))
        .unwrap_or(first);
    focus.set(focus_target, FocusCause::Navigated);
}

impl Tablet {
    fn save_reading(&mut self, reading: &mut reading::Settings) {
        self.status = match reading::Settings::with_inputs(
            self.mode,
            &self.bounds[0],
            &self.bounds[1],
            &self.bounds[2],
            &self.bounds[3],
        )
        .and_then(|mut next| {
            next.block_kind = self.block_kind;
            next.auto_advance = self.auto_advance;
            next.show_all_source = self.show_all_source;
            next.enabled = self.reading_enabled;
            next.save()?;
            Ok(next)
        }) {
            Ok(next) => {
                *reading = next;
                "Saved automatically.".into()
            }
            Err(error) => error,
        };
    }

    fn capture_bounds(&mut self, fields: &Query<&EditableText>) {
        if let Some(inputs) = self.inputs {
            for (field, value) in inputs.into_iter().zip(&mut self.bounds) {
                if let Ok(field) = fields.get(field) {
                    *value = field.value().to_string();
                }
            }
        }
    }
}

fn search_results(
    repo: &Repository,
    query: &str,
    content: Option<&content_search::Matches>,
) -> Vec<(usize, Option<content_search::FileMatch>)> {
    let query = query.trim().to_lowercase();
    let mut results = repo
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            let rank = file_picker::rank(&file.path, &query);
            let found = content.and_then(|matches| matches.get(&file.path)).copied();
            (rank.is_some() || found.is_some()).then_some((
                rank.unwrap_or(usize::MAX),
                index,
                found,
            ))
        })
        .collect::<Vec<_>>();
    results.sort_unstable_by_key(|&(rank, index, _)| (rank, index));
    results
        .into_iter()
        .map(|(_, index, found)| (index, found))
        .collect()
}

fn draw_search_results(parent: &mut ChildSpawnerCommands, repo: &Repository, tablet: &Tablet) {
    let content = tablet
        .search_text
        .result
        .as_ref()
        .and_then(|result| result.as_ref().ok());
    let results = search_results(repo, &tablet.search, content);
    let status = match &tablet.search_text.result {
        Some(Err(error)) => Some(error.as_str()),
        None if results.is_empty() && !tablet.search.trim().is_empty() => Some("Searching…"),
        _ if results.is_empty() => Some("No matching files."),
        _ => None,
    };
    if let Some(status) = status {
        native_panel::text(parent, status, native_panel::INK);
    }
    for (row, &(index, found)) in results.iter().take(100).enumerate() {
        let file = &repo.files[index];
        let (label, action) = if let Some(found) = found {
            let unit = if found.count == 1 { "match" } else { "matches" };
            (
                format!("{} · {} {unit}", file.path, found.count),
                Action::VisitMatch {
                    index,
                    line: found.first_line,
                },
            )
        } else {
            (file.path.clone(), Action::Visit(index))
        };
        let entity =
            native_panel::button::<Action, Tablet>(parent, &label, native_panel::INK, action);
        parent
            .commands()
            .entity(entity)
            .insert(TabIndex(100 + row as i32));
    }
    if results.len() > 100 {
        native_panel::text(
            parent,
            &format!("Showing 100 of {} files", results.len()),
            native_panel::INK,
        );
    }
}

fn draw(
    commands: &mut Commands,
    tablet: &mut Tablet,
    files: &[FileRow<'_>],
    elapsed: f64,
    links: Option<&file_links::Links>,
    audio: Option<&city_audio::Settings>,
    mut terminal: Option<&mut terminal::Terminal>,
    repository: &Repository,
    trace: Option<&trace_import::Trace>,
    agent: Option<agent_engine::Provider>,
    hosts_room: bool,
    world: Option<&WorldSet>,
) -> (Entity, Entity) {
    let title = match tablet.view {
        Page::Files if repository.is_worktree() => "Live worktree",
        Page::Files => "Changed files",
        Page::Help => "Help",
        Page::Settings => "Settings",
        Page::Connections => "File connections",
        Page::Image => tablet
            .image
            .as_ref()
            .map_or("Image", |image| image.preview.path.as_str()),
        Page::Video => tablet
            .video
            .as_ref()
            .map_or("Video", |video| video.path.as_str()),
        Page::Document => tablet
            .document
            .as_ref()
            .map_or("Markdown", |document| document.path.as_str()),
        Page::Git => "Git history",
        Page::Search => "Find in repository",
        Page::Terminal => "Terminal",
        Page::Trace => "Paste a trace",
        Page::Agent => "Chat",
    };
    let chrome_title = if matches!(tablet.view, Page::Agent | Page::Terminal) {
        ""
    } else {
        title
    };
    let panel = if let Some(panel) = tablet.panel {
        native_panel::refresh_tablet(commands, panel, chrome_title);
        panel
    } else {
        native_panel::tablet(commands, chrome_title)
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let first_row = tablet.page * FILES_PER_PAGE;
    let mut tab = 0;
    let mut first = None;
    tablet.inputs = None;
    tablet.file_buttons.clear();
    commands.entity(panel).with_children(|parent| {
        parent
            .spawn(Node {
                column_gap: px(6.),
                width: percent(100.),
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::bottom(px(9.)),
                border: UiRect::bottom(px(1.)),
                ..default()
            })
            .insert(BorderColor::all(Color::srgb_u8(31, 64, 70)))
            .with_children(|bar| {
                for (view, label) in Page::NAVIGATION {
                    let entity = add_button(bar, label, Action::Switch(view), &mut tab);
                    first.get_or_insert(entity);
                    bar.commands().entity(entity).insert(Node {
                        min_height: px(32.),
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(11.), px(5.)),
                        border: UiRect::bottom(px(2.)),
                        ..default()
                    });
                    if view == tablet.view {
                        bar.commands().entity(entity).insert((
                            BackgroundColor(Color::srgb_u8(19, 60, 68)),
                            BorderColor::all(Color::srgb_u8(117, 225, 205)),
                        ));
                    } else {
                        bar.commands()
                            .entity(entity)
                            .insert((BackgroundColor(Color::NONE), BorderColor::all(Color::NONE)));
                    }
                }
                let mut font = native_panel::panel_font();
                font.font_size = bevy::text::FontSize::Px(27.);
                let settings = native_panel::button_with_font::<_, Tablet>(
                    bar,
                    "⚙",
                    &font,
                    native_panel::INK,
                    Action::Switch(Page::Settings),
                );
                bar.commands().entity(settings).insert((
                    TabIndex(tab),
                    Node {
                        margin: UiRect::left(Val::Auto),
                        min_width: px(48.),
                        min_height: px(42.),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ));
                tab += 1;
            });
        match tablet.view {
            Page::Agent => {
                first = Some(if tablet.chat_room.is_some() {
                    review_party::chat::spawn(parent, hosts_room)
                } else {
                    let agy = terminal.as_deref_mut().filter(|_| tablet.agy_chat);
                    agent_session::spawn(parent, agy, &repository.root)
                });
                return;
            }
            Page::Trace => {
                first = Some(draw_trace(parent, tablet, trace, &mut tab));
                return;
            }
            Page::Search => {
                let input = native_panel::input(parent, "Path or text", &tablet.search, tab, false);
                tablet.search_input = Some(input);
                first = Some(input);
                tablet.search_list = Some(
                    parent
                        .spawn((
                            FileRows,
                            ScrollPosition::default(),
                            Node {
                                flex_direction: FlexDirection::Column,
                                flex_grow: 1.,
                                min_height: px(0.),
                                row_gap: px(6.),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|list| draw_search_results(list, repository, tablet))
                        .id(),
                );
                return;
            }
            Page::Git => {
                draw_git(parent, tablet.history.as_ref(), &mut tab);
                return;
            }
            Page::Terminal => {
                if let Some(terminal) = terminal.as_deref_mut() {
                    first = Some(terminal.draw(parent, &repository.root));
                } else {
                    native_panel::text(parent, "Terminal unavailable.", native_panel::INK);
                }
                return;
            }
            Page::Document => {
                if let Some(editor) =
                    draw_document(parent, tablet.document.as_mut(), repository, &mut tab)
                {
                    first = Some(editor);
                }
                return;
            }
            Page::Settings => {
                first = Some(draw_settings(parent, tablet, &mut tab, audio, agent));
                return;
            }
            Page::Connections => {
                tablet.links_version = links.map_or(0, file_links::Links::version);
                draw_connections(parent, links, &mut tab);
                return;
            }
            Page::Image => {
                parent
                    .spawn(Node {
                        flex_grow: 1.,
                        min_height: px(0.),
                        width: percent(100.),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|body| {
                        match tablet
                            .image
                            .as_ref()
                            .and_then(|image| image.result.as_ref())
                        {
                            Some(Ok(image)) => {
                                body.spawn((
                                    ImageNode::new(image.clone()),
                                    Node {
                                        max_width: percent(100.),
                                        max_height: percent(100.),
                                        ..default()
                                    },
                                    bevy::picking::Pickable::IGNORE,
                                ));
                            }
                            Some(Err(error)) => native_panel::text(body, error, native_panel::INK),
                            None => native_panel::text(
                                body,
                                if tablet.status.is_empty() {
                                    "Loading image…"
                                } else {
                                    &tablet.status
                                },
                                native_panel::INK,
                            ),
                        }
                    });
                return;
            }
            Page::Video => {
                let message = match &tablet.video {
                    Some(video) if video.pending.is_some() => "Opening video…",
                    Some(video) => video
                        .status
                        .as_ref()
                        .err()
                        .map_or("Opened in the system video player.", String::as_str),
                    None => "Video is unavailable.",
                };
                native_panel::text(parent, message, native_panel::INK);
                return;
            }
            Page::Help => {
                parent
                    .spawn((
                        FileRows,
                        ScrollPosition::default(),
                        Node {
                            min_height: px(0.),
                            flex_grow: 1.,
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(|body| guide::draw_controls(body, world));
                if world.is_some_and(WorldSet::is_abyss) {
                    return;
                }
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        ..default()
                    })
                    .with_children(|actions| {
                        first = Some(add_button(
                            actions,
                            "Start tour",
                            Action::StartTour,
                            &mut tab,
                        ));
                    });
                return;
            }
            Page::Files => {}
        }
        if !repository.is_worktree() {
            let revision = format!(
                "Comparison · {}",
                repository.revision.chars().take(8).collect::<String>()
            );
            native_panel::text(parent, &revision, native_panel::INK);
        }
        parent
            .spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6.),
                row_gap: px(6.),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            },))
            .with_children(|bar| {
                for (label, filter) in [
                    ("All changes", FileFilter::Changes),
                    ("Viewed", FileFilter::Viewed),
                    ("Agent files", FileFilter::Agent),
                ] {
                    let entity = add_button(bar, label, Action::Filter(filter), &mut tab);
                    if filter == tablet.file_filter {
                        bar.commands()
                            .entity(entity)
                            .insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                    }
                }
                add_button(bar, "Open canvas", Action::OpenCanvas, &mut tab);
                add_button(
                    bar,
                    "Choose files for canvas",
                    Action::ChooseCanvasFiles,
                    &mut tab,
                );
            });
        draw_file_pagination(
            parent,
            tablet.page,
            files.len().div_ceil(FILES_PER_PAGE),
            &mut tab,
        );
        parent
            .spawn((
                FileRows,
                ScrollPosition(Vec2::new(0., tablet.files_scroll)),
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.,
                    min_height: px(0.),
                    row_gap: px(4.),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                if files.is_empty() {
                    let message = match tablet.file_filter {
                        FileFilter::Changes => "No changed files.",
                        FileFilter::Viewed => "No changed files viewed.",
                        FileFilter::Agent => "No agent file activity in this checkout.",
                    };
                    native_panel::text(list, message, native_panel::INK);
                } else {
                    for file in files.iter().skip(first_row).take(FILES_PER_PAGE) {
                        let label = file.label(tablet.file_filter, elapsed, now);
                        let link = file.activity.and_then(|activity| {
                            agent_navigation::measured_link(
                                list,
                                repository,
                                &file.file.path,
                                activity.line,
                                &label,
                                AlignSelf::Auto,
                            )
                            .ok()
                        });
                        let button = if let Some(link) = link {
                            list.commands().entity(link).insert(TabIndex(tab));
                            tab += 1;
                            link
                        } else {
                            add_button(list, &label, Action::Visit(file.index), &mut tab)
                        };
                        if file.index == files[first_row].index {
                            first = Some(button);
                        }
                        tablet.file_buttons.push((file.file.path.clone(), button));
                    }
                }
            });
    });
    (panel, first.expect("the tablet always has navigation"))
}

fn draw_file_pagination(
    parent: &mut ChildSpawnerCommands,
    page: usize,
    pages: usize,
    tab: &mut i32,
) {
    if pages <= 1 {
        return;
    }
    parent
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![GridTrack::flex(1.); 3],
            align_items: AlignItems::Center,
            flex_shrink: 0.,
            ..default()
        })
        .with_children(|bar| {
            bar.spawn(Node {
                grid_column: GridPlacement::start(1),
                ..default()
            })
            .with_children(|slot| {
                if page > 0 {
                    add_button(slot, "Previous", Action::Page(page - 1), tab);
                }
            });
            bar.spawn(Node {
                grid_column: GridPlacement::start(2),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|slot| {
                native_panel::text(slot, &format!("{} / {pages}", page + 1), native_panel::INK);
            });
            bar.spawn(Node {
                grid_column: GridPlacement::start(3),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|slot| {
                if page + 1 < pages {
                    add_button(slot, "Next", Action::Page(page + 1), tab);
                }
            });
        });
}

fn draw_document(
    parent: &mut ChildSpawnerCommands,
    document: Option<&mut Document>,
    repository: &Repository,
    tab: &mut i32,
) -> Option<Entity> {
    let document = document?;
    if let Some(Ok(loaded)) = &document.result {
        let source = &loaded.source;
        let end = document.range.end.min(loaded.mark.rows);
        let range = document.range.start..end;
        let permitted = source_editor::chunk_bytes(source, range.clone())
            .is_ok_and(|bytes| bytes.len() <= 256 * 1024);
        let button = add_button(
            parent,
            if document.editing { "Preview" } else { "Edit" },
            Action::EditDocument,
            tab,
        );
        if !permitted {
            parent
                .commands()
                .entity(button)
                .insert(bevy::ui::InteractionDisabled);
            native_panel::text(
                parent,
                "Editing is limited to 256 KiB. The full file remains available in preview.",
                native_panel::INK,
            );
        }
        if document.editing && permitted {
            let identity = source_editor::DraftIdentity {
                repository: repository.root.to_string_lossy().into_owned(),
                revision: repository.revision.clone(),
                baseline: repository.baseline.clone(),
                path: document.path.clone(),
                blob: loaded.mark.blob.clone(),
                before: document.before,
                range: range.clone(),
            };
            let apply = (repository.is_worktree() && !document.before).then(|| {
                source_editor::ApplyTarget {
                    root: repository.root.clone(),
                    path: document.path.clone(),
                    expected: source.clone(),
                    bytes: source_editor::chunk_bytes(source, range.clone()).unwrap_or(0..0),
                }
            });
            return Some(source_editor::spawn(parent, identity, source, apply));
        }
    }
    let edited_offsets = document
        .edited_source
        .as_deref()
        .map(markdown_preview::page_offsets);
    let offsets = edited_offsets
        .as_deref()
        .or_else(|| match document.result.as_ref() {
            Some(Ok(loaded)) => Some(loaded.offsets.as_slice()),
            _ => None,
        });
    if let Some(offsets) = offsets.filter(|offsets| offsets.len() > 2) {
        let pages = offsets.len() - 1;
        let page = document.page;
        parent
            .spawn(Node {
                column_gap: px(8.),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|bar| {
                let previous = add_button(
                    bar,
                    "Previous page",
                    Action::DocumentPage(page.saturating_sub(1)),
                    tab,
                );
                if page == 0 {
                    bar.commands()
                        .entity(previous)
                        .insert(bevy::ui::InteractionDisabled);
                }
                native_panel::text(
                    bar,
                    &format!("Page {} of {pages}", page + 1),
                    native_panel::INK,
                );
                let next = add_button(
                    bar,
                    "Next page",
                    Action::DocumentPage((page + 1).min(pages - 1)),
                    tab,
                );
                if page + 1 == pages {
                    bar.commands()
                        .entity(next)
                        .insert(bevy::ui::InteractionDisabled);
                }
            });
    }
    document.content = None;
    parent
        .spawn((
            FileRows,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                min_height: px(0.),
                flex_grow: 1.,
                ..default()
            },
        ))
        .with_children(|body| match document.result.as_ref() {
            Some(Ok(loaded)) => {
                let offsets = offsets.expect("a loaded document has page offsets");
                let page = document.page;
                let text = document.edited_source.as_ref().unwrap_or(&loaded.source);
                let text = &text[offsets[page]..offsets[page + 1]];
                if text.trim().is_empty() {
                    native_panel::text(body, "Empty document.", native_panel::INK);
                } else {
                    let html = markdown_preview::document_html(&document.path, text);
                    let source = markdown_preview::SourceFile {
                        path: document.path.clone(),
                        before: document.before,
                    };
                    document.content = Some(markdown_preview::draw(body, html, Some(source)));
                }
            }
            Some(Err(error)) => native_panel::text(body, error, native_panel::INK),
            None => native_panel::text(body, "Loading document…", native_panel::INK),
        });
    None
}

fn draw_git(parent: &mut ChildSpawnerCommands, history: Option<&History>, tab: &mut i32) {
    let Some(history) = history else {
        return;
    };
    native_panel::text(
        parent,
        &format!(
            "Before {} → After {}",
            history.before.as_deref().map_or("—", |id| &id[..8]),
            history.after.as_deref().map_or("—", |id| &id[..8])
        ),
        native_panel::INK,
    );
    let compare = add_button(parent, "Compare selected commits", Action::GitCompare, tab);
    if history.before.is_none() || history.after.is_none() {
        parent
            .commands()
            .entity(compare)
            .insert(bevy::ui::InteractionDisabled);
    }
    parent
        .spawn(Node {
            column_gap: px(8.),
            ..default()
        })
        .with_children(|tools| {
            if history.detail.is_some() || history.showing.is_some() {
                add_button(tools, "Back to log", Action::GitBack, tab);
            } else {
                add_button(tools, "Refresh", Action::GitPage(history.skip), tab);
                let newer = add_button(
                    tools,
                    "Newer",
                    Action::GitPage(history.skip.saturating_sub(100)),
                    tab,
                );
                if history.skip == 0 {
                    tools
                        .commands()
                        .entity(newer)
                        .insert(bevy::ui::InteractionDisabled);
                }
                if history.commits.as_ref().is_ok_and(|rows| rows.len() == 100) {
                    add_button(tools, "Older", Action::GitPage(history.skip + 100), tab);
                }
            }
        });
    parent
        .spawn((
            FileRows,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.,
                min_height: px(0.),
                row_gap: px(8.),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|body| {
            if history.pending.is_some() {
                native_panel::text(body, "Loading commit history…", native_panel::INK);
            } else if history.showing.is_some() {
                native_panel::text(body, "Loading commit…", native_panel::INK);
            } else if let Some(detail) = &history.detail {
                match detail {
                    Ok(text) => {
                        markdown_preview::draw(
                            body,
                            markdown_preview::document_html("commit.diff", text),
                            None,
                        );
                    }
                    Err(error) => native_panel::text(body, error, native_panel::INK),
                }
            } else {
                match &history.commits {
                    Err(error) => {
                        native_panel::text(body, "Git is unavailable", native_panel::INK);
                        native_panel::text(body, error, native_panel::INK);
                    }
                    Ok(commits) if commits.is_empty() => {
                        native_panel::text(body, "No commits in this history.", native_panel::INK)
                    }
                    Ok(commits) => {
                        for (index, commit) in commits.iter().enumerate() {
                            let label = format!(
                                "{} · {} · {}\n{}",
                                &commit.id[..8],
                                commit.date.get(..10).unwrap_or(&commit.date),
                                commit.author,
                                commit.summary
                            );
                            add_button(body, &label, Action::GitShow(index), tab);
                            body.spawn(Node {
                                column_gap: px(8.),
                                ..default()
                            })
                            .with_children(|row| {
                                add_button(
                                    row,
                                    if history.before.as_deref() == Some(commit.id.as_str()) {
                                        "Before selected"
                                    } else {
                                        "Before"
                                    },
                                    Action::GitBefore(index),
                                    tab,
                                );
                                add_button(
                                    row,
                                    if history.after.as_deref() == Some(commit.id.as_str()) {
                                        "After selected"
                                    } else {
                                        "After"
                                    },
                                    Action::GitAfter(index),
                                    tab,
                                );
                            });
                        }
                    }
                }
            }
        });
}

fn settings_card(parent: &mut ChildSpawnerCommands, title: &str) -> Entity {
    let card = parent
        .spawn((
            Name::new(title.to_owned()),
            Node {
                width: percent(100.),
                padding: UiRect::all(px(18.)),
                row_gap: px(12.),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1.)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(10, 32, 40)),
            BorderColor::all(Color::srgb_u8(39, 71, 78)),
        ))
        .id();
    parent.commands().entity(card).with_child((
        Text::new(title),
        TextFont::from_font_size(18.),
        TextColor(native_panel::INK),
        bevy::picking::Pickable::IGNORE,
    ));
    card
}

fn draw_settings(
    parent: &mut ChildSpawnerCommands,
    tablet: &mut Tablet,
    tab: &mut i32,
    audio: Option<&city_audio::Settings>,
    agent: Option<agent_engine::Provider>,
) -> Entity {
    let mut first = None;
    let mut inputs = Vec::new();
    parent.spawn((FileRows, ScrollPosition::default(), Node { width: percent(100.), min_height: px(0.), flex_grow: 1., flex_direction: FlexDirection::Column, row_gap: px(16.), overflow: Overflow::scroll_y(), ..default() })).with_children(|body| {
        body.spawn(Node { width: percent(100.), flex_wrap: FlexWrap::Wrap, column_gap: px(20.), row_gap: px(20.), align_items: AlignItems::Start, ..default() }).with_children(|columns| {
            columns.spawn(Node { flex_basis: px(540.), flex_grow: 1.2, min_width: px(320.), flex_direction: FlexDirection::Column, ..default() }).with_children(|column| {
                let card = settings_card(column, "Reading");
                column.commands().entity(card).with_children(|form| {
                    add_button(form, if tablet.reading_enabled { "[x] Reading mode · F4" } else { "[ ] Reading mode · F4" }, Action::ReadingEnabled, tab);
                    native_panel::text(form, "When off, source stays visible and reading timers pause. Saved progress is preserved.", native_panel::INK);
                    add_button(form, if tablet.show_all_source { "Source: always visible" } else { "Source: thaw to reveal" }, Action::ShowSource, tab);
                    native_panel::text(form, "Reading progress is timed separately from source visibility.", native_panel::INK);
                    native_panel::text(form, "Read credit", native_panel::INK);
                    form.spawn(Node { column_gap: px(6.), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                        for (label, mode) in [("Delayed", reading::Mode::Delayed), ("Immediate", reading::Mode::Immediate)] {
                            let entity = add_button(row, label, Action::Mode(mode), tab);
                            if mode == tablet.mode { row.commands().entity(entity).insert(BackgroundColor(Color::srgb_u8(23, 79, 87))); first = Some(entity); }
                        }
                    });
                    native_panel::text(form, "Block", native_panel::INK);
                    form.spawn(Node { column_gap: px(6.), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                        for (label, kind) in [("Fixed lines", reading::BlockKind::Lines), ("Whole function", reading::BlockKind::Function)] {
                            let entity = add_button(row, label, Action::Block(kind), tab);
                            if kind == tablet.block_kind { row.commands().entity(entity).insert(BackgroundColor(Color::srgb_u8(23, 79, 87))); }
                        }
                    });
                    let input = native_panel::input(form, "Lines per reading block", &tablet.bounds[0], *tab, false); *tab += 1; inputs.push(input);
                    native_panel::text(form, "Used for fixed blocks and when a function boundary is unavailable.", native_panel::INK);
                    add_button(form, if tablet.auto_advance { "Auto-advance: on" } else { "Auto-advance: off" }, Action::AutoAdvance, tab);
                    form.spawn(Node { column_gap: px(12.), width: percent(100.), ..default() }).with_children(|limits| {
                        for (index, label) in [(1, "Minimum delay (seconds)"), (2, "Maximum delay (seconds)")] {
                            limits.spawn(Node { flex_basis: percent(50.), flex_grow: 1., min_width: px(0.), flex_direction: FlexDirection::Column, row_gap: px(6.), ..default() }).with_children(|field| {
                                inputs.push(native_panel::input(field, label, &tablet.bounds[index], *tab, false)); *tab += 1;
                            });
                        }
                    });
                    native_panel::text(form, "Leave a limit blank to use the calculated time. Limits and block size are independent.", native_panel::INK);
                    let input = native_panel::input(form, "City fly-to font size (px)", &tablet.bounds[3], *tab, false); *tab += 1; inputs.push(input);
                    let status = form.spawn((Text::new(if tablet.status.is_empty() { "Changes save automatically." } else { &tablet.status }), native_panel::panel_font(), TextColor(native_panel::INK), Node { min_height: px(24.), ..default() })).id();
                    tablet.settings_status = Some(status);
                });
            });
            columns.spawn(Node { flex_basis: px(420.), flex_grow: 1., min_width: px(300.), flex_direction: FlexDirection::Column, row_gap: px(20.), ..default() }).with_children(|column| {
                if let Some(selected) = agent {
                    let card = settings_card(column, "Agent provider");
                    column.commands().entity(card).with_children(|form| {
                        form.spawn(Node { column_gap: px(6.), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                            for provider in agent_engine::PROVIDERS {
                                let entity = add_button(row, provider.label, Action::AgentProvider(provider), tab);
                                if provider == selected {
                                    row.commands().entity(entity).insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                                }
                            }
                        });
                        native_panel::text(form, "Voice is available with Codex.", native_panel::INK);
                        agent_session::settings(form, selected);
                    });
                }
                if let Some(audio) = audio {
                    let card = settings_card(column, "Sound");
                    column.commands().entity(card).with_children(|form| {
                        add_button(form, if audio.muted { "[ ] Sound · F10" } else { "[x] Sound · F10" }, Action::MuteAudio, tab);
                        form.spawn(Node { column_gap: px(6.), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
                            add_button(row, if audio.ambience { "City sounds: on" } else { "City sounds: off" }, Action::Ambience, tab);
                            add_button(row, if audio.speech { "Robot voice: on" } else { "Robot voice: off" }, Action::Speech, tab);
                            add_button(row, if audio.file_changes { "File changes: on" } else { "File changes: off" }, Action::FileChangeSound, tab);
                        });
                        native_panel::text(form, "Volume", native_panel::INK);
                        form.spawn(Node { column_gap: px(6.), ..default() }).with_children(|row| {
                            for (label, volume) in [("Quiet", 0.18), ("Normal", 0.4), ("Mute", 0.)] {
                                let entity = add_button(row, label, Action::Volume(volume), tab);
                                if (audio.volume - volume).abs() < 0.01 { row.commands().entity(entity).insert(BackgroundColor(Color::srgb_u8(23, 79, 87))); }
                            }
                        });
                        add_button(form, &format!("System voice: {}", audio.voice), Action::Voice, tab);
                        add_button(form, &format!("Radio: {}", audio.radio.label()), Action::Radio, tab);
                        native_panel::text(form, "File changes plays one short chime for local saves, additions or deletions. Off by default; waits until narration or the voice conversation ends.", native_panel::INK);
                        native_panel::text(form, "Reads the visible block from start to end. Tool sound is muted; ambience and radio become quieter during narration. City audio stays quiet during an agent voice conversation. Radio music is synthesized on this device.", native_panel::INK);
                    });
                }
                let card = settings_card(column, "Reading estimate");
                column.commands().entity(card).with_children(|note| {
                    native_panel::text(note, "seconds = characters × 60 / (238 × 4.6)", native_panel::INK);
                    native_panel::text(note, "Counts non-whitespace characters; consecutive copies count once. Six lines is a configurable preset, not a measured optimum.", native_panel::INK);
                    native_panel::text(note, "Brysbaert (2019) measured English nonfiction, not code comprehension. This length-adjusted estimate is an approximation.", native_panel::INK);
                });
            });
        });
    });
    tablet.inputs = Some(inputs.try_into().expect("four reading inputs"));
    first.expect("a reading mode is selected")
}

fn draw_connections(
    parent: &mut ChildSpawnerCommands,
    links: Option<&file_links::Links>,
    tab: &mut i32,
) {
    parent
        .spawn((
            FileRows,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.,
                min_height: px(0.),
                row_gap: px(8.),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|list| {
            let Some(links) = links else { return };
            native_panel::text(list, links.status(), native_panel::INK);
            if let Some(file) = links.file() {
                native_panel::text(list, &file.path, native_panel::INK);
                native_panel::text(list, "Tree-sitter resolves static imports and modules. Roads show these file links; they do not imply function calls.", native_panel::INK);
                for (index, link) in file.links.iter().enumerate() {
                    let symbol = link
                        .symbol
                        .as_ref()
                        .map_or(String::new(), |name| format!(" · {name}"));
                    let label = format!("Import/module at line {} · {}{symbol}", link.line, link.import);
                    native_panel::text(list, &label, native_panel::INK);
                    if let Some(target) = &link.destination {
                        let label = if target.exact {
                            format!("{}:{}", target.path, target.line)
                        } else {
                            format!("{} · module start", target.path)
                        };
                        add_button(list, &label, Action::Connection(index), tab);
                    } else {
                        native_panel::text(
                            list,
                            link.reason.as_deref().unwrap_or("Unresolved import"),
                            Color::srgb_u8(157, 193, 196),
                        );
                    }
                }
            }
        });
}

fn draw_trace(
    parent: &mut ChildSpawnerCommands,
    tablet: &mut Tablet,
    trace: Option<&trace_import::Trace>,
    tab: &mut i32,
) -> Entity {
    tablet.trace_version = trace.map_or(0, |trace| trace.version);
    native_panel::text(
        parent,
        "Paste a stack trace, OpenTelemetry OTLP JSON or Chrome Trace JSON. Source stays on this device.",
        native_panel::INK,
    );
    let input = native_panel::input(
        parent,
        "Stack trace",
        trace.map_or("", |trace| trace.source.as_str()),
        *tab,
        true,
    );
    parent.commands().queue(move |world: &mut World| {
        if let Some(mut field) = world.get_mut::<EditableText>(input) {
            field.max_characters = Some(65536);
        }
        if let Some(mut node) = world.get_mut::<Node>(input) {
            node.height = px(180.);
        }
    });
    tablet.trace_input = Some(input);
    *tab += 1;
    parent
        .spawn(Node {
            column_gap: px(8.),
            ..default()
        })
        .with_children(|bar| {
            add_button(bar, "Build route", Action::TraceImport, tab);
            let follow = add_button(bar, "Follow trace", Action::TraceFollow, tab);
            tablet.trace_follow = Some(follow);
            if trace.is_none_or(|trace| !trace.can_follow()) {
                bar.commands()
                    .entity(follow)
                    .insert(bevy::ui::InteractionDisabled);
            }
        });
    native_panel::text(
        parent,
        "Up to 64 KiB and 128 frames or spans. Locations are checked against the selected snapshot when opened.",
        native_panel::INK,
    );
    parent
        .spawn((
            FileRows,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.,
                min_height: px(0.),
                width: percent(100.),
                row_gap: px(6.),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|body| {
            if let Some(trace) = trace {
                native_panel::text(
                    body,
                    &format!(
                        "{} frames · {} unresolved locations · {} ignored entries",
                        trace.report.frames.len(),
                        trace.report.issues.len(),
                        trace.report.ignored
                    ),
                    native_panel::INK,
                );
                if !trace.report.error.is_empty() {
                    native_panel::text(body, &trace.report.error, native_panel::INK);
                }
                native_panel::text(body, &trace.report.format, native_panel::INK);
                if trace.report.groups.len() > 1 {
                    native_panel::text(
                        body,
                        "Choose one trace or thread to follow.",
                        native_panel::INK,
                    );
                    for (index, group) in trace.report.groups.iter().enumerate() {
                        let button =
                            add_button(body, &group.label, Action::TraceSelect(index), tab);
                        if trace.selected.as_deref() == Some(group.id.as_str()) {
                            body.commands()
                                .entity(button)
                                .insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                        }
                    }
                }
                for frame in trace.frames() {
                    native_panel::text(body, &frame.description(), native_panel::INK);
                }
                for issue in &trace.report.issues {
                    native_panel::text(body, issue, native_panel::INK);
                }
            }
        });
    input
}

fn add_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: Action,
    tab: &mut i32,
) -> Entity {
    let entity = native_panel::button::<_, Tablet>(parent, label, native_panel::INK, action);
    parent.commands().entity(entity).insert(TabIndex(*tab));
    *tab += 1;
    entity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_renders_one_file_row_for_a_path_and_contents_match() {
        use bevy::ecs::system::RunSystemOnce;

        let mut repo = repository_picker::empty();
        for path in ["cart.rs", "needle.rs", "needle-notes.md", "unrelated.rs"] {
            repo.files.push(repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: repo_world::Change::Unchanged,
                position: [0., 0.],
            });
        }
        let content = [
            (
                "needle.rs".into(),
                content_search::FileMatch {
                    first_line: 2,
                    count: 3,
                },
            ),
            (
                "cart.rs".into(),
                content_search::FileMatch {
                    first_line: 9,
                    count: 1,
                },
            ),
        ]
        .into();
        let rows = search_results(&repo, "needle", Some(&content));
        assert_eq!(
            rows,
            [
                (1, Some(content["needle.rs"])),
                (2, None),
                (0, Some(content["cart.rs"]))
            ]
        );
        let mut app = App::new();
        let world = app.world_mut();
        world
            .run_system_once(move |mut commands: Commands| {
                let tablet = Tablet {
                    search: "needle".into(),
                    search_text: TextSearch {
                        result: Some(Ok(content.clone())),
                        ..default()
                    },
                    ..default()
                };
                commands
                    .spawn_empty()
                    .with_children(|list| draw_search_results(list, &repo, &tablet));
            })
            .unwrap();
        let labels: Vec<_> = world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "needle.rs · 3 matches",
                "needle-notes.md",
                "cart.rs · 1 match"
            ]
        );
        assert!(
            world
                .query::<&Action>()
                .iter(world)
                .any(|action| matches!(action, Action::VisitMatch { index: 1, line: 2 }))
        );
    }

    #[test]
    fn live_worktree_uses_one_scroll_area_with_a_full_heading() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        let world = app.world_mut();
        world
            .run_system_once(|mut commands: Commands| {
                let mut repo = repository_picker::empty();
                repo.revision = "worktree:fixture".into();
                draw(
                    &mut commands,
                    &mut Tablet::default(),
                    &[],
                    0.,
                    None,
                    None,
                    None,
                    &repo,
                    None,
                    None,
                    false,
                    None,
                );
            })
            .unwrap();

        assert_eq!(world.query::<&tablet_scroll::Area>().iter(world).count(), 1);
        let heading = world
            .query_filtered::<&Text, With<native_panel::Heading>>()
            .single(world)
            .unwrap();
        assert_eq!(heading.0, "Live worktree");
        let mut actions = world.query::<&Action>();
        assert!(
            actions
                .iter(world)
                .any(|action| matches!(action, Action::OpenCanvas))
        );
        assert!(
            actions
                .iter(world)
                .any(|action| matches!(action, Action::ChooseCanvasFiles))
        );
    }

    #[test]
    fn file_pager_stays_above_the_list_on_first_middle_and_last_pages() {
        use bevy::ecs::system::RunSystemOnce;

        for page in [0, 1, 3] {
            let mut app = App::new();
            let world = app.world_mut();
            let panel = world
                .run_system_once(move |mut commands: Commands| {
                    let mut repo = repository_picker::empty();
                    let mut tablet = Tablet { page, ..default() };
                    repo.files = (0..32)
                        .map(|index| repo_world::File {
                            path: format!("file_{index}.rs"),
                            before: None,
                            after: None,
                            change: Change::Added,
                            position: [0., 0.],
                        })
                        .collect();
                    let files = repo
                        .files
                        .iter()
                        .enumerate()
                        .map(|(index, file)| FileRow {
                            index,
                            file,
                            freshness: Freshness::Unknown,
                            activity: None,
                        })
                        .collect::<Vec<_>>();
                    draw(
                        &mut commands,
                        &mut tablet,
                        &files,
                        0.,
                        None,
                        None,
                        None,
                        &repo,
                        None,
                        None,
                        false,
                        None,
                    )
                    .0
                })
                .unwrap();
            let list = world
                .query_filtered::<Entity, With<FileRows>>()
                .single(world)
                .unwrap();
            let children = world.get::<Children>(panel).unwrap();
            assert_eq!(children.last(), Some(&list));
            let pager = children[children.len() - 2];
            assert_eq!(
                world.get::<Node>(pager).unwrap().grid_template_columns,
                vec![GridTrack::flex(1.); 3]
            );
            let slots = world.get::<Children>(pager).unwrap();
            assert_eq!(slots.len(), 3);
            let previous = world
                .get::<Children>(slots[0])
                .is_some_and(|children| !children.is_empty());
            let next = world
                .get::<Children>(slots[2])
                .is_some_and(|children| !children.is_empty());
            assert_eq!(previous, page > 0);
            assert_eq!(next, page < 3);
        }
    }

    #[test]
    fn function_keys_toggle_reading_and_sound_without_opening_the_tablet() {
        // Run preference writes in a child with isolated storage, never the player's settings.
        const CHILD: &str = "REPO_CITY_KEY_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory =
                std::env::temp_dir().join(format!("repo-city-key-test-{}", std::process::id()));
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "review_tablet::tests::function_keys_toggle_reading_and_sound_without_opening_the_tablet"])
                .env(CHILD, "1").env("VIBE_CITY_CONFIG_DIR", &directory)
                .status().unwrap();
            let _ = std::fs::remove_dir_all(directory);
            assert!(status.success());
            return;
        }
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Tablet>()
            .init_resource::<guide::Tour>()
            .init_resource::<reading::Settings>()
            .init_resource::<city_audio::Settings>()
            .init_resource::<review_notes::Review>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<live_worktree::Live>()
            .init_resource::<progress::Progress>()
            .init_resource::<listing::find::Search>()
            .init_resource::<InputFocus>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Repo(repository_picker::empty()))
            .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
            .add_systems(Update, update);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        assert!(!app.world().resource::<reading::Settings>().enabled);
        assert!(!app.world().resource::<Tablet>().is_open());
        let editor = app.world_mut().spawn(EditableText::default()).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(editor, FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        assert!(
            !app.world().resource::<reading::Settings>().enabled,
            "F4 must not steal editor input"
        );
        let volume = app.world().resource::<city_audio::Settings>().volume;
        for muted in [true, false] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F10);
            app.update();
            let audio = app.world().resource::<city_audio::Settings>();
            assert_eq!(audio.muted, muted);
            assert_eq!(audio.volume, volume, "mute must preserve the chosen volume");
        }
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F10);
        app.update();
        assert!(!app.world().resource::<city_audio::Settings>().muted);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F4);
        app.update();
        assert!(app.world().resource::<reading::Settings>().enabled);
        assert!(reading::Settings::load().enabled);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<Tablet>()
            .open_page(Page::Settings);
        app.update();
        let mut actions = app.world_mut().query::<&Action>();
        assert!(
            actions
                .iter(app.world())
                .any(|action| matches!(action, Action::ReadingEnabled))
        );
        app.world_mut().resource_mut::<Tablet>().action = Some(Action::ReadingEnabled);
        app.update();
        assert!(!app.world().resource::<reading::Settings>().enabled);
    }

    #[test]
    fn document_edit_renders_only_the_selected_chunk_with_original_source_identity() {
        // Given a text preview, Edit mounts a source widget for its canonical reading chunk.
        let source = "PREFIX\nfn chosen() {\n  work();\n}\nUNSEEN_TAIL\n";
        let mut repository = repository_picker::empty();
        repository.root = PathBuf::from("/native-editor-fixture");
        repository.revision = "immutable-commit".into();
        let mut document = Document {
            path: "sample.rs".into(),
            before: false,
            range: 1..4,
            editing: true,
            result: Some(Ok(LoadedDocument {
                mark: markdown_preview::ReadDocument {
                    path: "sample.rs".into(),
                    blob: format!("test-{}", std::process::id()),
                    before: false,
                    rows: 5,
                },
                source: source.into(),
                offsets: markdown_preview::page_offsets(source),
            })),
            ..default()
        };
        let mut app = App::new();
        app.init_resource::<Tablet>();
        let root = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .commands()
            .entity(root)
            .with_children(|parent| {
                assert!(draw_document(parent, Some(&mut document), &repository, &mut 0).is_some());
            });
        app.world_mut().flush();
        let mut query = app.world_mut().query::<&source_editor::View>();
        let editor = query.single(app.world()).unwrap();
        assert_eq!(editor.value(), "fn chosen() {\n  work();\n}\n");
        assert_eq!(editor.range(), 1..4);
        assert_eq!(
            editor.full_value(),
            source,
            "opening Edit must not mutate source outside its chunk"
        );
    }

    #[test]
    fn pasted_trace_stays_editable_after_failure_and_starts_from_worktrees_or_commits() {
        for revision in ["worktree:head", "commit-sha"] {
            let mut repository = repository_picker::empty();
            repository.revision = revision.into();
            repository.files = vec![repo_world::File {
                path: "lib/demo.rb".into(),
                before: None,
                after: Some(repo_world::Blob {
                    id: "blob".into(),
                    size: 100,
                    mode: "100644".into(),
                }),
                change: Change::Added,
                position: [0., 0.],
            }];
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .add_message::<KeyboardInput>()
                .init_resource::<AccumulatedMouseScroll>()
                .init_resource::<Tablet>()
                .init_resource::<guide::Tour>()
                .init_resource::<reading::Settings>()
                .init_resource::<review_notes::Review>()
                .init_resource::<file_picker::FilePicker>()
                .init_resource::<city_menu::Menu>()
                .init_resource::<live_worktree::Live>()
                .init_resource::<progress::Progress>()
                .init_resource::<listing::find::Search>()
                .init_resource::<InputFocus>()
                .init_resource::<trace_import::Trace>()
                .insert_resource(Time::<()>::default())
                .insert_resource(Repo(repository))
                .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
                .add_systems(Update, update);
            app.world_mut()
                .resource_mut::<Tablet>()
                .open_page(Page::Trace);
            app.update();
            for source in ["missing.rb:1", "lib/demo.rb:12"] {
                let input = app.world().resource::<Tablet>().trace_input.unwrap();
                app.world_mut()
                    .get_mut::<EditableText>(input)
                    .unwrap()
                    .editor_mut()
                    .set_text(source);
                app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceImport);
                app.update();
                assert_eq!(app.world().resource::<trace_import::Trace>().source, source);
                assert_eq!(app.world().resource::<Tablet>().page_name(), Some("trace"));
            }
            let trace = app.world().resource::<trace_import::Trace>();
            assert_eq!(trace.report.frames[0].line, 12);
            app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceFollow);
            app.update();
            assert!(!app.world().resource::<Tablet>().is_open());
            assert!(app.world().resource::<trace_import::Trace>().start);
            // The same tablet requires choosing one JSON trace before following it.
            app.world_mut()
                .resource_mut::<review_notes::Review>()
                .blocked = false;
            app.world_mut()
                .resource_mut::<Tablet>()
                .open_page(Page::Trace);
            app.update();
            let source = serde_json::json!({"spans": [
                {"traceId":"a","spanId":"first","name":"first","attributes":{"code.file.path":"lib/demo.rb","code.line.number":12}},
                {"traceId":"b","spanId":"second","name":"second","attributes":{"code.file.path":"lib/demo.rb","code.line.number":19}}
            ]}).to_string();
            let input = app.world().resource::<Tablet>().trace_input.unwrap();
            app.world_mut()
                .get_mut::<EditableText>(input)
                .unwrap()
                .editor_mut()
                .set_text(&source);
            app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceImport);
            app.update();
            assert!(!app.world().resource::<trace_import::Trace>().can_follow());
            app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceFollow);
            app.update();
            assert!(app.world().resource::<Tablet>().is_open());
            app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceSelect(1));
            app.update();
            assert_eq!(
                app.world()
                    .resource::<trace_import::Trace>()
                    .frames()
                    .next()
                    .unwrap()
                    .line,
                19
            );
            app.world_mut().resource_mut::<Tablet>().action = Some(Action::TraceFollow);
            app.update();
            assert!(!app.world().resource::<Tablet>().is_open());
        }
    }

    #[test]
    fn help_and_search_shortcuts_preserve_text_input_and_open_in_vibepad() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .init_resource::<agent_session::Session>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Tablet>()
            .init_resource::<guide::Tour>()
            .init_resource::<reading::Settings>()
            .init_resource::<review_notes::Review>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<live_worktree::Live>()
            .init_resource::<progress::Progress>()
            .init_resource::<listing::find::Search>()
            .init_resource::<InputFocus>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Repo(repository_picker::empty()))
            .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
            .add_systems(Update, update);

        let world = app.world_mut();
        let editor = world.spawn(EditableText::default()).id();
        let camera = world
            .spawn((
                Camera3d::default(),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        world
            .resource_mut::<InputFocus>()
            .set(editor, FocusCause::Navigated);
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyH);
        app.update();
        assert!(!app.world().resource::<Tablet>().is_open());
        assert!(app.world().get::<Pilot>(camera).unwrap().captured);

        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        assert!(app.world().resource::<Tablet>().is_open());
        assert!(app.world().resource::<Tablet>().view == Page::Help);
        assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
        assert!(app.world().resource::<review_notes::Review>().tablet_open);

        let screen = app.world().resource::<Tablet>().panel;
        let world = app.world_mut();
        let window = world.spawn(Window::default()).id();
        let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::KeyH);
        keys.press(KeyCode::ShiftLeft);
        world.write_message(KeyboardInput {
            key_code: KeyCode::Digit7,
            logical_key: bevy::input::keyboard::Key::Character("/".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("/".into()),
            repeat: false,
            window,
        });
        app.update();
        let tablet = app.world().resource::<Tablet>();
        assert_eq!(tablet.page_name(), Some("search"));
        assert_eq!(tablet.panel, screen);
        assert!(tablet.search_input.is_some());
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            tablet.search_input
        );
        let labels: Vec<_> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        assert!(
            !labels
                .iter()
                .any(|text| matches!(text.as_str(), "Paths" | "In files" | "File names" | "Text"))
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<FileRows>>()
                .iter(world)
                .count(),
            1
        );

        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::ShiftLeft);
        keys.press(KeyCode::Tab);
        app.update();
        assert!(!app.world().resource::<Tablet>().is_open());

        // In the running app the review controls clear this flag before the next frame.
        app.world_mut()
            .resource_mut::<review_notes::Review>()
            .blocked = false;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::Tab);
        keys.press(KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<Tablet>().is_open());
        assert!(app.world().resource::<Tablet>().view == Page::Files);

        // Settings remains reachable from every page without occupying a navigation tab.
        let settings = app
            .world_mut()
            .query::<&Action>()
            .iter(app.world())
            .filter(|action| matches!(action, Action::Switch(Page::Settings)))
            .count();
        assert_eq!(settings, 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::Tab);
        let screen_root = app.world().resource::<Tablet>().panel;
        app.world_mut().resource_mut::<Tablet>().action = Some(Action::Switch(Page::Settings));
        app.update();
        assert_eq!(
            app.world().resource::<Tablet>().page_name(),
            Some("settings")
        );
        assert!(app.world().resource::<Tablet>().inputs.is_some());
        let providers: Vec<_> = app
            .world_mut()
            .query::<&Action>()
            .iter(app.world())
            .filter_map(|action| match action {
                Action::AgentProvider(provider) => Some(*provider),
                _ => None,
            })
            .collect();
        assert_eq!(providers, agent_engine::PROVIDERS);
        assert!(providers.contains(&agent_engine::MUSE));
        assert_eq!(
            app.world().resource::<Tablet>().panel,
            screen_root,
            "switching pages must preserve the physical screen's UI root"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        app.update();
        assert!(!app.world().resource::<Tablet>().is_open());
    }

    #[test]
    fn file_filters_share_rows_and_keep_each_files_latest_agent_location() {
        use agent_engine::activity::{Details, Kind, Location, Phase, Timeline};

        let mut repo = repository_picker::empty();
        repo.files = [
            ("seen.rs", Change::Modified),
            ("unseen.rs", Change::Added),
            ("context.rs", Change::Unchanged),
        ]
        .into_iter()
        .map(|(path, change)| repo_world::File {
            path: path.into(),
            before: None,
            after: None,
            change,
            position: [0., 0.],
        })
        .collect();
        let mut progress = progress::Progress::default();
        progress.visit(0, &repo.files[0], false);
        progress.visit(2, &repo.files[2], false);
        let times = FileTimes::from([("seen.rs".into(), 10), ("unseen.rs".into(), 20)]);
        let live = live_worktree::Live::default();
        let mut timeline = Timeline::default();
        for (item, path, line, kind) in [
            ("first", "seen.rs", 2, Kind::Edit),
            ("second", "seen.rs", 7, Kind::Edit),
            ("third", "context.rs", 5, Kind::Command),
        ] {
            let mut details = Details::new(kind, item);
            details.locations = vec![Location {
                path: path.into(),
                line,
            }];
            timeline.record("thread", item, details, Phase::Completed);
        }
        let activity = agent_timeline::files(&timeline);
        let rows = |filter| file_rows(&repo, &progress, &live, &times, filter, &activity);
        let paths = |filter| {
            rows(filter)
                .into_iter()
                .map(|row| row.file.path.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(FileFilter::Changes), ["unseen.rs", "seen.rs"]);
        assert_eq!(paths(FileFilter::Viewed), ["seen.rs"]);
        assert_eq!(paths(FileFilter::Agent), ["context.rs", "seen.rs"]);
        let all = rows(FileFilter::Changes);
        assert_eq!(all[0].activity, None);
        assert_eq!(all[1].activity.unwrap().line, 7);
        assert_eq!(
            all[1].label(FileFilter::Changes, 0., 20),
            "seen.rs\nCommitted 10s ago · Agent edited · L7"
        );
    }
}
