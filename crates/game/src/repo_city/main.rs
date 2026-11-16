//! Repository explorer and the code city's presentation.
#![allow(dead_code)]
mod android;
mod blame;
mod city_menu;
mod city_navigation;
mod code_texture;
mod comparison_scene;
mod district;
mod file_facade;
mod file_picker;
mod guide;
mod listing;
mod physics;
mod progress;
mod repository_picker;
mod review_notes;
#[path = "../shapes.rs"]
mod shapes;
mod world_set;

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
#[cfg(not(target_arch = "wasm32"))]
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use repo_world::{Change, FileAnatomy, Repository};
use repo_world::{comparison::Comparison, syntax};
use std::{collections::HashSet, path::PathBuf};
use world_set::{WorldSet, color};

type PlayerCamera = (With<Camera3d>, Without<district::MirrorCamera>);

const CAMERA_FLOOR: f32 = 0.05;

#[derive(Resource)]
struct Repo(Repository);
#[derive(Resource)]
struct Explorer {
    selected: usize,
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
    comparison: Option<Comparison>,
    comparison_mode: u8,
    hunk: usize,
    pending_fracture: bool,
    frame_loaded_source: bool,
}
impl Explorer {
    fn new(index: usize) -> Self {
        Self {
            selected: index,
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
            comparison: None,
            comparison_mode: 0,
            hunk: 0,
            pending_fracture: false,
            frame_loaded_source: true,
        }
    }
    fn open(&mut self, repo: &Repository, index: usize) {
        self.frame_loaded_source = true;
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
                self.notice = "Source loaded from the selected commit.".into();
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
    fn refresh(&mut self, repo: &Repository) {
        let file = &repo.files[self.selected];
        self.highlighted = syntax::highlight(&file.path, &self.source);
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
        self.comparison = match (old, new) {
            (Ok(old), Ok(new)) => match Comparison::new(&file.path, &old, &new) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.notice = format!("Diff unavailable: {e}");
                    None
                }
            },
            _ => None,
        };
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
    facade_closed: Handle<StandardMaterial>,
    facade_open: Handle<StandardMaterial>,
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
#[derive(Component)]
struct TopHud;
#[derive(Component)]
struct BottomHud;
#[derive(Component)]
struct StatusHud;
#[derive(Component)]
struct AimHud;
#[derive(Component)]
struct TargetLabel;
#[derive(Component, Default)]
struct Pilot {
    captured: bool,
    fire_ready: bool,
    firing: bool,
    shot: bool,
}
impl Pilot {
    fn update_fire(&mut self, mouse: &ButtonInput<MouseButton>) {
        let pressed = mouse.pressed(MouseButton::Left);
        let clicked = mouse.just_pressed(MouseButton::Left);
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
    directory: PathBuf,
    elapsed: f32,
    stage: usize,
    pending: bool,
    prepared: bool,
    expected_draft: Option<String>,
    capture_attempts: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        args.windows(2)
            .find(|p| p[0] == name)
            .map(|p| p[1].as_str())
    };
    if args.iter().any(|a| a == "--help") {
        println!(
            "repo-city --repo PATH [--rev HEAD] [--base REF] [--trace FILE] [--file PATH] [--world-set FILE] [--smoke-dir PATH]\nWASD fly · E flies to the aimed face · Space/Ctrl altitude · Shift boost · Click captures the cursor · Esc releases it\nHold left mouse close to wash source · Home frames the file · G diff · B blame\nC reviews changes · T follows a supplied stack · W/S fly between platforms · Esc returns to the city\nG ghost/pair · [ ] hunks · Enter source wall · Q before/after · Scroll/PgUp/PgDn read\nF6 exports the draft to this project's sandbox"
        );
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let explicit = arg("--repo").map(PathBuf::from);
    #[cfg(not(target_arch = "wasm32"))]
    let root = explicit.clone().or_else(repository_picker::last_folder);
    #[cfg(not(target_arch = "wasm32"))]
    let mut startup_error = String::new();
    #[cfg(target_arch = "wasm32")]
    let startup_error = String::new();
    #[cfg(not(target_arch = "wasm32"))]
    let repo = match root {
        Some(root) => {
            match repository_picker::load(&root, arg("--rev").unwrap_or("HEAD"), arg("--base")) {
                Ok(repo) => repo,
                Err(error) if explicit.is_none() => {
                    startup_error = error;
                    repository_picker::empty()
                }
                Err(error) => {
                    eprintln!("Repository: {error}");
                    std::process::exit(1)
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
    let set = WorldSet::load(arg("--world-set")).unwrap_or_else(|e| {
        eprintln!("World set: {e}");
        std::process::exit(1)
    });
    let initial = arg("--file")
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
            "REPO CITY: {} files, {} districts, commit {}",
            repo.files.len(),
            repo.districts.len(),
            &repo.revision[..8]
        );
    }
    let mut app = App::new();
    let navigation =
        city_navigation::Navigation::new(&repo, arg("--trace")).unwrap_or_else(|error| {
            eprintln!("Trace: {error}");
            std::process::exit(1)
        });
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
    .add_plugins((PhysicsPlugins::default(), listing::CoverPlugin))
    .insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.64, 0.82, 0.85),
        brightness: 550.,
        ..default()
    })
    .insert_resource(Repo(repo))
    .insert_resource(set)
    .insert_resource(explorer)
    .insert_resource(navigation)
    .insert_resource(repository_picker::RepositoryPicker::new(startup_error))
    .init_resource::<Nearest>()
    .init_resource::<city_menu::Menu>()
    .init_resource::<file_picker::FilePicker>()
    .init_resource::<file_facade::ChangesOnly>()
    .init_resource::<listing::Listing>()
    .init_resource::<listing::find::Search>()
    .init_resource::<progress::Progress>()
    .init_resource::<review_notes::Review>()
    .init_resource::<guide::Tour>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            (file_facade::lower_unchanged, file_picker::update).chain(),
            flight,
            (file_facade::keep_beams_clear, targeting).chain(),
            (city_menu::update, listing::find::update).chain(),
            (review_notes::update, review_notes::panels).chain(),
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
            listing::wash_stream
                .run_if(city_menu::gameplay_enabled)
                .run_if(listing::find::movement_enabled),
            (
                guide::advance,
                listing::paint_notes,
                listing::find::mark,
                android::show_sprayer,
            )
                .chain(),
            physics::splashes,
            city_navigation::shoot_reviewed_platform,
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
        (
            repository_picker::open_repository,
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
    #[cfg(not(target_arch = "wasm32"))]
    app.init_resource::<guide::Guide>()
        .add_systems(Update, guide::update.before(flight));
    app.add_systems(Startup, district::setup);
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
            app.insert_resource(Smoke {
                directory: directory.into(),
                elapsed: 0.,
                stage: std::env::args()
                    .find_map(|arg| match arg.as_str() {
                        "--smoke-walls" => Some(7),
                        "--smoke-blame" => Some(8),
                        _ => None,
                    })
                    .unwrap_or(0),
                pending: false,
                prepared: false,
                expected_draft: None,
                capture_attempts: 0,
            })
            .add_systems(Update, smoke.after(animate));
        }
    }
    app.run();
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
    mut camera: Query<&mut Transform, PlayerCamera>,
) {
    let current = repo_world::browser_source_version();
    if current != *version {
        *version = current;
        if explorer.anatomy.is_none() || (explorer.comparison.is_none() && !explorer.dirty) {
            let page = explorer.page;
            let awaiting_source = explorer.anatomy.is_none();
            explorer.reload(&repo.0);
            listing.invalidate();
            explorer.page = page;
            if awaiting_source
                && explorer.frame_loaded_source
                && !explorer.highlighted.is_empty()
                && let Ok(mut camera) = camera.single_mut()
            {
                let file = &repo.0.files[explorer.selected];
                let pending = listing::view(file, 0);
                // Frame the first source only if the player stayed at its loading pose.
                if camera.translation.distance_squared(pending.translation) < 0.0001
                    && camera.rotation.dot(pending.rotation).abs() > 0.99999
                {
                    *camera = listing::view(file, explorer.highlighted.len());
                }
            }
        }
    }
    let syntax_version = repo_world::browser_syntax_version();
    if syntax_version != *styling {
        *styling = syntax_version;
        let file = &repo.0.files[explorer.selected];
        explorer.highlighted = syntax::highlight(&file.path, &explorer.source);
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
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf").to_vec(),
    ));
    let unit = meshes.add(shapes::rounded_box(Vec3::ONE, 0.035));
    let shell = materials.add(StandardMaterial {
        base_color: color(set.shell),
        metallic: 0.6,
        perceptual_roughness: 0.35,
        ..default()
    });
    let [facade_closed, facade_open] =
        [code_texture::Coverage::Closed, code_texture::Coverage::Open].map(|coverage| {
            materials.add(listing::overview_material(
                images.add(code_texture::facade_overview(std::iter::once(coverage))),
            ))
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
        facade_closed,
        facade_open,
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
            Msaa::Sample4,
            Transform::default(),
            Pilot::default(),
        ))
        .id();
    android::build(&mut commands, &mut meshes, &mut materials, pilot);
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.4, 0.)),
    ));
    let ui_font = |size| TextFont {
        font: bevy::text::FontSource::Handle(font.clone()),
        font_size: FontSize::Px(size),
        ..default()
    };
    file_picker::spawn(&mut commands, &ui_font(13.), color(set.ink));
    city_menu::spawn(&mut commands, &ui_font(13.), color(set.ink));
    guide::spawn_hint(&mut commands, &ui_font(13.), color(set.ink));
    listing::find::spawn(&mut commands, &ui_font(13.), color(set.ink));
    #[cfg(not(target_arch = "wasm32"))]
    guide::spawn(&mut commands, &ui_font(15.), color(set.ink));
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
    repository_picker::spawn(&mut commands, &ui_font(13.), color(set.ink));
    commands.spawn((
        Text::new("REPO CITY"),
        ui_font(18.),
        TextColor(color(set.ink)),
        Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            top: px(if cfg!(target_arch = "wasm32") {
                75.
            } else {
                20.
            }),
            ..default()
        },
        TopHud,
    ));
    commands.spawn((
        Text::new(""),
        ui_font(14.),
        TextColor(Color::srgb(0.7, 0.8, 0.82)),
        BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.94)),
        Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            bottom: px(22.),
            padding: UiRect::all(px(14.)),
            ..default()
        },
        BottomHud,
    ));
    commands.spawn((
        Text::new(""),
        ui_font(13.),
        TextColor(color(set.ink)),
        BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.91)),
        Node {
            position_type: PositionType::Absolute,
            right: px(24.),
            top: px(if cfg!(target_arch = "wasm32") {
                65.
            } else {
                20.
            }),
            max_width: px(460.),
            padding: UiRect::all(px(14.)),
            ..default()
        },
        StatusHud,
    ));
    commands.spawn((
        Text::new("+"),
        ui_font(24.),
        TextColor(color(set.ink)),
        Node {
            position_type: PositionType::Absolute,
            left: percent(50.),
            top: percent(50.),
            ..default()
        },
        AimHud,
    ));
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

