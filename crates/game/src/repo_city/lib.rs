//! Repository explorer and the code city's presentation.
#![allow(dead_code)]
#[cfg(not(target_arch = "wasm32"))]
mod agent_engine;
#[cfg(not(target_arch = "wasm32"))]
mod agent_navigation;
#[cfg(not(target_arch = "wasm32"))]
mod agent_session;
#[cfg(not(target_arch = "wasm32"))]
mod agent_timeline;
mod android;
mod blame;
#[cfg(target_arch = "wasm32")]
mod browser_audio;
#[cfg(target_arch = "wasm32")]
mod browser_tablet;
#[cfg(target_arch = "wasm32")]
mod browser_touch;
#[cfg(not(target_arch = "wasm32"))]
mod city_audio;
mod city_boards;
mod city_menu;
mod city_navigation;
mod city_stream;
mod code_texture;
mod comparison_scene;
#[cfg(not(target_arch = "wasm32"))]
mod desktop_splash;
mod district;
mod file_facade;
#[cfg(not(target_arch = "wasm32"))]
mod file_links;
mod file_picker;
mod file_preview;
#[cfg(not(target_arch = "wasm32"))]
mod focus_context;
#[cfg(not(target_arch = "wasm32"))]
mod github_choices;
mod guide;
mod image_preview;
mod listing;
#[cfg(not(target_arch = "wasm32"))]
mod live_worktree;
mod markdown_preview;
#[cfg(not(target_arch = "wasm32"))]
mod native_github;
mod native_panel;
mod physics;
mod progress;
mod reading;
mod repository_picker;
mod review_notes;
mod review_party;
#[cfg(not(target_arch = "wasm32"))]
mod review_tablet;
#[path = "../shapes.rs"]
mod shapes;
mod sketch;
mod source_editor;
mod tablet_display;
mod tablet_scroll;
#[cfg(not(target_arch = "wasm32"))]
mod terminal;
mod trace_import;
#[cfg(not(target_arch = "wasm32"))]
mod video_preview;
mod world_set;

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
#[cfg(not(target_arch = "wasm32"))]
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
#[cfg(not(target_arch = "wasm32"))]
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    window::PrimaryWindow,
};
use repo_world::{Change, FileAnatomy, Repository};
use repo_world::{comparison::Comparison, syntax};
use std::{collections::HashSet, path::PathBuf};
use world_set::{WorldSet, color};

type PlayerCamera = (
    With<Camera3d>,
    Without<district::MirrorCamera>,
    Without<android::TabletCamera>,
);

const CAMERA_FLOOR: f32 = 0.05;
const MAX_LOOK_PITCH: f32 = 1.5;
const APP_NAME: &str = "Repo City";

#[derive(Resource)]
struct Repo(Repository);
#[derive(Resource)]
struct Explorer {
    selected: usize,
    resume_on_open: bool,
    before: bool,
    source: String,
    original: String,
    anatomy: Option<FileAnatomy>,
    function: Option<usize>,
    page: usize,
    function_page: usize,
    expanded: bool,
    anchored: bool,
    dirty: bool,
    rebuild: bool,
    reviewed: HashSet<usize>,
    notice: String,
    pulse: f32,
    detached: Option<(String, String, usize)>,
    column: usize,
    highlighted: Vec<syntax::Line>,
    source_layout: listing::layout::SourceLayout,
    blame_layout: listing::layout::SourceLayout,
    diff_layout: listing::layout::DiffLayout,
    comparison: Option<Comparison>,
    comparison_mode: u8,
    hunk: usize,
    pending_fracture: bool,
}
impl Explorer {
    fn new(index: usize) -> Self {
        Self {
            selected: index,
            resume_on_open: true,
            before: false,
            source: String::new(),
            original: String::new(),
            anatomy: None,
            function: None,
            page: 0,
            function_page: 0,
            expanded: false,
            anchored: false,
            dirty: false,
            rebuild: true,
            reviewed: HashSet::new(),
            notice: "Aim at a file and press E to fly there.".into(),
            pulse: 0.,
            detached: None,
            column: 0,
            highlighted: vec![],
            source_layout: default(),
            blame_layout: default(),
            diff_layout: default(),
            comparison: None,
            comparison_mode: 0,
            hunk: 0,
            pending_fracture: false,
        }
    }
    fn open(&mut self, repo: &Repository, index: usize) {
        self.selected = index;
        self.before = repo.files[index].after.is_none();
        self.function = None;
        self.page = 0;
        self.function_page = 0;
        self.dirty = false;
        self.reload(repo);
        self.expanded = true;
        self.rebuild = true;
    }
    fn reload(&mut self, repo: &Repository) {
        match repo.anatomy(self.selected, self.before) {
            Ok((source, anatomy)) => {
                self.source = source.clone();
                self.original = source;
                self.anatomy = Some(anatomy);
                self.notice = if repo.is_worktree() && !self.before {
                    "Source loaded from the worktree."
                } else {
                    "Source loaded from the selected commit."
                }
                .into();
            }
            Err(e) => {
                self.source = String::new();
                self.original = String::new();
                self.anatomy = None;
                self.notice = e;
            }
        }
        self.dirty = false;
        self.detached = None;
        self.column = 0;
        self.hunk = 0;
        self.pending_fracture = false;
        self.refresh(repo);
        if self.comparison_mode > 0 {
            self.page = self
                .comparison
                .as_ref()
                .and_then(|c| c.hunks.first())
                .map_or(0, |h| h.row / 23);
        }
        self.rebuild = true;
    }
    fn set_highlighted(&mut self, lines: Vec<syntax::Line>) {
        self.source_layout.replace(&lines);
        self.blame_layout = listing::layout::SourceLayout::for_face(&lines, listing::Face::Blame);
        self.highlighted = lines;
    }

    fn set_comparison(&mut self, comparison: Option<Comparison>) {
        self.comparison = comparison;
        self.refresh_diff_layout();
    }

    fn refresh_diff_layout(&mut self) {
        self.diff_layout = listing::layout::DiffLayout::new(self.comparison.as_ref());
    }

    fn refresh(&mut self, repo: &Repository) {
        let file = &repo.files[self.selected];
        self.set_highlighted(syntax::highlight(&file.path, &self.source));
        let old = if file.before.is_none() {
            Ok(String::new())
        } else {
            repo.source(self.selected, true)
        };
        let new = if !self.before {
            if file.after.is_some() && self.anatomy.is_none() {
                repo.source(self.selected, false)
            } else {
                Ok(self.source.clone())
            }
        } else if file.after.is_none() {
            Ok(String::new())
        } else {
            repo.source(self.selected, false)
        };
        let comparison = match (old, new) {
            (Ok(old), Ok(new)) => match Comparison::new(&file.path, &old, &new) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.notice = format!("Diff unavailable: {e}");
                    None
                }
            },
            _ => None,
        };
        self.set_comparison(comparison);
    }
    fn detach_selection(&mut self, repo: &Repository) {
        self.cut();
        if self.pending_fracture {
            self.anatomy = Some(repo_world::anatomy::analyze(
                &repo.files[self.selected].path,
                &self.source,
            ));
            self.function = None;
            self.function_page = 0;
            self.page = 0;
            self.refresh(repo);
        }
    }

    fn cut(&mut self) {
        if self.before {
            self.notice = "BEFORE is a read-only ghost. Q switches to AFTER.".into();
            return;
        }
        if let Some(callable) = self
            .function
            .and_then(|i| self.anatomy.as_ref()?.callables.get(i))
        {
            if let (Some(start), Some(end)) = (callable.start_line, callable.end_line) {
                self.detached = Some((
                    callable.name.clone(),
                    self.source[callable.start_byte..callable.end_byte].to_owned(),
                    start as usize,
                ));
                self.source
                    .replace_range(callable.start_byte..callable.end_byte, "");
                self.dirty = true;
                self.notice = format!(
                    "Detached {} · lines {start}–{end}. R restores the draft.",
                    callable.name
                );
                self.rebuild = true;
                self.pulse = 1.;
                self.pending_fracture = true;
            }
        } else {
            self.notice = "Unfold a file, then aim at a function section to detach it.".into();
        }
    }
}
#[derive(Resource)]
struct Palette {
    shell: Handle<StandardMaterial>,
    facades: [[Handle<StandardMaterial>; 2]; code_texture::FacadePrint::ALL.len()],
    facade_neutral: Handle<StandardMaterial>,
    facade_rims: [Handle<StandardMaterial>; 3],
    edge: Handle<StandardMaterial>,
    removed: Handle<StandardMaterial>,
    plate: Handle<StandardMaterial>,
    unit: Handle<Mesh>,
}
#[derive(Component)]
struct CityObject;
#[derive(Component)]
struct FileBuilding(usize);
#[derive(Component)]
struct Facade {
    index: usize,
}
#[derive(Component)]
struct FacadeRim(usize);
#[derive(Component)]
struct Section {
    index: usize,
}
#[derive(Component)]
struct Detail;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct TopHud;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct BottomHud;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct StatusHud;
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ProgressLabel(bool);
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ProgressGauge(bool);
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ReviewToast;
#[derive(Component)]
struct TargetLabel;
#[derive(Component, Default)]
struct Pilot {
    captured: bool,
    keyboard: bool,
    fire_ready: bool,
    firing: bool,
    shot: bool,
    map_click: bool,
    walk: Option<city_navigation::Walk>,
    landing_warning_until: f32,
}
impl Pilot {
    fn update_fire(&mut self, mouse: &ButtonInput<MouseButton>) {
        let pressed = mouse.pressed(MouseButton::Left);
        let clicked = mouse.just_pressed(MouseButton::Left);
        self.update_trigger(pressed, clicked);
    }
    fn update_trigger(&mut self, pressed: bool, clicked: bool) {
        self.firing = self.captured && self.fire_ready && (pressed || clicked);
        self.shot = self.firing && clicked;
        self.fire_ready = self.captured && (self.fire_ready || (!pressed && !clicked));
    }
}

#[derive(Resource, Default)]
struct Nearest {
    file: Option<file_facade::Aim>,
    file_distance: f32,
    near_source: Option<usize>,
    district: Option<usize>,
    screen: Option<(Vec2, Vec2)>,
    function: Option<usize>,
    hunk: Option<usize>,
}
#[derive(Resource)]
struct Smoke {
    review_file: Option<usize>,
    directory: PathBuf,
    elapsed: f32,
    stage: usize,
    stop: usize,
    pending: bool,
    prepared: bool,
    expected_draft: Option<String>,
    capture_attempts: usize,
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        args.windows(2)
            .find(|p| p[0] == name)
            .map(|p| p[1].as_str())
    };
    if args.iter().any(|a| a == "--help") {
        println!(
            "repo-city [--repo PATH [--rev HEAD | --live] [--base REF] | --github URL [--rev REF] [--base REF]] [--file PATH] [--world-set ID|FILE | --world-picker] [--smoke-dir PATH]\nWASD fly · E flies to the aimed face · Space/Ctrl altitude · Shift boost · Click captures the cursor · Esc releases it\nClick close to start or stop washing source · Home frames the file · G diff · B blame\nC reviews changes · T follows a supplied stack · W/S or scroll fly between review stops · Q returns to the diff wall\nG ghost/pair · [ ] hunks · Enter source wall · Q before/after · Scroll/PgUp/PgDn read\nF6 exports the draft to this project's sandbox"
        );
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if args.iter().any(|arg| arg == "--notes-mcp") {
        let result = arg("--repo")
            .ok_or_else(|| "--notes-mcp requires --repo PATH".to_owned())
            .and_then(|path| review_notes::mcp::serve(std::path::Path::new(path)));
        if let Err(error) = result {
            eprintln!("Repo City MCP: {error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if args.iter().any(|arg| arg == "--world-picker") {
        world_set::picker::run(args);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let explicit = arg("--repo").map(PathBuf::from);
    #[cfg(not(target_arch = "wasm32"))]
    let remote = arg("--github");
    #[cfg(not(target_arch = "wasm32"))]
    let root = remote
        .is_none()
        .then(|| explicit.clone().or_else(repository_picker::last_folder))
        .flatten();
    #[cfg(not(target_arch = "wasm32"))]
    let mut startup_error = String::new();
    #[cfg(target_arch = "wasm32")]
    let startup_error = String::new();
    #[cfg(not(target_arch = "wasm32"))]
    let repo = match root {
        Some(root) => {
            let revision = repository_picker::startup_revision(
                args.iter().any(|arg| arg == "--live"),
                arg("--rev"),
            );
            match repository_picker::load(&root, revision, arg("--base")) {
                Ok(repo) => repo,
                Err(error) => {
                    startup_error = error;
                    repository_picker::empty()
                }
            }
        }
        None => repository_picker::empty(),
    };
    #[cfg(target_arch = "wasm32")]
    let repo = match repo_world::open_browser() {
        Ok(repo) => repo,
        Err(error) => {
            bevy::log::error!("{error}");
            return;
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let set = WorldSet::load(arg("--world-set")).unwrap_or_else(|e| {
        eprintln!("World set: {e}");
        std::process::exit(1)
    });
    #[cfg(target_arch = "wasm32")]
    let set = WorldSet::browser();
    let requested_file = arg("--file").filter(|_| !repo.files.is_empty());
    let initial = requested_file
        .map(|path| {
            repo.files
                .iter()
                .position(|f| f.path == path)
                .unwrap_or_else(|| {
                    eprintln!("File is not present in either snapshot: {path}");
                    std::process::exit(1)
                })
        })
        .unwrap_or_else(|| repo.initial_file());
    let mut explorer = Explorer::new(initial);
    explorer.resume_on_open = requested_file.is_none();
    if !repo.files.is_empty() {
        explorer.open(&repo, explorer.selected);
    }
    if let Some(directory) = arg("--export-review-textures") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("texture export directory");
        if let Some(diff) = &explorer.comparison {
            for before in [true, false] {
                let title = if before {
                    "BEFORE / parent snapshot"
                } else {
                    "AFTER / current snapshot"
                };
                let rows = diff.page(before, explorer.page * 23, 23);
                code_texture::source(
                    title,
                    &rows,
                    if before { set.removed } else { set.added },
                    1280,
                    768,
                    0,
                )
                .try_into_dynamic()
                .expect("RGBA image")
                .save(directory.join(if before { "before.png" } else { "after.png" }))
                .expect("save source texture");
            }
        }
        println!("Source textures exported to {}", directory.display());
        return;
    }
    if !repo.files.is_empty() {
        println!(
            "{APP_NAME}: {} files, {} districts, commit {}",
            repo.files.len(),
            repo.districts.len(),
            &repo.revision[..8]
        );
    }
    let mut app = App::new();
    let install = set.installer();
    let picker = repository_picker::RepositoryPicker::new(startup_error);
    #[cfg(not(target_arch = "wasm32"))]
    let picker = {
        let mut picker = picker;
        picker.restore_session();
        if let Some(input) = remote {
            picker.open_github(
                input,
                arg("--rev").unwrap_or_default(),
                arg("--base").unwrap_or_default(),
                arg("--file").map(str::to_owned),
            );
        }
        picker
    };
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: repository_picker::window_title(&set, &repo),
            resolution: (1440, 900).into(),
            #[cfg(target_arch = "wasm32")]
            canvas: Some("#repo-city".into()),
            #[cfg(target_arch = "wasm32")]
            fit_canvas_to_parent: true,
            #[cfg(target_arch = "wasm32")]
            prevent_default_event_handling: false,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(color(set.background)))
    .insert_resource(Repo(repo))
    .insert_resource(set)
    .insert_resource(explorer)
    .insert_resource(picker)
    .init_resource::<progress::Progress>()
    .init_resource::<review_notes::Review>()
    .init_resource::<listing::Listing>()
    .add_plugins(bevy::input_focus::tab_navigation::TabNavigationPlugin);
    install(&mut app, &args);
    app.run();
}

fn install_city(app: &mut App, args: &[String]) {
    let mut trace = trace_import::Trace::default();
    if let Some(path) = args
        .windows(2)
        .find(|pair| pair[0] == "--trace")
        .map(|pair| &pair[1])
    {
        let source = std::fs::read_to_string(path).unwrap_or_else(|error| {
            eprintln!("Trace {path}: {error}");
            std::process::exit(1)
        });
        trace.import(&app.world().resource::<Repo>().0, source);
        if !trace.report.error.is_empty() {
            eprintln!("Trace: {}", trace.report.error);
            std::process::exit(1)
        }
    }
    app.add_plugins((PhysicsPlugins::default(), listing::CoverPlugin))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.64, 0.82, 0.85),
            brightness: 550.,
            ..default()
        })
        .init_resource::<city_navigation::Navigation>()
        .insert_resource(trace)
        .init_resource::<Nearest>()
        .init_resource::<city_menu::Menu>()
        .init_resource::<file_picker::FilePicker>()
        .init_resource::<file_facade::ChangesOnly>()
        .init_resource::<reading::Settings>()
        .init_resource::<listing::find::Search>()
        .init_resource::<guide::Tour>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (
                    trace_import::update,
                    file_facade::lower_unchanged,
                    file_picker::update,
                )
                    .chain(),
                flight,
                (file_facade::keep_beams_clear, targeting).chain(),
                (
                    city_menu::update,
                    listing::fold_controls,
                    listing::find::update,
                )
                    .chain(),
                (
                    review_notes::refresh,
                    review_notes::update,
                    review_notes::panels,
                    review_notes::update_color_picker,
                    #[cfg(not(target_arch = "wasm32"))]
                    review_tablet::update,
                    #[cfg(not(target_arch = "wasm32"))]
                    live_worktree::focus_latest,
                )
                    .chain(),
                city_navigation::navigate
                    .run_if(city_menu::gameplay_enabled)
                    .run_if(listing::find::movement_enabled),
                interaction
                    .run_if(city_menu::gameplay_enabled)
                    .run_if(listing::find::movement_enabled),
                comparison_scene::controls
                    .run_if(city_menu::gameplay_enabled)
                    .run_if(listing::find::movement_enabled),
                listing::controls
                    .run_if(city_menu::gameplay_enabled)
                    .run_if(listing::find::movement_enabled),
                (listing::exposure::update, listing::wash_stream).chain(),
                (
                    guide::advance,
                    listing::paint_notes,
                    listing::find::mark,
                    (android::show_sprayer, android::show_tablet),
                )
                    .chain(),
                physics::splashes,
                physics::fractures,
                detail_scene,
                city_navigation::draw,
                listing::draw,
                listing::draw_overview,
                animate,
                hud,
            )
                .chain()
                .run_if(repository_picker::has_city),
        );
    app.add_systems(
        Update,
        city_menu::apply_filter_shortcut
            .after(file_facade::lower_unchanged)
            .before(file_picker::update)
            .run_if(repository_picker::has_city),
    );
    app.add_systems(
        Update,
        (
            #[cfg(not(target_arch = "wasm32"))]
            repository_picker::open_repository,
            #[cfg(not(target_arch = "wasm32"))]
            live_worktree::update,
            spawn_city,
            progress::restore,
        )
            .chain()
            .before(file_facade::lower_unchanged),
    );
    app.add_systems(
        Update,
        progress::sync
            .after(listing::wash_stream)
            .before(listing::draw)
            .run_if(repository_picker::has_city),
    );
    tablet_display::register(app);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(city_audio::Plugin)
        .add_systems(PostUpdate, synchronize_cursor)
        .add_plugins(focus_context::Plugin)
        .init_resource::<file_links::Links>()
        .add_systems(Update, file_links::update.before(review_tablet::update))
        .add_systems(Update, file_links::draw.after(file_links::update))
        .init_resource::<review_notes::ReviewUi>()
        .init_resource::<review_tablet::Tablet>()
        .add_plugins(desktop_splash::Plugin)
        .init_resource::<terminal::Terminal>()
        .add_systems(Update, terminal::update.after(review_tablet::update))
        .init_resource::<markdown_preview::Request>()
        .init_resource::<markdown_preview::ReadQueue>()
        .init_resource::<image_preview::Request>()
        .init_resource::<live_worktree::Live>()
        .add_systems(
            Update,
            live_worktree::animate
                .after(spawn_city)
                .before(listing::draw),
        )
        .add_systems(Update, live_worktree::highlight.after(animate));
    markdown_preview::register(app);
    source_editor::register(app);
    app.add_plugins(review_party::Plugin);
    app.add_plugins(city_boards::Plugin);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(agent_session::Plugin);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(agent_timeline::Plugin);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(agent_navigation::Plugin);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut reading = reading::Settings::load();
        if args.iter().any(|argument| argument == "--show-source") {
            reading.show_all_source = true;
        }
        app.insert_resource(reading);
    }
    #[cfg(target_arch = "wasm32")]
    app.init_resource::<browser_touch::Touch>()
        .add_systems(
            First,
            browser_touch::viewport.before(bevy::picking::PickingSystems::Input),
        )
        .add_systems(
            Update,
            browser_touch::update
                .before(file_facade::lower_unchanged)
                .before(tablet_display::resize),
        )
        .add_systems(Update, reading::apply.before(listing::wash_stream))
        .add_systems(Update, browser_audio::update.after(listing::draw));
    app.add_systems(Startup, district::setup);
    app.add_systems(
        Update,
        city_stream::draw_project_map.after(city_navigation::navigate),
    );
    app.add_systems(Update, district::reflect.after(listing::draw_overview));
    #[cfg(target_arch = "wasm32")]
    app.add_systems(
        Update,
        (
            browser_switch.before(spawn_city),
            browser_refresh
                .after(spawn_city)
                .before(file_picker::update)
                .run_if(repository_picker::has_city),
        ),
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        let arg = |name: &str| {
            args.windows(2)
                .find(|pair| pair[0] == name)
                .map(|pair| pair[1].as_str())
        };
        let bundle_smoke = std::env::current_exe()
            .ok()
            .and_then(|p| {
                p.parent()?
                    .parent()
                    .map(|p| p.join("Resources/smoke-output.txt"))
            })
            .and_then(|p| std::fs::read_to_string(p).ok());
        if let Some(directory) = arg("--smoke-dir").or(bundle_smoke.as_deref().map(str::trim)) {
            let failure_path = PathBuf::from(directory).join("failure.txt");
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if let Some(parent) = failure_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&failure_path, info.to_string());
                default_hook(info);
            }));
            let stage = args
                .iter()
                .find_map(|arg| match arg.as_str() {
                    "--smoke-walls" => Some(7),
                    "--smoke-blame" => Some(8),
                    "--smoke-source" => Some(9),
                    "--smoke-github-picker" => Some(10),
                    "--smoke-help" => Some(11),
                    "--smoke-review" => Some(12),
                    "--smoke-tablet" => Some(13),
                    "--smoke-paint" => Some(14),
                    "--smoke-guidance" => Some(15),
                    "--smoke-agent" => Some(23),
                    "--smoke-agent-controls" => Some(25),
                    "--smoke-settings" => Some(27),
                    "--smoke-hunk-pages" => Some(29),
                    "--smoke-review-roofs" => Some(31),
                    "--smoke-overview" => Some(24),
                    "--smoke-empty-diff" => Some(18),
                    "--smoke-frost" => Some(20),
                    _ => None,
                })
                .unwrap_or(0);
            app.insert_resource(Smoke {
                directory: directory.into(),
                elapsed: 0.,
                stage,
                stop: match stage {
                    29 => 31,
                    31 => 33,
                    15 => 18,
                    18 => 20,
                    20 => 23,
                    25 => 27,
                    27 => 29,
                    _ => (stage + 1).max(9),
                },
                pending: false,
                review_file: None,
                prepared: false,
                expected_draft: None,
                capture_attempts: 0,
            })
            .add_systems(Update, smoke.after(animate));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_switch(
    mut commands: Commands,
    mut observed: Local<Option<u32>>,
    old_city: repository_picker::CityEntities,
) {
    let next = repo_world::browser_repository_version();
    if observed
        .replace(next)
        .is_some_and(|previous| previous != next)
        && let Ok(repo) = repo_world::open_browser()
    {
        repository_picker::replace_city(&mut commands, &old_city, repo);
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_refresh(
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut version: Local<u32>,
    mut styling: Local<u32>,
    mut listing: ResMut<listing::Listing>,
    mut navigation: ResMut<city_navigation::Navigation>,
) {
    let current = repo_world::browser_source_version();
    if current != *version {
        *version = current;
        navigation.refresh_trace();
        if explorer.anatomy.is_none() || (explorer.comparison.is_none() && !explorer.dirty) {
            let page = explorer.page;
            explorer.reload(&repo.0);
            listing.invalidate();
            explorer.page = page;
        }
    }
    let syntax_version = repo_world::browser_syntax_version();
    if syntax_version != *styling {
        *styling = syntax_version;
        navigation.refresh_trace();
        let file = &repo.0.files[explorer.selected];
        let highlighted = syntax::highlight(&file.path, &explorer.source);
        explorer.set_highlighted(highlighted);
        let dirty = explorer.dirty;
        let displayed = (dirty && !explorer.before).then(|| explorer.source.clone());
        let selected = explorer.selected;
        if let Some(diff) = explorer.comparison.as_mut() {
            for (before, rows) in [(true, &mut diff.before), (false, &mut diff.after)] {
                if rows.is_empty() {
                    continue;
                }
                let source = if !before && dirty {
                    displayed.clone().ok_or_else(|| "Missing draft".to_owned())
                } else {
                    repo.0.source(selected, before)
                };
                if let Ok(source) = source {
                    let styled = syntax::highlight(&file.path, &source);
                    if styled.len() == rows.len() {
                        *rows = styled;
                    }
                }
            }
        }
        explorer.refresh_diff_layout();
        explorer.rebuild = true;
        listing.recolor();
    }
}

fn label(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    title: &str,
    position: Vec3,
    width: f32,
    ink: [f32; 3],
) -> Entity {
    let texture = images.add(code_texture::render(title, &[], ink, 768, 64));
    commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(width, width / 12.))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                unlit: true,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(position),
            CityObject,
        ))
        .id()
}