fn spawn_city(
    mut commands: Commands,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    explorer: Res<Explorer>,
    palette: Res<Palette>,
    mut camera: Query<(&mut Transform, &mut Pilot), PlayerCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !repo.is_changed() || repo.0.files.is_empty() {
        return;
    }
    let unit = &palette.unit;
    let plate = &palette.plate;
    let shell = &palette.shell;
    if let Ok((mut pose, mut pilot)) = camera.single_mut() {
        *pose = listing::view(&repo.0.files[explorer.selected], explorer.highlighted.len());
        pilot.captured = false;
    }
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
    let lights = district::Lights::new(&mut meshes, &mut materials);
    for (index, district) in repo.0.districts.iter().enumerate() {
        let [x, z] = district.position;
        let [w, d] = district.size;
        commands.spawn((
            CityObject,
            Mesh3d(unit.clone()),
            MeshMaterial3d(shell.clone()),
            Transform::from_xyz(x + w / 2. - 3., -0.12, z + d / 2. - 1.)
                .with_scale(Vec3::new(w, 0.18, d)),
        ));
        lights.spawn(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            district,
            index,
        );
    }
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
    use file_facade::{Part, transform};
    let mirror = meshes.add(Rectangle::new(1., 1.));
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.065, 0.10, 0.14),
        unlit: true,
        ..default()
    });
    let beams = file_facade::INK.map(|ink| materials.add(file_facade::beam_material(ink)));
    for (i, file) in repo.0.files.iter().enumerate() {
        let size = listing::block_size(file);
        let body = commands
            .spawn((
                CityObject,
                Facade { index: i },
                Mesh3d(block.clone()),
                MeshMaterial3d(palette.facade_closed.clone()),
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
                Mesh3d(rim.clone()),
                MeshMaterial3d(palette.facade_rims[0].clone()),
                transform(file, size, Part::Roof),
            ))
            .id();
        let status = match file.change {
            Change::Added => &added,
            Change::Removed => &removed,
            Change::Modified => &modified,
            Change::Unchanged => &unchanged,
        };
        let stripes = [-1., 1.].map(|face| {
            let part = Part::Stripe(face);
            let stripe = commands
                .spawn((
                    CityObject,
                    Mesh3d(block.clone()),
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
                Mesh3d(block.clone()),
                MeshMaterial3d(beams[kind].clone()),
                transform(file, size, Part::Beam),
            ))
            .id();
        let pane = commands
            .spawn((
                CityObject,
                district::Glass,
                bevy::light::NotShadowCaster,
                Mesh3d(mirror.clone()),
                MeshMaterial3d(glass.clone()),
                transform(file, size, Part::Mirror),
            ))
            .id();
        if file.change == Change::Unchanged {
            for (entity, part) in [
                (body, Part::Body),
                (roof, Part::Roof),
                (sign, Part::Label),
                (beam, Part::Beam),
                (pane, Part::Mirror),
            ]
            .into_iter()
            .chain(stripes)
            {
                commands
                    .entity(entity)
                    .insert(file_facade::SinkPart::new(file, size, part));
            }
        }
    }
}

fn flight(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut explorer: ResMut<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    mut camera: Query<(&mut Transform, &mut Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    #[cfg(not(target_arch = "wasm32"))] mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    listing: Res<listing::Listing>,
    file_buttons: Query<&bevy::picking::hover::Hovered, With<file_picker::FileButton>>,
    review: Res<review_notes::Review>,
    search: Res<listing::find::Search>,
    #[cfg(target_arch = "wasm32")] mut previous_pointer_ui: Local<Option<bool>>,
) {
    let Ok((mut camera, mut pilot)) = camera.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let pointer_ui_open = picker.is_open()
        || menu.is_open()
        || review.blocked
        || search.blocks_controls()
        || navigation.overview();
    let was_captured = pilot.captured;
    let over_button = !was_captured && file_buttons.iter().any(|hovered| hovered.get());
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let pointer_ui = pointer_ui_open || over_button;
            if *previous_pointer_ui != Some(pointer_ui)
                && let Some(canvas) = document.get_element_by_id("repo-city")
            {
                if pointer_ui {
                    let _ = canvas.set_attribute("data-pointer-ui", "");
                } else {
                    let _ = canvas.remove_attribute("data-pointer-ui");
                }
                *previous_pointer_ui = Some(pointer_ui);
            }
            pilot.captured = document
                .pointer_lock_element()
                .is_some_and(|element| element.id() == "repo-city");
            if pilot.captured && pointer_ui_open {
                document.exit_pointer_lock();
                pilot.captured = false;
            }
        }
    }
    let focused = window.focused || (cfg!(target_arch = "wasm32") && pilot.captured);
    #[cfg(not(target_arch = "wasm32"))]
    if focused && !pointer_ui_open && !over_button {
        if keys.just_pressed(KeyCode::Tab) {
            pilot.captured = !pilot.captured;
        } else if mouse.just_pressed(MouseButton::Left) {
            pilot.captured = true;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        pilot.captured = false;
        if !menu.is_open() {
            explorer.anchored = false;
        }
    }
    if !focused || pointer_ui_open {
        pilot.captured = false;
    }
    // Updating CursorOptions on the web reapplies winit's grab mode (None)
    // and would release the browser lock. CSS handles visibility there.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(mut cursor) = cursors.single_mut() {
        let grab_mode = if pilot.captured {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        if cursor.grab_mode != grab_mode {
            cursor.grab_mode = grab_mode;
        }
        if cursor.visible == pilot.captured {
            cursor.visible = !pilot.captured;
        }
    }
    pilot.update_fire(&mouse);
    if pointer_ui_open || listing.turning() || !focused {
        return;
    }
    if navigation.is_flying() {
        return;
    }
    // This frame may contain movement from before the capture click.
    if was_captured && pilot.captured && motion.delta != Vec2::ZERO {
        let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        let yaw = yaw - motion.delta.x * 0.0025;
        let pitch = (pitch - motion.delta.y * 0.0025).clamp(-1.5, 1.5);
        camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.);
    }
    if navigation.guides_camera() {
        return;
    }
    let axis = |p, n| f32::from(keys.pressed(p)) - f32::from(keys.pressed(n));
    let mut direction = camera.forward().as_vec3() * axis(KeyCode::KeyW, KeyCode::KeyS)
        + camera.right().as_vec3() * axis(KeyCode::KeyD, KeyCode::KeyA)
        + Vec3::Y * axis(KeyCode::Space, KeyCode::ControlLeft);
    if explorer.anchored {
        direction = Vec3::new(
            axis(KeyCode::KeyD, KeyCode::KeyA),
            axis(KeyCode::KeyW, KeyCode::KeyS),
            0.,
        );
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        42.
    } else if explorer.anchored {
        3.
    } else {
        13.
    };
    let movement = direction.clamp_length_max(1.) * speed * time.delta_secs();
    camera.translation += movement;
    camera.translation.y = camera.translation.y.max(CAMERA_FLOOR);
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
            transform.translation - transform.scale / 2.,
            transform.translation + transform.scale / 2.,
        ) && d < distance
        {
            distance = d;
            nearest.hunk = Some(hunk.0);
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
    navigation: Res<city_navigation::Navigation>,
    picker: Res<file_picker::FilePicker>,
    mut progress: ResMut<progress::Progress>,
) {
    if picker.is_open() {
        return;
    }
    if navigation.guides_camera() {
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
    if keys.just_pressed(KeyCode::KeyX) {
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
    set: Res<WorldSet>,
    explorer: Res<Explorer>,
    nearest: Res<Nearest>,
    navigation: Res<city_navigation::Navigation>,
    listing: Res<listing::Listing>,
    progress: Res<progress::Progress>,
    tour: Res<guide::Tour>,
    picker: Res<file_picker::FilePicker>,
    mut label: Query<(&mut Text, &mut TextColor, &mut Node), With<TargetLabel>>,
    mut texts: Query<
        (
            &mut Text,
            Has<TopHud>,
            Has<BottomHud>,
            Has<StatusHud>,
            Has<AimHud>,
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
    for (mut text, top, bottom, status, aim) in &mut texts {
        let value = if top {
            let title = repo
                .0
                .root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let name = if cfg!(target_arch = "wasm32") {
                repo.0.root.to_string_lossy().into_owned()
            } else {
                format!("{}  /  {title}", set.name)
            };
            format!(
                "{name}\n{} files · {} districts · {} → {}",
                repo.0.files.len(),
                repo.0.districts.len(),
                repo.0.baseline.as_ref().map_or("empty", |s| &s[..8]),
                &repo.0.revision[..8]
            )
        } else if bottom {
            if navigation.overview() {
                format!(
                    "District view · Click or E flies to a file · {} finds files · M returns",
                    file_picker::SHORTCUT
                )
            } else if tour.active() {
                String::new()
            } else {
                navigation.prompt(&explorer)
            }
        } else if status && listing::Listing::visible(&explorer, &navigation) {
            let (opened, total) = listing.counts(&explorer);
            let anatomy = explorer
                .anatomy
                .as_ref()
                .filter(|anatomy| !anatomy.callables.is_empty());
            let metrics = anatomy.map_or_else(String::new, |anatomy| {
                let branches = anatomy
                    .callables
                    .iter()
                    .filter_map(|callable| callable.decision_count)
                    .max();
                let longest = anatomy
                    .callables
                    .iter()
                    .filter_map(|callable| callable.body_line_span)
                    .max();
                format!(
                    "\n{} functions · peak branches {} · longest body {} lines",
                    anatomy.callables.len(),
                    branches.map_or("—".into(), |value| value.to_string()),
                    longest.map_or("—".into(), |value| value.to_string())
                )
            });
            format!(
                "{}\n{:?} · {}\n{opened} / {total} lines revealed · {} / {} files reviewed\n{}{metrics}\n{}",
                file.path,
                file.change,
                if explorer.before { "BEFORE" } else { "AFTER" },
                explorer.reviewed.len(),
                repo.0.files.len(),
                progress.exploration_label(repo.0.files.len()),
                explorer.notice
            )
        } else if status {
            let count = explorer.anatomy.as_ref().map_or(0, |a| a.callables.len());
            let title = explorer
                .function
                .and_then(|i| explorer.anatomy.as_ref()?.callables.get(i))
                .map(|c| c.display_name.as_ref().unwrap_or(&c.name).as_str())
                .unwrap_or("whole file");
            format!(
                "{}\n{:?}  ·  {}{}\n{} functions · {} lines · page {}\n{}\n\n{}\n\n{} / {} files reviewed\n{}{}",
                file.path,
                file.change,
                if explorer.before { "BEFORE" } else { "AFTER" },
                if explorer.dirty {
                    " / PRIVATE DRAFT"
                } else {
                    ""
                },
                count,
                explorer.source.lines().count(),
                explorer.page + 1,
                title,
                explorer.notice,
                explorer.reviewed.len(),
                repo.0.files.len(),
                progress.exploration_label(repo.0.files.len()),
                format!(
                    "\n{} hunks · {}",
                    explorer.comparison.as_ref().map_or(0, |c| c.hunks.len()),
                    navigation
                        .route_status()
                        .unwrap_or_else(|| match explorer.comparison_mode {
                            1 => "TWO BUILDINGS",
                            2 => "GHOST OVERLAY",
                            _ if explorer.anchored => "SOURCE WALL",
                            _ => "EXPLORING CITY",
                        }
                        .into())
                )
            )
        } else if aim {
            "+".into()
        } else {
            continue;
        };
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
    listing: Res<listing::Listing>,
) {
    if check.pending {
        return;
    }
    if check.stage >= 9 {
        if explorer.dirty {
            let expected = explorer.original.clone();
            explorer.reload(&repo.0);
            assert_eq!(explorer.source, expected);
        }
        std::fs::write(
            check.directory.join("passed.txt"),
            "Rendered pair, ghost, syntax wall, city overview, district mirror, diff and blame walls. The draft action detached the exact declaration, spawned physical fragments, and restore recovered the immutable source and cleared debris.",
        )
        .expect("smoke report");
        println!(
            "Native smoke passed: pair, ghost, syntax, physical fragment, restore, neon, mirror, diff and blame."
        );
        commands.write_message(AppExit::Success);
        return;
    }
    if !check.prepared {
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
                    let base = comparison_scene::BASE;
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
                _ => {
                    *camera = listing::smoke_side(
                        &explorer,
                        &repo.0.files[explorer.selected],
                        check.stage == 8,
                    );
                }
            }
        }
        explorer.rebuild = true;
        check.elapsed = 0.;
        check.prepared = true;
        return;
    }
    check.elapsed += time.delta_secs();
    if check.elapsed < 4. {
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