fn setup(
    mut commands: Commands,
    set: Res<WorldSet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    load_ui_font(&mut fonts);
    let unit = meshes.add(shapes::rounded_box(Vec3::ONE, 0.035));
    let shell = materials.add(StandardMaterial {
        base_color: color(set.shell),
        metallic: 0.6,
        perceptual_roughness: 0.35,
        ..default()
    });
    let facades = code_texture::FacadePrint::ALL.map(|print| {
        [code_texture::Coverage::Closed, code_texture::Coverage::Open].map(|coverage| {
            let mut material =
                listing::overview_material(images.add(code_texture::facade_atlas(coverage, print)));
            listing::scale_facade(&mut material, print, code_texture::FACADE_TILE_HEIGHT);
            materials.add(material)
        })
    });
    let facade_rims = code_texture::Coverage::ALL.map(|state| {
        let [r, g, b] = state.accent();
        materials.add(StandardMaterial {
            base_color: Color::srgb_u8(r, g, b),
            unlit: true,
            cull_mode: None,
            ..default()
        })
    });
    let [r, g, b] = code_texture::FACADE_GLASS;
    let facade_neutral = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(r, g, b),
        unlit: true,
        ..default()
    });
    let edge = materials.add(StandardMaterial {
        base_color: color(set.accent),
        emissive: LinearRgba::new(
            set.accent[0] * 2.,
            set.accent[1] * 2.,
            set.accent[2] * 2.,
            1.,
        ),
        ..default()
    });
    let removed = materials.add(StandardMaterial {
        base_color: color(set.removed),
        unlit: true,
        ..default()
    });
    let plate = materials.add(StandardMaterial {
        base_color: color(set.ground),
        metallic: 0.35,
        perceptual_roughness: 0.7,
        ..default()
    });
    commands.insert_resource(Palette {
        shell: shell.clone(),
        facades,
        facade_neutral,
        facade_rims,
        edge: edge.clone(),
        removed,
        plate: plate.clone(),
        unit: unit.clone(),
    });
    let pilot = commands
        .spawn((
            Camera3d::default(),
            bevy::ui::IsDefaultUiCamera,
            Camera { ..default() },
            Projection::Perspective(PerspectiveProjection {
                far: 100_000.,
                ..default()
            }),
            Msaa::Sample4,
            Transform::default(),
            Pilot::default(),
        ))
        .id();
    android::build(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        pilot,
    );
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.,
            shadow_maps_enabled: true,
            ..default()
        },
        bevy::light::CascadeShadowConfigBuilder {
            first_cascade_far_bound: 22.,
            maximum_distance: city_stream::RADIUS * 1.5,
            overlap_proportion: 0.25,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.4, 0.)),
    ));
    let ui_font = TextFont::from_font_size;
    file_picker::spawn(&mut commands, &ui_font(13.), color(set.ink));
    city_menu::spawn(&mut commands, &ui_font(13.), color(set.ink));
    guide::spawn_hint(&mut commands, &ui_font(13.), color(set.ink));
    #[cfg(not(target_arch = "wasm32"))]
    review_notes::spawn_empty_hint(&mut commands, ui_font(13.), color(set.ink));
    listing::find::spawn(&mut commands, &ui_font(13.), color(set.ink));
    #[cfg(not(target_arch = "wasm32"))]
    review_tablet::spawn_help(&mut commands, &ui_font(15.), color(set.ink));
    commands.spawn((
        review_notes::ToolHud,
        Text::new(""),
        ui_font(13.),
        TextColor(color(set.ink)),
        Node {
            position_type: PositionType::Absolute,
            top: percent(50.),
            right: percent(50.),
            margin: UiRect {
                right: px(13.),
                top: px(12.),
                ..default()
            },
            ..default()
        },
        bevy::picking::Pickable::IGNORE,
    ));
    #[cfg(not(target_arch = "wasm32"))]
    repository_picker::spawn(
        &mut commands,
        &ui_font(13.),
        color(set.ink),
        &mut images,
        None,
    );
    #[cfg(not(target_arch = "wasm32"))]
    commands.spawn((
        Text::new(""),
        ui_font(18.),
        TextColor(color(set.ink)),
        Node {
            position_type: PositionType::Absolute,
            left: px(222.),
            max_width: percent(42.),
            top: px(28.),
            ..default()
        },
        TopHud,
        desktop_splash::CityOverlay,
    ));
    #[cfg(not(target_arch = "wasm32"))]
    let bottom_hud = commands
        .spawn((
            Text::new(""),
            ui_font(14.),
            TextColor(Color::srgb(0.7, 0.8, 0.82)),
            BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.94)),
            Node {
                position_type: PositionType::Absolute,
                left: px(24.),
                top: px(92.),
                max_width: percent(90.),
                display: Display::None,
                padding: UiRect::all(px(14.)),
                ..default()
            },
            BottomHud,
        ))
        .id();
    #[cfg(not(target_arch = "wasm32"))]
    commands.entity(bottom_hud).insert((
        bevy::ui_widgets::Button,
        GlobalZIndex(25),
        file_picker::FileButton,
        bevy::picking::hover::Hovered::default(),
        bevy::ui_widgets::observe(live_worktree::request_focus),
    ));
    #[cfg(not(target_arch = "wasm32"))]
    commands
        .spawn((
            desktop_splash::CityOverlay,
            Node {
                position_type: PositionType::Absolute,
                right: px(24.),
                top: px(20.),
                width: px(260.),
                padding: UiRect::all(px(12.)),
                row_gap: px(6.),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.72)),
            bevy::picking::Pickable::IGNORE,
        ))
        .with_children(|hud| {
            hud.spawn((
                StatusHud,
                Text::new(""),
                ui_font(13.),
                TextColor(color(set.ink)),
            ));
            for diff in [false, true] {
                hud.spawn((
                    ProgressLabel(diff),
                    Text::new(""),
                    ui_font(21.),
                    TextColor(color(set.ink)),
                ));
                hud.spawn((
                    Node {
                        width: percent(100.),
                        height: px(4.),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(31, 49, 60)),
                ))
                .with_child((
                    ProgressGauge(diff),
                    Node {
                        height: percent(100.),
                        width: percent(0.),
                        ..default()
                    },
                    BackgroundColor(if diff {
                        Color::srgb_u8(250, 190, 104)
                    } else {
                        Color::srgb_u8(100, 230, 196)
                    }),
                ));
            }
        });
    #[cfg(not(target_arch = "wasm32"))]
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                bottom: px(22.),
                width: percent(100.),
                justify_content: JustifyContent::Center,
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ))
        .with_child((
            ReviewToast,
            Text::new(""),
            ui_font(15.),
            TextColor(color(set.ink)),
            BackgroundColor(Color::srgba(0.024, 0.066, 0.08, 0.95)),
            Node {
                display: Display::None,
                max_width: percent(54.),
                padding: UiRect::axes(px(12.), px(8.)),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));
    listing::spawn_reticle(&mut commands);
    commands.spawn((
        TargetLabel,
        Text::new(""),
        ui_font(14.),
        TextColor(color(set.ink)),
        BackgroundColor(Color::srgba(0.025, 0.045, 0.055, 0.96)),
        bevy::picking::Pickable::IGNORE,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(9.), px(5.)),
            ..default()
        },
        ZIndex(9),
    ));
}

fn load_ui_font(fonts: &mut Assets<Font>) {
    fonts
        .insert(
            Handle::<Font>::default().id(),
            Font::from_bytes(
                include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf").to_vec(),
            ),
        )
        .expect("default user interface font");
    fonts
        .insert(EMOJI_FONT.id(), Font::from_bytes(EMOJI_FONT_DATA.to_vec()))
        .expect("emoji fallback font");
    fonts
        .insert(MATH_FONT.id(), Font::from_bytes(MATH_FONT_DATA.to_vec()))
        .expect("math fallback font");
}

const MATH_FONT_DATA: &[u8] = include_bytes!("../../../../assets/fonts/JuliaMono-Medium.ttf");
const MATH_FONT: Handle<Font> = bevy::asset::uuid_handle!("66256012-d3ec-4c70-96cd-1fa58cb85c15");
const EMOJI_FONT_DATA: &[u8] = include_bytes!("../../../../assets/fonts/NotoEmoji.ttf");
const EMOJI_FONT: Handle<Font> = bevy::asset::uuid_handle!("3b4571e0-061f-44e9-8cbf-771e89a9860d");

struct BuildingAssets {
    block: Handle<Mesh>,
    rim: Handle<Mesh>,
    status: [Handle<StandardMaterial>; 4],
    mirror: Handle<Mesh>,
    glass: Handle<StandardMaterial>,
    beams: [Handle<StandardMaterial>; file_facade::INK.len()],
    lights: district::Lights,
}

fn building_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    set: &WorldSet,
) -> BuildingAssets {
    let mut block = Mesh::from(Cuboid::from_size(Vec3::ONE));
    // The stock cuboid rotates its UVs by face. Keep row height aligned on every wall.
    if let (
        Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)),
        Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)),
    ) = (
        block.attribute(Mesh::ATTRIBUTE_POSITION),
        block.attribute(Mesh::ATTRIBUTE_NORMAL),
    ) {
        let uvs: Vec<_> = positions
            .iter()
            .zip(normals)
            .map(|(&[x, y, z], &[nx, ny, nz])| {
                if nx != 0. {
                    [0.5 - z * nx, 0.5 - y]
                } else if nz != 0. {
                    [0.5 + x * nz, 0.5 - y]
                } else {
                    debug_assert!(ny != 0.);
                    [1., 1.]
                }
            })
            .collect();
        block.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    let block = meshes.add(block);
    let rim = meshes.add(file_facade::roof_rim());
    let [added, removed, modified, unchanged] = [
        Change::Added,
        Change::Removed,
        Change::Modified,
        Change::Unchanged,
    ]
    .map(|change| {
        materials.add(StandardMaterial {
            base_color: set.change(&change),
            unlit: true,
            ..default()
        })
    });
    let mirror = meshes.add(Rectangle::new(1., 1.));
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.065, 0.10, 0.14),
        unlit: true,
        ..default()
    });
    let beams = file_facade::INK.map(|ink| materials.add(file_facade::beam_material(ink)));
    BuildingAssets {
        block,
        rim,
        status: [added, removed, modified, unchanged],
        mirror,
        glass,
        beams,
        lights: district::Lights::new(meshes, materials),
    }
}

fn spawn_city(
    mut commands: Commands,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    mut explorer: ResMut<Explorer>,
    mut progress: ResMut<progress::Progress>,
    palette: Res<Palette>,
    mut camera: Query<(&mut Transform, &mut Pilot), PlayerCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    #[cfg(not(target_arch = "wasm32"))] mut live: ResMut<live_worktree::Live>,
    mut stream: Local<city_stream::Stream>,
    mut assets: Local<Option<BuildingAssets>>,
    mut listing: Option<ResMut<listing::Listing>>,
    parts: Query<
        (
            Entity,
            Option<&FileBuilding>,
            Option<&city_stream::DistrictPart>,
        ),
        Or<(With<FileBuilding>, With<city_stream::DistrictPart>)>,
    >,
) {
    use city_stream::Item;
    use file_facade::{Part, transform};
    if repo.0.files.is_empty() {
        return;
    }
    let unit = &palette.unit;
    let plate = &palette.plate;
    let shell = &palette.shell;
    #[cfg(not(target_arch = "wasm32"))]
    let keep_camera = std::mem::take(&mut live.keep_camera);
    #[cfg(target_arch = "wasm32")]
    let keep_camera = false;
    #[cfg(not(target_arch = "wasm32"))]
    let refreshed_files = live.refreshed_files.take();
    #[cfg(target_arch = "wasm32")]
    let refreshed_files: Option<HashSet<usize>> = None;
    if repo.is_changed()
        && !keep_camera
        && let Ok((mut pose, mut pilot)) = camera.single_mut()
    {
        *pose = progress
            .resume_camera(&repo.0, &mut explorer)
            .unwrap_or_else(|| {
                listing::view(
                    &repo.0.files[explorer.selected],
                    explorer.source_layout.len(),
                )
            });
        pilot.captured = false;
        pilot.walk = None;
        pilot.landing_warning_until = 0.;
    }
    if let Some(files) = refreshed_files.as_ref() {
        stream.refresh_files(files);
    } else if repo.is_changed() {
        stream.reset(&repo.0);
        let extent = repo
            .0
            .districts
            .iter()
            .fold(Vec2::splat(22.), |extent, district| {
                extent.max(Vec2::from_array(district.position) + Vec2::from_array(district.size))
            });
        commands.spawn((
            CityObject,
            Mesh3d(unit.clone()),
            MeshMaterial3d(plate.clone()),
            Transform::from_xyz(extent.x / 2., -0.7, extent.y / 2.).with_scale(Vec3::new(
                extent.x + 56.,
                1.,
                extent.y + 56.,
            )),
            RigidBody::Static,
            Collider::cuboid(1., 1., 1.),
        ));
    }
    let eye = camera.single().map_or_else(
        |_| Vec2::from_array(repo.0.files[explorer.selected].position),
        |(pose, _)| pose.translation.xz(),
    );
    let (evicted, batch) = stream.next(&repo.0, eye, explorer.selected);
    if !evicted.is_empty() {
        for (entity, file, district) in &parts {
            let item = file
                .map(|file| Item::File(file.0))
                .or_else(|| district.map(|district| Item::District(district.0)));
            if item.is_some_and(|item| evicted.contains(&item)) {
                commands.entity(entity).despawn();
            }
        }
    }
    let assets = assets.get_or_insert_with(|| building_assets(&mut meshes, &mut materials, &set));
    for item in batch {
        let i = match item {
            Item::District(index) => {
                let district = &repo.0.districts[index];
                let [x, z] = district.position;
                let [w, d] = district.size;
                commands.spawn((
                    CityObject,
                    city_stream::DistrictPart(index),
                    Mesh3d(unit.clone()),
                    MeshMaterial3d(shell.clone()),
                    Transform::from_xyz(x + w / 2. - 3., -0.12, z + d / 2. - 1.)
                        .with_scale(Vec3::new(w, 0.18, d)),
                ));
                assets.lights.spawn(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    district,
                    index,
                );
                continue;
            }
            Item::File(index) => index,
        };
        let file = &repo.0.files[i];
        let print = code_texture::FacadePrint::for_path(&file.path);
        if let Some(listing) = listing.as_mut() {
            listing.building_loaded(i);
        }
        let size = listing::block_size(file);
        let body = commands
            .spawn((
                CityObject,
                Facade { index: i },
                Mesh3d(assets.block.clone()),
                MeshMaterial3d(palette.facades[print as usize][0].clone()),
                transform(file, size, Part::Body),
                RigidBody::Static,
                Collider::cuboid(1., 1., 1.),
            ))
            .id();
        let roof = commands
            .spawn((
                CityObject,
                FacadeRim(i),
                bevy::light::NotShadowCaster,
                Mesh3d(assets.rim.clone()),
                MeshMaterial3d(palette.facade_rims[0].clone()),
                transform(file, size, Part::Roof),
            ))
            .id();
        let status = match file.change {
            Change::Added => &assets.status[0],
            Change::Removed => &assets.status[1],
            Change::Modified => &assets.status[2],
            Change::Unchanged => &assets.status[3],
        };
        let stripes = [-1., 1.].map(|face| {
            let part = Part::Stripe(face);
            let stripe = commands
                .spawn((
                    CityObject,
                    Mesh3d(assets.block.clone()),
                    MeshMaterial3d(status.clone()),
                    transform(file, size, part),
                ))
                .id();
            (stripe, part)
        });
        let kind = file_facade::kind(&file.path);
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        let extension = file_facade::extension(&file.path);
        let title = if extension.is_empty() {
            name.to_owned()
        } else {
            format!(".{}   {name}", extension.to_uppercase())
        };
        let sign = label(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &title,
            Vec3::ZERO,
            1.,
            file_facade::INK[kind],
        );
        commands
            .entity(sign)
            .insert(transform(file, size, Part::Label));
        let beam = commands
            .spawn((
                CityObject,
                file_facade::FileBeam,
                bevy::light::NotShadowCaster,
                Mesh3d(assets.block.clone()),
                MeshMaterial3d(assets.beams[kind].clone()),
                transform(file, size, Part::Beam),
            ))
            .id();
        let pane = commands
            .spawn((
                CityObject,
                district::Glass,
                bevy::light::NotShadowCaster,
                Mesh3d(assets.mirror.clone()),
                MeshMaterial3d(assets.glass.clone()),
                transform(file, size, Part::Mirror),
            ))
            .id();
        let parts = [
            (body, Part::Body),
            (roof, Part::Roof),
            (sign, Part::Label),
            (beam, Part::Beam),
            (pane, Part::Mirror),
        ]
        .into_iter()
        .chain(stripes);
        for (entity, _) in parts.clone() {
            commands.entity(entity).insert(FileBuilding(i));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(&previous) = live.growth.get(&i) {
            for (entity, part) in parts.clone() {
                live_worktree::grow(
                    &mut commands,
                    entity,
                    file,
                    part,
                    previous,
                    size,
                    live.growing_until,
                );
            }
        }
        if file.change == Change::Unchanged {
            for (entity, part) in parts {
                commands
                    .entity(entity)
                    .insert(file_facade::SinkPart::new(file, size, part));
            }
        }
    }
}

/// Apply cursor state after every tablet/menu action, rather than racing Update writers.
#[cfg(not(target_arch = "wasm32"))]
fn synchronize_cursor(
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    camera: Query<&Pilot, PlayerCamera>,
) {
    let Ok(pilot) = camera.single() else {
        return;
    };
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let captured = window.focused && pilot.captured;
    let grab = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
    }
    if cursor.visible == captured {
        cursor.visible = !captured;
    }
}

fn flight(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    (motion, wheel): (Res<AccumulatedMouseMotion>, Res<AccumulatedMouseScroll>),
    mut explorer: ResMut<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    mut camera: Query<(&mut Transform, &mut Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    (listing, repo, focus, facades): (
        Res<listing::Listing>,
        Res<Repo>,
        Res<file_facade::ChangesOnly>,
        Query<&Facade>,
    ),
    file_buttons: Query<&bevy::picking::hover::Hovered, With<file_picker::FileButton>>,
    review: Res<review_notes::Review>,
    search: Res<listing::find::Search>,
    #[cfg(not(target_arch = "wasm32"))] repository: Option<
        Res<repository_picker::RepositoryPicker>,
    >,
    #[cfg(target_arch = "wasm32")] mut previous_pointer_ui: Local<Option<(bool, bool)>>,
    #[cfg(target_arch = "wasm32")] touch: Res<browser_touch::Touch>,
) {
    let Ok((mut camera, mut pilot)) = camera.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let menu_open = menu.is_open();
    let map_open = navigation.overview();
    let pointer_ui_open = picker.is_open() || review.blocked || search.blocks_controls();
    #[cfg(not(target_arch = "wasm32"))]
    let pointer_ui_open =
        pointer_ui_open || repository.is_some_and(|picker| picker.blocks_controls());
    let was_captured = pilot.captured;
    let over_button = !was_captured && file_buttons.iter().any(|hovered| hovered.get());
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            // Browser dialogs manage their own pointer state synchronously.
            let pointer_ui = picker.is_open() || search.blocks_controls() || over_button;
            if *previous_pointer_ui != Some((pointer_ui, map_open))
                && let Some(canvas) = document.get_element_by_id("repo-city")
            {
                if pointer_ui {
                    let _ = canvas.set_attribute("data-pointer-ui", "");
                } else {
                    let _ = canvas.remove_attribute("data-pointer-ui");
                }
                if map_open {
                    let _ = canvas.set_attribute("data-overview", "");
                } else {
                    let _ = canvas.remove_attribute("data-overview");
                }
                *previous_pointer_ui = Some((pointer_ui, map_open));
            }
            pilot.captured = document
                .pointer_lock_element()
                .is_some_and(|element| element.id() == "repo-city");
            if pilot.captured && (pointer_ui_open || map_open) {
                document.exit_pointer_lock();
                pilot.captured = false;
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    if touch.enabled && !pointer_ui_open && !map_open && !menu_open {
        pilot.captured = true;
    }
    let focused = window.focused || (cfg!(target_arch = "wasm32") && pilot.captured);
    #[cfg(not(target_arch = "wasm32"))]
    if focused && !pointer_ui_open && !menu_open && !over_button && !map_open {
        if mouse.just_pressed(MouseButton::Left) {
            pilot.captured = true;
        }
    }
    if focused
        && !pointer_ui_open
        && !menu_open
        && !map_open
        && keys.any_pressed([
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::KeyK,
        ])
    {
        pilot.keyboard = true;
        if keys.just_pressed(KeyCode::KeyK) {
            pilot.fire_ready = true;
        }
    }
    if pilot.keyboard && focused && !pointer_ui_open && !menu_open && !map_open {
        pilot.captured = true;
    }
    if keys.just_pressed(KeyCode::Escape) {
        pilot.captured = false;
        pilot.keyboard = false;
        if !menu.is_open() {
            explorer.anchored = false;
        }
    }
    if !focused || pointer_ui_open || map_open {
        pilot.captured = false;
    }
    pilot.map_click = map_open
        && focused
        && !pointer_ui_open
        && !over_button
        && !menu_open
        && mouse.just_pressed(MouseButton::Left);
    if menu_open {
        pilot.fire_ready = false;
        pilot.firing = false;
        pilot.shot = false;
        return;
    }
    pilot.update_trigger(
        mouse.pressed(MouseButton::Left) || keys.pressed(KeyCode::KeyK),
        mouse.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::KeyK),
    );
    if navigation.guides_camera()
        || listing.turning()
        || map_open
        || explorer.anchored
        || explorer.comparison_mode != 0
    {
        pilot.walk = None;
    }
    if pointer_ui_open || listing.turning() || !focused || navigation.is_flying() {
        return;
    }
    // This frame may contain movement from before the capture click.
    let close_wall = listing
        .close_face(
            &repo.0.files[explorer.selected],
            &camera,
            &explorer,
            window.width() / window.height().max(1.),
        )
        .is_some();
    let keyboard_axis =
        |positive, negative| f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative));
    let keyboard_look = if !navigation.guides_camera()
        && (!close_wall || keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]))
    {
        Vec2::new(
            keyboard_axis(KeyCode::ArrowRight, KeyCode::ArrowLeft),
            keyboard_axis(KeyCode::ArrowDown, KeyCode::ArrowUp),
        ) * time.delta_secs()
            * 600.
    } else {
        Vec2::ZERO
    };
    let look = motion.delta + keyboard_look;
    #[cfg(target_arch = "wasm32")]
    let look = look + Vec2::from_array(touch.look);
    if (was_captured || pilot.keyboard) && pilot.captured && look != Vec2::ZERO {
        let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        let yaw = yaw - look.x * 0.0025;
        let pitch = (pitch - look.y * 0.0025).clamp(-MAX_LOOK_PITCH, MAX_LOOK_PITCH);
        camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.);
    }
    if navigation.guides_camera() {
        return;
    }
    let clear = |point: Vec2| {
        let half_width = listing::WIDTH * 0.5 + 0.35;
        facades.iter().all(|facade| {
            let file = &repo.0.files[facade.index];
            !focus.includes(file)
                || (point - Vec2::from_array(file.position))
                    .abs()
                    .max_element()
                    >= half_width
        })
    };
    if !map_open
        && !explorer.anchored
        && explorer.comparison_mode == 0
        && keys.just_pressed(KeyCode::KeyO)
        && !keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ])
    {
        pilot.landing_warning_until = 0.;
        if pilot.walk.take().is_none() {
            if clear(camera.translation.xz()) {
                pilot.walk = Some(city_navigation::Walk::new(camera.translation.y));
            } else {
                pilot.landing_warning_until = time.elapsed_secs() + 3.;
            }
        }
    }
    if keys.any_pressed([
        KeyCode::Space,
        KeyCode::Home,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ]) || close_wall && keys.any_pressed([KeyCode::ArrowUp, KeyCode::ArrowDown])
        || wheel.delta.y != 0. && close_wall
    {
        pilot.walk = None;
    }
    let (forward, right) = if pilot.walk.is_some() {
        let right = camera.right().as_vec3().with_y(0.).normalize_or(Vec3::X);
        (Vec3::Y.cross(right), right)
    } else {
        (camera.forward().as_vec3(), camera.right().as_vec3())
    };
    let axis = |p, n| f32::from(keys.pressed(p)) - f32::from(keys.pressed(n));
    let mut direction = forward * axis(KeyCode::KeyW, KeyCode::KeyS)
        + right * axis(KeyCode::KeyD, KeyCode::KeyA)
        + Vec3::Y
            * if city_navigation::history_direction(&keys).is_some() {
                0.
            } else {
                axis(KeyCode::Space, KeyCode::ControlLeft)
            };
    if explorer.anchored {
        direction = Vec3::new(
            axis(KeyCode::KeyD, KeyCode::KeyA),
            axis(KeyCode::KeyW, KeyCode::KeyS),
            0.,
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        direction +=
            forward * touch.r#move[1].clamp(-1., 1.) + right * touch.r#move[0].clamp(-1., 1.);
    }
    if let Some(walk) = &mut pilot.walk {
        walk.step(&mut camera, direction, time.delta_secs(), clear);
        return;
    }
    let speed = flight_speed(
        camera.translation.y,
        explorer.anchored,
        keys.pressed(KeyCode::ShiftLeft),
    );
    let movement = direction.clamp_length_max(1.) * speed * time.delta_secs();
    camera.translation += movement;
    camera.translation.y = camera.translation.y.max(CAMERA_FLOOR);
}

fn flight_speed(altitude: f32, anchored: bool, boost: bool) -> f32 {
    if anchored {
        return if boost { 42. } else { 3. };
    }
    let scale = 1. + ((altitude - 72.) / 55.).clamp(0., 11.);
    scale * if boost { 42. } else { 13. }
}

fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    ray_box_entry(origin, direction, min, max).map(|(distance, _)| distance)
}

fn ray_box_entry(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if direction[axis].abs() < 0.00001 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            let entry = a.min(b);
            if entry >= near {
                near = entry;
                normal = Vec3::ZERO;
                normal[axis] = -direction[axis].signum();
            }
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some((near, normal))
}
fn targeting(
    repo: Res<Repo>,
    focus: Res<file_facade::ChangesOnly>,
    explorer: Res<Explorer>,
    camera: Query<(&Camera, &Transform, &Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    facades: Query<&Facade>,
    sections: Query<(&Section, &Transform)>,
    hunks: Query<(&comparison_scene::HunkBlock, &Transform)>,
    district_beams: Query<(&district::Beacon, &Transform)>,
    mut nearest: ResMut<Nearest>,
    navigation: Res<city_navigation::Navigation>,
    mut map_hover: Local<Option<(Vec3, Vec3, Option<usize>)>>,
) {
    let Ok((projection, camera, pilot)) = camera.single() else {
        nearest.screen = None;
        return;
    };
    nearest.screen = None;
    let window = windows.single().ok();
    if let Some(window) = window.filter(|window| window.focused || pilot.captured) {
        let size = Vec2::new(window.width(), window.height());
        nearest.screen = if pilot.captured {
            Some((size * 0.5, size))
        } else {
            window.cursor_position().map(|point| (point, size))
        };
    }
    let (origin, direction) = if !pilot.captured
        && let Some(window) = window
        && let Some(cursor) = window.cursor_position()
        && let Ok(ray) = projection.viewport_to_world(&GlobalTransform::from(*camera), cursor)
    {
        (ray.origin, ray.direction.as_vec3())
    } else {
        (camera.translation, camera.forward().as_vec3())
    };
    nearest.file = None;
    nearest.function = None;
    nearest.hunk = None;
    nearest.near_source = None;
    nearest.district = None;
    if navigation.overview_level() == Some(city_navigation::Overview::Project) {
        if nearest.screen.is_some() {
            if repo.is_changed()
                || map_hover
                    .as_ref()
                    .is_none_or(|(eye, ray, _)| *eye != origin || *ray != direction)
            {
                *map_hover = Some((
                    origin,
                    direction,
                    city_stream::district_at_ray(&repo.0, origin, direction),
                ));
            }
            nearest.district = map_hover.as_ref().and_then(|(_, _, district)| *district);
        }
        nearest.file_distance = f32::INFINITY;
        return;
    }
    let mut distance = 1000.;
    let reading = file_facade::reading_plane(&repo.0, &explorer, camera);
    let mut near_distance = f32::INFINITY;
    // The aim ray may use the free cursor. Proximity follows the camera.
    for f in facades
        .iter()
        .filter(|_| !(explorer.anchored && explorer.comparison_mode > 0))
    {
        let file = &repo.0.files[f.index];
        if !focus.includes(file) {
            continue;
        }
        let [x, z] = file.position;
        let size = listing::block_size(file);
        let min = Vec3::new(x - size.x / 2., 0., z - size.z / 2.);
        let max = Vec3::new(x + size.x / 2., size.y, z + size.z / 2.);
        let beam_min = Vec3::new(x - 0.55, size.y, z - 0.55);
        let beam_max = Vec3::new(x + 0.55, size.y + file_facade::BEAM_HEIGHT, z + 0.55);
        if let Some((d, normal)) = ray_box_entry(origin, direction, min, max)
            && d < distance
        {
            distance = d;
            nearest.file = Some(file_facade::Aim {
                index: f.index,
                surface: Some(file_facade::Surface {
                    normal,
                    height: ((origin.y + direction.y * d) / size.y).clamp(0., 1.),
                }),
            });
        }
        if let Some(d) = ray_box(origin, direction, beam_min, beam_max)
            && file_facade::beam_visible(
                &file_facade::transform(file, size, file_facade::Part::Beam),
                camera,
                reading.as_ref(),
            )
            && d < distance
        {
            distance = d;
            nearest.file = Some(file_facade::Aim {
                index: f.index,
                surface: None,
            });
        }
        if let Some(d) = file_facade::approach(camera, min, max)
            && d <= (size.x * 0.96).clamp(4.5, 14.)
            && d < near_distance
        {
            near_distance = d;
            nearest.near_source = Some(f.index);
        }
    }
    for (marker, pose) in &district_beams {
        if !file_facade::beam_visible(pose, camera, reading.as_ref()) {
            continue;
        }
        let half = Vec3::new(0.7, pose.scale.y / 2., 0.7);
        if let Some(d) = ray_box(
            origin,
            direction,
            pose.translation - half,
            pose.translation + half,
        ) && d < distance
        {
            distance = d;
            nearest.file = None;
            nearest.district = Some(marker.0);
        }
    }
    for (section, transform) in &sections {
        if let Some(d) = ray_box(
            origin,
            direction,
            transform.translation - transform.scale / 2.,
            transform.translation + transform.scale / 2.,
        ) && d < distance
        {
            distance = d;
            nearest.function = Some(section.index);
            nearest.file = None;
        }
    }
    for (hunk, transform) in &hunks {
        if let Some(d) = ray_box(
            origin,
            direction,
            transform.translation - hunk.hit_size / 2.,
            transform.translation + hunk.hit_size / 2.,
        ) && d < distance
        {
            distance = d;
            nearest.hunk = Some(hunk.index);
            nearest.function = None;
            nearest.file = None;
        }
    }
    nearest.file_distance = distance;
}

fn interaction(
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    repo: Res<Repo>,
    nearest: Res<Nearest>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<(&mut Transform, &Pilot), PlayerCamera>,
    mut navigation: ResMut<city_navigation::Navigation>,
    picker: Res<file_picker::FilePicker>,
    mut progress: ResMut<progress::Progress>,
) {
    if picker.is_open() {
        return;
    }
    if navigation.is_flying() || (navigation.guides_camera() && !navigation.reviewing()) {
        return;
    }
    let Ok((mut camera, pilot)) = camera.single_mut() else {
        return;
    };
    if pilot.shot {
        if let Some(index) = nearest.hunk {
            if let Some(diff) = &explorer.comparison
                && let Some(h) = diff.hunks.get(index)
            {
                let page = h.row / 23;
                let (row, count) = (h.row, h.before.len().max(h.after.len()));
                navigation.select_hunk(explorer.selected, index, row, count);
                explorer.page = page;
                explorer.hunk = index;
                explorer.rebuild = true;
                explorer.notice = format!("Hunk {}: both source pages aligned.", index + 1);
            }
        } else if let Some(index) = nearest.function {
            explorer.function = Some(index);
            explorer.detach_selection(&repo.0);
        }
    }
    if navigation.reviewing() {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        let enter_wall = explorer.comparison_mode > 0 || !explorer.anchored;
        explorer.comparison_mode = 0;
        explorer.rebuild = true;
        if enter_wall {
            if let Some(index) = nearest.file.map(|aim| aim.index)
                && index != explorer.selected
            {
                explorer.open(&repo.0, index);
            }
            let [x, z] = repo.0.files[explorer.selected].position;
            *camera = Transform::from_xyz(x, 6.2, z + 14.5)
                .looking_at(Vec3::new(x, 6.2, z + 3.2), Vec3::Y);
        }
        explorer.anchored = enter_wall;
        explorer.notice = if explorer.anchored {
            "Anchored to facade. WASD crawls. Enter returns to flight."
        } else {
            "Free flight."
        }
        .into();
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        explorer.before = !explorer.before;
        explorer.function = None;
        explorer.page = 0;
        explorer.reload(&repo.0);
    }
    let total = explorer.anatomy.as_ref().map_or(0, |a| a.callables.len());
    if keys.pressed(KeyCode::ShiftLeft)
        && (keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::ArrowLeft))
    {
        explorer.column = if keys.just_pressed(KeyCode::ArrowRight) {
            explorer.column + 30
        } else {
            explorer.column.saturating_sub(30)
        };
        explorer.rebuild = true;
    }
    if total > 0
        && explorer.anchored
        && !keys.pressed(KeyCode::ShiftLeft)
        && (keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::ArrowLeft))
    {
        let old = explorer
            .function
            .unwrap_or(if keys.just_pressed(KeyCode::ArrowRight) {
                total - 1
            } else {
                0
            });
        let index = if keys.just_pressed(KeyCode::ArrowRight) {
            (old + 1) % total
        } else {
            (old + total - 1) % total
        };
        explorer.function = Some(index);
        explorer.function_page = index / 8;
        explorer.page = 0;
        explorer.rebuild = true;
    }
    let page_down = keys.just_pressed(KeyCode::PageDown) || wheel.delta.y < 0.;
    let page_up = keys.just_pressed(KeyCode::PageUp) || wheel.delta.y > 0.;
    if (page_down || page_up) && (explorer.anchored || explorer.comparison_mode > 0) {
        let count = if explorer.comparison_mode > 0 {
            explorer
                .comparison
                .as_ref()
                .map_or(1, |c| c.rows.len().max(1).div_ceil(23))
        } else {
            visible_lines(&explorer).len().max(1).div_ceil(23)
        };
        explorer.page = if page_down {
            (explorer.page + 1).min(count - 1)
        } else {
            explorer.page.saturating_sub(1)
        };
        explorer.rebuild = true;
    }
    if keys.just_pressed(KeyCode::KeyX) && explorer.anchored {
        explorer.detach_selection(&repo.0);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        explorer.reload(&repo.0);
        explorer.function = None;
        explorer.page = 0;
        explorer.function_page = 0;
        explorer.notice = "Draft restored from the immutable source snapshot.".into();
    }
    if keys.just_pressed(KeyCode::KeyV) {
        let index = explorer.selected;
        let checked = explorer.reviewed.insert(index);
        if checked {
            progress.visit(index, &repo.0.files[index], false);
        } else {
            explorer.reviewed.remove(&index);
        }
        progress.mark(
            progress::Mark::Reviewed(repo.0.files[index].path.clone()),
            checked,
        );
        explorer.notice = if checked {
            "File marked reviewed."
        } else {
            "File marked unreviewed."
        }
        .into();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if keys.just_pressed(KeyCode::F6) && explorer.dirty {
        let file = &repo.0.files[explorer.selected].path;
        if std::path::Path::new(file)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            let destination = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../sandbox")
                .join(&repo.0.revision[..8])
                .join(file);
            let result = std::fs::create_dir_all(destination.parent().unwrap())
                .and_then(|_| std::fs::write(&destination, &explorer.source));
            explorer.notice = match result {
                Ok(()) => format!("Draft saved: {}", destination.display()),
                Err(e) => format!("Save failed: {e}"),
            };
        }
    }
}
fn visible_lines(explorer: &Explorer) -> Vec<(usize, String)> {
    let range = explorer
        .function
        .and_then(|i| explorer.anatomy.as_ref()?.callables.get(i))
        .and_then(|c| Some((c.start_line? as usize, c.end_line? as usize)));
    explorer
        .source
        .lines()
        .enumerate()
        .filter(|(i, _)| range.is_none_or(|(a, b)| *i + 1 >= a && *i < b))
        .map(|(i, s)| (i + 1, s.to_owned()))
        .collect()
}

fn detail_scene(
    mut commands: Commands,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    mut explorer: ResMut<Explorer>,
    palette: Res<Palette>,
    old: Query<Entity, With<Detail>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !explorer.rebuild {
        return;
    }
    explorer.rebuild = false;
    for entity in &old {
        commands.entity(entity).despawn();
    }
    if explorer.comparison_mode == 0 && !explorer.anchored {
        return;
    }
    let file = &repo.0.files[explorer.selected];
    let [x, z] = file.position;
    let title = format!(
        "{} · {}{}",
        if explorer.before { "BEFORE" } else { "AFTER" },
        file.path,
        if explorer.dirty { " · DRAFT" } else { "" }
    );
    comparison_scene::draw(
        &mut commands,
        &repo.0,
        &set,
        &explorer,
        &palette,
        &mut meshes,
        &mut materials,
        &mut images,
    );
    let lines: Vec<_> = visible_lines(&explorer)
        .into_iter()
        .skip(explorer.page * 23)
        .take(23)
        .filter_map(|(n, _)| {
            explorer
                .highlighted
                .get(n - 1)
                .cloned()
                .map(|line| (Some(line), repo_world::comparison::Mark::Context))
        })
        .collect();
    let panel_height = (lines.len().max(7) * 27 + 65).min(768) as u32;
    let panel_world_height = 7.2 * panel_height as f32 / 768.;
    let texture = images.add(code_texture::source(
        &title,
        &lines,
        set.ink,
        1280,
        panel_height,
        explorer.column,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(12., panel_world_height))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_xyz(x, 6.2, z + 4.16),
        Detail,
    ));
    if let Some((name, source, start)) = &explorer.detached {
        let lines: Vec<_> = syntax::highlight(&file.path, source)
            .into_iter()
            .take(18)
            .map(|mut line| {
                line.number += start - 1;
                (Some(line), repo_world::comparison::Mark::Removed)
            })
            .collect();
        let texture = images.add(code_texture::source(
            &format!("DETACHED / {name} / R restores"),
            &lines,
            set.removed,
            1024,
            640,
            0,
        ));
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(8., 5.))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                unlit: true,
                cull_mode: None,
                ..default()
            })),
            Transform::from_xyz(x - 10.2, 6.2, z + 3.35),
            Detail,
        ));
    }
    if !explorer.expanded {
        return;
    }
    if let Some(anatomy) = &explorer.anatomy {
        for (i, callable) in anatomy
            .callables
            .iter()
            .enumerate()
            .skip(explorer.function_page * 8)
            .take(8)
        {
            let slot = i % 8;
            let column = slot % 4;
            let row = slot / 4;
            let cx = x - 5.1 + column as f32 * 3.4;
            let cz = z + 7. + row as f32 * 3.;
            // Sloposcope's logarithmic metrics, adapted to world metres.
            let length = 0.55 + 0.28 * (callable.body_line_span.unwrap_or(0) as f32).ln_1p();
            let height = 0.35 + 0.22 * (callable.decision_count.unwrap_or(0) as f32).ln_1p();
            let selected = explorer.function == Some(i);
            commands.spawn((
                Mesh3d(palette.unit.clone()),
                MeshMaterial3d(if selected {
                    palette.edge.clone()
                } else {
                    palette.shell.clone()
                }),
                Transform::from_xyz(cx, 0.8 + height / 2., cz)
                    .with_scale(Vec3::new(2.9, height, length)),
                Section { index: i },
                Detail,
            ));
            let title = format!(
                "{} · L{}",
                callable.display_name.as_ref().unwrap_or(&callable.name),
                callable.start_line.unwrap_or(0)
            );
            let tex = images.add(code_texture::render(
                &title,
                &[],
                if selected { set.accent } else { set.ink },
                768,
                64,
            ));
            commands.spawn((
                Mesh3d(meshes.add(Rectangle::new(3.1, 0.35))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color_texture: Some(tex),
                    unlit: true,
                    cull_mode: None,
                    ..default()
                })),
                Transform::from_xyz(cx, 1.6 + height, cz + length / 2. + 0.05),
                Detail,
            ));
        }
    }
}
fn animate(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut explorer: ResMut<Explorer>,
    mut debris: Query<(Entity, &mut physics::Lifetime)>,
) {
    explorer.pulse = (explorer.pulse - time.delta_secs()).max(0.);
    for (entity, mut part) in &mut debris {
        part.0 -= time.delta_secs();
        if part.0 <= 0. || keys.just_pressed(KeyCode::KeyR) {
            commands.entity(entity).despawn();
        }
    }
}
fn hud(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    nearest: Res<Nearest>,
    navigation: Res<city_navigation::Navigation>,
    progress: Res<progress::Progress>,
    time: Res<Time>,
    pilot: Query<&Pilot, PlayerCamera>,
    #[cfg(target_arch = "wasm32")] mut browser_hud: Local<browser_touch::Hud>,
    picker: Res<file_picker::FilePicker>,
    #[cfg(not(target_arch = "wasm32"))] repository: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] native: (
        Res<review_notes::Review>,
        Res<live_worktree::Live>,
    ),
    mut label: Query<(&mut Text, &mut TextColor, &mut Node), With<TargetLabel>>,
    #[cfg(not(target_arch = "wasm32"))] mut gauges: Query<
        (&ProgressGauge, &mut Node),
        (Without<Text>, Without<TargetLabel>),
    >,
    #[cfg(not(target_arch = "wasm32"))] mut texts: Query<
        (
            &mut Text,
            &mut Node,
            Has<TopHud>,
            Has<BottomHud>,
            Has<StatusHud>,
            Has<ReviewToast>,
            Option<&ProgressLabel>,
        ),
        Without<TargetLabel>,
    >,
) {
    let file = &repo.0.files[explorer.selected];
    if let Ok((mut text, mut tint, mut node)) = label.single_mut() {
        let target = if !picker.is_open()
            && (navigation.overview() || listing::Listing::visible(&explorer, &navigation))
        {
            nearest
                .district
                .map(|index| {
                    (
                        repo.0.districts[index].name.as_str(),
                        district::beacon_ink(index),
                    )
                })
                .or_else(|| {
                    nearest
                        .file
                        .filter(|aim| {
                            navigation.overview()
                                || aim.index != explorer.selected
                                || aim.surface.is_none()
                                || nearest.file_distance > 24.
                        })
                        .map(|aim| aim.index)
                        .map(|index| {
                            let path = repo.0.files[index].path.as_str();
                            (path, file_facade::INK[file_facade::kind(path)])
                        })
                })
        } else {
            None
        };
        if let (Some((path, ink)), Some((point, size))) = (target, nearest.screen) {
            if text.0 != path {
                text.0 = path.into();
            }
            tint.set_if_neq(TextColor(color(ink)));
            let left = point.x <= size.x / 2.;
            let below = point.y <= size.y / 2.;
            node.display = Display::Flex;
            node.left = if left { px(point.x + 16.) } else { Val::Auto };
            node.right = if left {
                Val::Auto
            } else {
                px(size.x - point.x + 16.)
            };
            node.top = if below { px(point.y + 13.) } else { Val::Auto };
            node.bottom = if below {
                Val::Auto
            } else {
                px(size.y - point.y + 13.)
            };
            node.max_width = px((size.x / 2. - 24.).clamp(40., 460.));
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
    let (city, diffs, total_diffs) = progress.visited_counts();
    let total = repo.0.files.len();
    let filename = file.path.rsplit('/').next().unwrap_or(&file.path);
    let walk_hint = pilot.single().ok().and_then(|pilot| {
        if pilot.walk.is_some() {
            Some("Walking · O or Space to fly")
        } else {
            (time.elapsed_secs() < pilot.landing_warning_until)
                .then_some("Move over a street to land")
        }
    });
    #[cfg(target_arch = "wasm32")]
    {
        let hint = if navigation.overview() {
            navigation.prompt(&explorer)
        } else {
            walk_hint.unwrap_or_default().into()
        };
        browser_hud.publish(filename, [city, total, diffs, total_diffs], hint);
    }
    #[cfg(not(target_arch = "wasm32"))]
    for (gauge, mut node) in &mut gauges {
        let (done, total) = if gauge.0 {
            (diffs, total_diffs)
        } else {
            (city, total)
        };
        let width = percent(if total == 0 {
            0.
        } else {
            100. * done as f32 / total as f32
        });
        if node.width != width {
            node.width = width;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    for (mut text, mut node, top, bottom, status, toast, counter) in &mut texts {
        if let Some(counter) = counter {
            let value = if counter.0 {
                if total_diffs == 0 {
                    "Diff —".into()
                } else {
                    format!("Diff {diffs}/{total_diffs}")
                }
            } else {
                format!("City {city}/{total}")
            };
            text.set_if_neq(Text::new(value));
            continue;
        }
        let (review, live) = &native;
        let value = if top {
            let name = repository.github.as_ref().map_or_else(
                || {
                    repo.0
                        .root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                },
                native_github::Target::label,
            );
            if repo.0.is_worktree() {
                format!("{name} · WORKTREE")
            } else {
                name
            }
        } else if status {
            filename.to_owned()
        } else if toast {
            review
                .feedback
                .as_ref()
                .filter(|(deadline, _)| time.elapsed_secs() < *deadline)
                .map(|(_, message)| message.clone())
                .or_else(|| review.auto_publish.then(|| "Auto-publish on".into()))
                .unwrap_or_default()
        } else if bottom {
            live.last_touched(time.elapsed_secs_f64())
                .filter(|(_, age)| *age < 8)
                .map(|(path, _)| {
                    let filename = path.rsplit('/').next().unwrap_or(path);
                    format!("Updated {filename}    L to fly")
                })
                .or_else(|| walk_hint.map(str::to_owned))
                .unwrap_or_default()
        } else {
            continue;
        };
        let display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        if text.0 != value {
            text.0 = value;
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn smoke(
    mut commands: Commands,
    time: Res<Time>,
    mut check: ResMut<Smoke>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<&mut Transform, PlayerCamera>,
    panes: Query<&Transform, (With<district::Glass>, Without<Camera3d>)>,
    sections: Query<(&Section, &Transform), Without<Camera3d>>,
    fragments: Query<Entity, With<physics::Fragment>>,
    debris: Query<Entity, With<physics::Lifetime>>,
    mut listing: ResMut<listing::Listing>,
    mut picker: ResMut<repository_picker::RepositoryPicker>,
    mut session: ResMut<agent_session::Session>,
    window: Query<Entity, With<PrimaryWindow>>,
    (mut tablet, mut review, mut reading, mut audio, navigation): (
        ResMut<review_tablet::Tablet>,
        ResMut<review_notes::Review>,
        ResMut<reading::Settings>,
        ResMut<city_audio::Settings>,
        Res<city_navigation::Navigation>,
    ),
) {
    if check.pending || repo.0.files.is_empty() {
        return;
    }
    if check.stage >= check.stop {
        if explorer.dirty {
            let expected = explorer.original.clone();
            explorer.reload(&repo.0);
            assert_eq!(explorer.source, expected);
        }
        let report = if check.stop == 33 {
            "Rendered a comparison roof tour and moved to the next changed building without hunk floors."
        } else if check.stop == 31 {
            "Rendered both hunk button pages with reachable previous and next controls."
        } else if check.stop == 29 {
            "Rendered model choices and a refresh failure alongside reading and sound settings."
        } else if check.stop == 27 {
            "Rendered agent approval controls with a multiline draft, then the rejected request without running an agent."
        } else if check.stop == 25 {
            "Rendered the district overview above its uncapped buildings."
        } else if check.stop == 24 {
            "Rendered the Agent tablet."
        } else if check.stop == 23 {
            "Rendered frosted unread source, immediately legible guided ranges and their settled highlight without read credit."
        } else if check.stop == 20 {
            "Rendered readable explanations in the center of unchanged and unavailable diff walls."
        } else if check.stop == 18 {
            "Rendered guided reading, always-visible source and Follow agent controls without premature read credit."
        } else if check.stop == 15 {
            "Rendered the paint color picker."
        } else if check.stop == 14 {
            "Rendered the changed files tablet."
        } else if check.stop == 13 {
            "Rendered the review comment previews."
        } else if check.stop == 12 {
            "Rendered the controls sheet."
        } else if check.stop == 11 {
            "Rendered the GitHub picker."
        } else if check.stop == 10 {
            "Rendered the revealed source wall."
        } else {
            "Rendered pair, ghost, syntax wall, city overview, district mirror, diff and blame walls. The draft action detached the exact declaration, spawned physical fragments, and restore recovered the immutable source and cleared debris."
        };
        std::fs::write(check.directory.join("passed.txt"), report).expect("smoke report");
        println!("Native smoke passed. {report}");
        commands.write_message(AppExit::Success);
        return;
    }
    if !check.prepared {
        match check.stage {
            10 => picker.smoke_open(),
            11 => tablet.open_help(),
            12 => {
                picker.smoke_review();
                let path = &repo.0.files[explorer.selected].path;
                let preset = review.palette[4].as_ref().expect("Explain preset").clone();
                review.notes = [
                    ("smoke-one", 5, preset.prompt.clone()),
                    ("smoke-two", 9, "Please describe when this runs.".to_owned()),
                ]
                .into_iter()
                .map(|(id, start, text)| review_notes::Note {
                    id: id.into(),
                    revision: repo.0.revision.clone(),
                    baseline: repo.0.baseline.clone(),
                    source_blob: None,
                    anchor: review_notes::Anchor {
                        path: path.clone(),
                        before: false,
                        start,
                        end: start + 1,
                    },
                    preset: preset.clone(),
                    text,
                    published: None,
                    remote: false,
                    resolved: None,
                    attention: false,
                })
                .collect();
                review.request = Some(review_notes::Panel::Notes);
            }
            13 => {
                use bevy::input::{
                    ButtonState,
                    keyboard::{Key, KeyboardInput},
                };
                commands.write_message(KeyboardInput {
                    key_code: KeyCode::Tab,
                    logical_key: Key::Tab,
                    text: None,
                    state: ButtonState::Pressed,
                    repeat: false,
                    window: window.single().expect("primary smoke window"),
                });
            }
            14 => commands.queue(|world: &mut World| {
                world
                    .resource_mut::<review_notes::ReviewUi>()
                    .smoke_edit_paint();
            }),
            31 | 32 => {
                commands.write_message(bevy::input::keyboard::KeyboardInput {
                    key_code: if check.stage == 31 {
                        KeyCode::KeyC
                    } else {
                        KeyCode::ArrowUp
                    },
                    logical_key: bevy::input::keyboard::Key::Unidentified(
                        bevy::input::keyboard::NativeKey::Unidentified,
                    ),
                    text: None,
                    state: bevy::input::ButtonState::Pressed,
                    repeat: false,
                    window: window.single().expect("smoke window"),
                });
            }
            _ => {}
        }
        if let Ok(mut camera) = camera.single_mut() {
            let [x, z] = repo.0.files[explorer.selected].position;
            match check.stage {
                0 => {
                    explorer.comparison_mode = 1;
                    explorer.page = explorer
                        .comparison
                        .as_ref()
                        .and_then(|c| c.hunks.first())
                        .map_or(0, |h| h.row / 23);
                    comparison_scene::focus(&mut camera, &repo.0, &explorer);
                }
                1 => {
                    explorer.comparison_mode = 2;
                    let base = comparison_scene::base(&repo.0.files[explorer.selected]);
                    *camera = Transform::from_xyz(x + 19., base + 19., z + 28.)
                        .looking_at(Vec3::new(x, base + 6., z), Vec3::Y);
                }
                2 => {
                    explorer.comparison_mode = 0;
                    explorer.function = explorer.anatomy.as_ref().and_then(|a| {
                        a.callables
                            .iter()
                            .position(|c| c.name == "createFrameScheduler")
                            .or_else(|| (!a.callables.is_empty()).then_some(0))
                    });
                    explorer.page = 0;
                    explorer.function_page = explorer.function.unwrap_or(0) / 8;
                    explorer.anchored = true;
                    *camera = Transform::from_xyz(x, 6.2, z + 14.5)
                        .looking_at(Vec3::new(x, 6.2, z + 3.2), Vec3::Y);
                }
                3 => {
                    let (section, _) = sections
                        .iter()
                        .find(|(section, _)| Some(section.index) == explorer.function)
                        .or_else(|| sections.iter().next())
                        .expect("smoke requires an extractable function section");
                    let callable = &explorer.anatomy.as_ref().unwrap().callables[section.index];
                    let mut expected = explorer.source.clone();
                    expected.replace_range(callable.start_byte..callable.end_byte, "");
                    check.expected_draft = Some(expected);
                    explorer.function = Some(section.index);
                    explorer.detach_selection(&repo.0);
                    // Keep the camera in the street between rows.
                    *camera = Transform::from_xyz(x - 4., 6.2, z + 14.5)
                        .looking_at(Vec3::new(x - 4., 6.2, z + 3.3), Vec3::Y);
                }
                4 => {
                    let expected = explorer.original.clone();
                    explorer.reload(&repo.0);
                    assert_eq!(explorer.source, expected);
                    for entity in &debris {
                        commands.entity(entity).despawn();
                    }
                    explorer.function = None;
                    explorer.comparison_mode = 1;
                    explorer.page = 0;
                    comparison_scene::focus(&mut camera, &repo.0, &explorer);
                }
                5 => {
                    explorer.comparison_mode = 0;
                    explorer.anchored = false;
                    let target = Vec3::new(x, 21., z);
                    *camera = Transform::from_translation(target + Vec3::new(64., 68., 100.))
                        .looking_at(target, Vec3::Y);
                }
                6 => {
                    let pane = panes
                        .iter()
                        .min_by(|a, b| {
                            let file = Vec2::new(x, z);
                            a.translation
                                .xz()
                                .distance_squared(file)
                                .total_cmp(&b.translation.xz().distance_squared(file))
                        })
                        .expect("smoke repository has a district mirror");
                    *camera = Transform::from_translation(
                        pane.translation + pane.rotation * Vec3::new(2., 2., 12.),
                    )
                    .looking_at(pane.translation, Vec3::Y);
                }
                9 => {
                    let file = &repo.0.files[explorer.selected];
                    let count = explorer.highlighted.len();
                    explorer.anchored = false;
                    explorer.comparison_mode = 0;
                    for row in (0..count).step_by(listing::TILE_ROWS) {
                        listing.reveal(
                            explorer.selected,
                            explorer.before,
                            file,
                            row..(row + listing::TILE_ROWS).min(count),
                            count,
                        );
                    }
                    *camera = listing::view(file, count);
                }
                15 | 16 => {
                    explorer.anchored = false;
                    explorer.comparison_mode = 0;
                    reading.min_seconds = Some(60.);
                    reading.max_seconds = Some(60.);
                    reading.mode = reading::Mode::Delayed;
                    reading.show_all_source = check.stage == 16;
                    listing.reset_view();
                    if check.stage == 15 {
                        listing.present_source(&explorer);
                    }
                    *camera = listing::Listing::line_view(
                        &explorer,
                        &repo.0.files[explorer.selected],
                        listing::Face::Source,
                        1,
                    )
                    .expect("guidance source position");
                }
                17 | 23 => tablet.open_page(review_tablet::Page::Agent),
                25 => {
                    session.smoke_controls(&repo.0.root, true);
                    tablet.request_close();
                }
                26 => {
                    session.smoke_controls(&repo.0.root, false);
                    tablet.open_page(review_tablet::Page::Agent);
                }
                27 | 28 => {
                    session.smoke_models(check.stage == 28);
                    reading.enabled = check.stage == 28;
                    audio.muted = check.stage == 27;
                    tablet.open_page(review_tablet::Page::Settings);
                }
                29 | 30 => {
                    let diff = explorer
                        .comparison
                        .as_ref()
                        .expect("hunk page smoke needs a comparison");
                    assert!(
                        diff.hunks.len() > 8,
                        "hunk page smoke needs more than eight hunks"
                    );
                    let hunk = if check.stage == 29 { 7 } else { 8 };
                    let row = diff.hunks[hunk].row;
                    explorer.hunk = hunk;
                    explorer.page = row / 23;
                    explorer.comparison_mode = 1;
                    explorer.anchored = true;
                    explorer.rebuild = true;
                    comparison_scene::focus(&mut camera, &repo.0, &explorer);
                }
                24 => {
                    explorer.anchored = false;
                    explorer.comparison_mode = 0;
                    let path = &repo.0.files[explorer.selected].path;
                    let name = path
                        .rsplit_once('/')
                        .map_or("/", |(directory, _)| directory);
                    let district = repo
                        .0
                        .districts
                        .iter()
                        .find(|district| district.name == name)
                        .unwrap_or(&repo.0.districts[0]);
                    *camera = city_navigation::district_view(district, &repo.0, 1.6);
                }
                20 | 21 | 22 => {
                    explorer.anchored = false;
                    explorer.comparison_mode = 0;
                    reading.show_all_source = false;
                    reading.min_seconds = Some(60.);
                    reading.max_seconds = Some(60.);
                    listing.reset_view();
                    if check.stage > 20 {
                        listing.present_range(&explorer, 24, 26);
                    }
                    *camera = listing::Listing::line_view(
                        &explorer,
                        &repo.0.files[explorer.selected],
                        listing::Face::Source,
                        25,
                    )
                    .expect("frost source wall");
                }
                18 | 19 => {
                    explorer.anchored = false;
                    explorer.comparison_mode = 0;
                    explorer.comparison = if check.stage == 18 {
                        Some(
                            Comparison::new(
                                &repo.0.files[explorer.selected].path,
                                &explorer.original,
                                &explorer.original,
                            )
                            .expect("unchanged smoke comparison"),
                        )
                    } else {
                        None
                    };
                    explorer.refresh_diff_layout();
                    listing.reset_view();
                    *camera =
                        listing::smoke_side(&explorer, &repo.0.files[explorer.selected], false);
                }
                7 | 8 => {
                    *camera = listing::smoke_side(
                        &explorer,
                        &repo.0.files[explorer.selected],
                        check.stage == 8,
                    );
                }
                _ => {}
            }
        }
        explorer.rebuild = true;
        check.elapsed = 0.;
        check.prepared = true;
        return;
    }
    if matches!(check.stage, 20..=22)
        && let Ok(mut camera) = camera.single_mut()
    {
        *camera = listing::Listing::line_view(
            &explorer,
            &repo.0.files[explorer.selected],
            listing::Face::Source,
            25,
        )
        .expect("stable frost view");
    }
    let previous = check.elapsed;
    check.elapsed += time.delta_secs();
    if check.stage == 10 && previous < 1. && check.elapsed >= 1. {
        use bevy::input::{
            ButtonState,
            keyboard::{Key, KeyboardInput, NativeKeyCode},
        };
        let text = "привет";
        commands.write_message(KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: Key::Character(text.into()),
            text: Some(text.into()),
            state: ButtonState::Pressed,
            repeat: false,
            window: window.single().expect("primary smoke window"),
        });
    }
    if matches!(check.stage, 31 | 32) && (!navigation.reviewing() || navigation.is_flying()) {
        assert!(check.elapsed < 30., "roof review smoke timed out");
        return;
    }
    let settle = if check.stage == 21 { 0.8 } else { 4. };
    if check.elapsed < settle {
        return;
    }
    if check.stage == 3 {
        assert!(explorer.dirty, "draft action did not detach a declaration");
        assert_eq!(
            Some(&explorer.source),
            check.expected_draft.as_ref(),
            "draft action detached the wrong declaration"
        );
        assert!(
            !fragments.is_empty(),
            "draft action did not spawn physical fragments"
        );
    }
    if check.stage == 4 {
        assert!(!explorer.dirty);
        assert!(debris.is_empty(), "restore left physical debris behind");
    }
    if check.stage == 8 {
        assert!(
            listing::smoke_blame_ready(&listing, explorer.highlighted.len()),
            "blame rows missing"
        );
    }
    if check.stage == 9 {
        assert!(listing::smoke_source_ready(&listing), "source rows missing");
    }
    if matches!(check.stage, 20..=22) {
        assert!(
            listing::smoke_source_ready(&listing),
            "frost source tiles missing"
        );
        assert_eq!(
            listing.counts(&explorer).0,
            0,
            "frost presentation granted read credit"
        );
    }
    if matches!(check.stage, 18 | 19) {
        assert!(
            listing::smoke_empty_diff_ready(&listing),
            "empty diff body explanation missing"
        );
    }
    if matches!(check.stage, 15 | 16) {
        let file = &repo.0.files[explorer.selected];
        let (_, lines) = listing
            .visible_wall_lines(file, camera.single().expect("smoke camera"), &explorer, 1.6)
            .expect("guided source wall in view");
        assert!(
            lines.iter().any(|line| line.readable),
            "guided code stayed covered"
        );
        assert_eq!(
            listing.counts(&explorer).0,
            0,
            "preview granted premature read credit"
        );
    }
    if check.stage == 25 {
        commands.queue(agent_session::Session::check_smoke_banner);
    }
    if matches!(check.stage, 31 | 32) {
        assert_eq!(explorer.comparison_mode, 1);
        assert!(
            explorer
                .comparison
                .as_ref()
                .is_some_and(|diff| !diff.hunks.is_empty())
        );
        let current = camera.single().expect("roof review camera");
        let mut facing_diff = *current;
        comparison_scene::focus(&mut facing_diff, &repo.0, &explorer);
        assert!(
            current.forward().dot(*facing_diff.forward()) > 0.98,
            "review arrows pulled the camera away from the diff"
        );
        if check.stage == 32 {
            assert_ne!(check.review_file, Some(explorer.selected));
        }
        check.review_file = Some(explorer.selected);
        commands.queue(|world: &mut World| {
            assert_eq!(
                world
                    .query_filtered::<Entity, With<city_navigation::RouteVisual>>()
                    .iter(world)
                    .count(),
                0,
                "roof reviews must not spawn hunk floors"
            );
        });
    }
    std::fs::create_dir_all(&check.directory).expect("smoke output");
    let path = check.directory.join(format!("{:02}.png", check.stage));
    check.pending = true;
    commands.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>, mut check: ResMut<Smoke>| {
            let image = event
                .image
                .clone()
                .try_into_dynamic()
                .expect("screenshot image");
            let rgba = image.to_rgba8();
            let visible_pixels = rgba
                .pixels()
                .filter(|p| p.0[..3].iter().any(|v| *v > 32))
                .count();
            check.pending = false;
            check.elapsed = 0.;
            // A close view is mostly dark paper; only the listing ink must register.
            let divisor = if check.stage >= 7 { 1000 } else { 100 };
            if visible_pixels < (rgba.width() as usize * rgba.height() as usize) / divisor {
                if check.stage >= 7 {
                    image
                        .save(path.with_extension("dark.png"))
                        .expect("save diagnostic screenshot");
                }
                check.capture_attempts += 1;
                assert!(
                    check.capture_attempts < 8,
                    "No rendered frame: bring the smoke window to the foreground"
                );
                return;
            }
            image.save(&path).expect("save screenshot");
            check.capture_attempts = 0;
            check.prepared = false;
            check.stage += 1;
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn default_ui_font_has_cyrillic_glyphs_for_editable_text_and_picker_rows() {
        let mut fonts = Assets::<Font>::default();
        let default_font = Handle::<Font>::default();
        fonts
            .insert(
                default_font.id(),
                Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec()),
            )
            .unwrap();
        load_ui_font(&mut fonts);
        let font =
            swash::FontRef::from_index(fonts.get(&default_font).unwrap().data.as_ref(), 0).unwrap();
        for character in "ПриветЁжЪыЇєҐў·".chars() {
            assert_ne!(
                font.charmap().map(character),
                0,
                "missing glyph for {character}"
            );
        }
    }
    #[test]
    fn ray_selects_frontmost_positive_intersection() {
        for normal in [Vec3::Z, Vec3::X, Vec3::NEG_X, Vec3::NEG_Z, Vec3::Y] {
            assert_eq!(
                ray_box_entry(normal * 10., -normal, Vec3::splat(-1.), Vec3::splat(1.)),
                Some((9., normal))
            );
        }
        assert!(
            ray_box(
                Vec3::new(5., 0., 10.),
                Vec3::NEG_Z,
                Vec3::splat(-1.),
                Vec3::splat(1.)
            )
            .is_none()
        );
    }
}

#[cfg(test)]
mod flight_speed_tests {
    #[test]
    fn flying_above_roofs_speeds_up_without_changing_wall_controls() {
        use super::flight_speed;
        assert_eq!(flight_speed(20., false, false), 13.);
        assert_eq!(flight_speed(250., true, false), 3.);
        assert!(flight_speed(250., false, false) > 50.);
        assert!(flight_speed(450., false, true) > flight_speed(250., false, false));
        assert_eq!(
            flight_speed(1000., false, false),
            flight_speed(2000., false, false)
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod cursor_visibility_tests {
    use super::*;
    #[test]
    fn capture_hides_cursor_after_late_panel_writes_and_focus_loss_releases_it() {
        let mut app = App::new();
        app.add_systems(PostUpdate, synchronize_cursor);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                CursorOptions::default(),
                PrimaryWindow,
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        app.update();
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::Locked
        );
        // A UI action wrote the free cursor before the final synchronization pass.
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .visible = true;
        app.update();
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::None
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut().get_mut::<Pilot>(camera).unwrap().captured = false;
        app.update();
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
    }
}
