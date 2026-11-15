//! Repository explorer and the code city's presentation.
#![allow(dead_code)]
mod android;
mod city_navigation;
mod code_texture;
mod comparison_scene;
mod district;
mod file_picker;
mod listing;
mod physics;
mod repository_picker;
#[path = "../shapes.rs"]
mod shapes;
mod world_set;

use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use repo_world::{Change, FileAnatomy, Repository};
use repo_world::{comparison::Comparison, syntax};
use std::{collections::HashSet, path::PathBuf};
use world_set::{WorldSet, color};

type PlayerCamera = (With<Camera3d>, Without<district::MirrorCamera>);

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
    charge: f32,
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
            notice: "Fly to a file. Hit its facade to unfold it.".into(),
            pulse: 0.,
            detached: None,
            column: 0,
            highlighted: vec![],
            comparison: None,
            comparison_mode: 0,
            hunk: 0,
            pending_fracture: false,
            charge: 0.,
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
    height: f32,
}
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
struct Pilot {
    yaw: f32,
    pitch: f32,
    captured: bool,
}
#[derive(Resource, Default)]
struct Nearest {
    file: Option<usize>,
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
            "repo-city --repo PATH [--rev HEAD] [--base REF] [--trace FILE] [--file PATH] [--world-set FILE] [--smoke-dir PATH]\nWASD fly · Click a file block to fly there · Space/Ctrl altitude · Shift boost · RMB look · Tab capture\nHold F and release to wash a source strip · Home frames the file · G turns to the diff\nC reviews changes · T follows a supplied stack · W/S fly between platforms · Esc returns to the city\nG ghost/pair · [ ] hunks · E source wall · Q before/after · Scroll/PgUp/PgDn read\nF6 exports the draft to this project's sandbox"
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
    .add_plugins(PhysicsPlugins::default())
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
    .init_resource::<file_picker::FilePicker>()
    .init_resource::<listing::Listing>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            file_picker::update,
            flight,
            targeting,
            city_navigation::navigate,
            interaction,
            comparison_scene::controls,
            listing::controls,
            physics::launch,
            physics::trajectory,
            listing::cover_impacts,
            physics::splashes,
            city_navigation::break_platforms,
            physics::impacts,
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
        (repository_picker::open_repository, spawn_city)
            .chain()
            .before(file_picker::update),
    );
    app.add_systems(Startup, district::setup);
    app.add_systems(Update, district::reflect.after(listing::draw_overview));
    #[cfg(target_arch = "wasm32")]
    app.add_systems(
        Update,
        browser_refresh
            .before(file_picker::update)
            .run_if(repository_picker::has_city),
    );

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
            stage: 0,
            pending: false,
            prepared: false,
            expected_draft: None,
            capture_attempts: 0,
        })
        .add_systems(Update, smoke.after(animate));
    }
    app.run();
}

#[cfg(target_arch = "wasm32")]
fn browser_refresh(
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut version: Local<u32>,
    mut listing: ResMut<listing::Listing>,
) {
    let current = repo_world::browser_source_version();
    if current != *version {
        *version = current;
        if explorer.anatomy.is_none() || (explorer.comparison.is_none() && !explorer.dirty) {
            let page = explorer.page;
            explorer.reload(&repo.0);
            listing.invalidate();
            explorer.page = page;
        }
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
) {
    let texture = images.add(code_texture::render(title, &[], ink, 768, 64));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(width, width / 12.))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(position),
        CityObject,
    ));
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
    commands.insert_resource(physics::Blaster::new(&mut meshes, &mut materials));
    commands.insert_resource(Palette {
        shell: shell.clone(),
        facade_closed,
        facade_open,
        edge: edge.clone(),
        removed,
        plate: plate.clone(),
        unit: unit.clone(),
    });
    commands.spawn((
        Camera3d::default(),
        bevy::ui::IsDefaultUiCamera,
        Camera { ..default() },
        Msaa::Sample4,
        Transform::default(),
        Pilot {
            yaw: 0.45,
            pitch: -0.3,
            captured: false,
        },
    ));
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
    #[cfg(not(target_arch = "wasm32"))]
    repository_picker::spawn(&mut commands, &ui_font(13.), color(set.ink));
    commands.spawn((
        Text::new("REPO CITY"),
        ui_font(18.),
        TextColor(color(set.ink)),
        Node {
            position_type: PositionType::Absolute,
            left: px(24.),
            top: px(20.),
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
            top: px(20.),
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
        *pose = listing::view(&repo.0.files[explorer.selected]);
        pilot.captured = false;
    }
    let extent_z = repo
        .0
        .districts
        .iter()
        .map(|d| d.position[1] + d.size[1])
        .fold(200., f32::max);
    commands.spawn((
        CityObject,
        Mesh3d(unit.clone()),
        MeshMaterial3d(plate.clone()),
        Transform::from_xyz(180., -0.7, extent_z / 2.).with_scale(Vec3::new(
            450.,
            1.,
            extent_z + 80.,
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
        Some(bevy::mesh::VertexAttributeValues::Float32x2(stock_uvs)),
    ) = (
        block.attribute(Mesh::ATTRIBUTE_POSITION),
        block.attribute(Mesh::ATTRIBUTE_NORMAL),
        block.attribute(Mesh::ATTRIBUTE_UV_0),
    ) {
        let uvs: Vec<_> = positions
            .iter()
            .zip(normals)
            .zip(stock_uvs)
            .map(|((&[x, y, z], &[nx, ny, nz]), &uv)| {
                if nx != 0. {
                    [0.5 - z * nx, 0.5 - y]
                } else if nz != 0. {
                    [0.5 + x * nz, 0.5 - y]
                } else {
                    debug_assert!(ny != 0.);
                    uv
                }
            })
            .collect();
        block.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    let block = meshes.add(block);
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
    for (i, file) in repo.0.files.iter().enumerate() {
        let [x, z] = file.position;
        let height = listing::building_height(file);
        commands.spawn((
            CityObject,
            Facade { index: i, height },
            Mesh3d(block.clone()),
            MeshMaterial3d(palette.facade_closed.clone()),
            Transform::from_xyz(x, height / 2., z).with_scale(Vec3::new(
                listing::WIDTH,
                height,
                listing::DEPTH,
            )),
            RigidBody::Static,
            Collider::cuboid(1., 1., 1.),
        ));
        let status = match file.change {
            Change::Added => &added,
            Change::Removed => &removed,
            Change::Modified => &modified,
            Change::Unchanged => &unchanged,
        };
        for face in [-1., 1.] {
            commands.spawn((
                CityObject,
                Mesh3d(block.clone()),
                MeshMaterial3d(status.clone()),
                Transform::from_xyz(
                    x - listing::WIDTH / 2. + 0.14,
                    height / 2.,
                    z + face * (listing::DEPTH / 2. + 0.02),
                )
                .with_scale(Vec3::new(0.28, height, 0.04)),
            ));
        }
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        label(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            name,
            Vec3::new(x, height + 1.1, z + listing::DEPTH / 2.),
            12.,
            set.ink,
        );
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
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
    listing: Res<listing::Listing>,
) {
    let Ok((mut camera, mut pilot)) = camera.single_mut() else {
        return;
    };
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    #[cfg(target_arch = "wasm32")]
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        pilot.captured = document.pointer_lock_element().is_some();
        if pilot.captured && picker.is_open() {
            document.exit_pointer_lock();
            pilot.captured = false;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if keys.just_pressed(KeyCode::Tab) && !picker.is_open() {
        pilot.captured = !pilot.captured;
    }
    if keys.just_pressed(KeyCode::Escape) {
        pilot.captured = false;
        explorer.anchored = false;
    }
    if !window.focused {
        pilot.captured = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        cursor.grab_mode = if pilot.captured && window.focused {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
    cursor.visible = !pilot.captured;
    if picker.is_open() || listing.turning() {
        return;
    }
    if !window.focused {
        return;
    }
    if navigation.is_flying() {
        return;
    }
    if pilot.captured || mouse.pressed(MouseButton::Right) {
        let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        pilot.yaw = yaw - motion.delta.x * 0.0025;
        pilot.pitch = (pitch - motion.delta.y * 0.0025).clamp(-1.5, 1.5);
        camera.rotation = Quat::from_euler(EulerRot::YXZ, pilot.yaw, pilot.pitch, 0.);
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
    camera.translation.y = camera.translation.y.max(1.);
}

fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 0.00001 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}
fn targeting(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    camera: Query<(&Camera, &Transform, &Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    facades: Query<&Facade>,
    sections: Query<(&Section, &Transform)>,
    hunks: Query<(&comparison_scene::HunkBlock, &Transform)>,
    mut nearest: ResMut<Nearest>,
) {
    let Ok((projection, camera, pilot)) = camera.single() else {
        return;
    };
    let (origin, direction) = if !pilot.captured
        && !mouse.pressed(MouseButton::Right)
        && let Ok(window) = windows.single()
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
    let mut distance = 250.;
    // While reading a comparison, do not select a city building through its panels.
    for f in facades
        .iter()
        .filter(|_| !(explorer.anchored && explorer.comparison_mode > 0))
    {
        let [x, z] = repo.0.files[f.index].position;
        if let Some(d) = ray_box(
            origin,
            direction,
            Vec3::new(x - listing::WIDTH / 2., 0., z - listing::DEPTH / 2.),
            Vec3::new(x + listing::WIDTH / 2., f.height, z + listing::DEPTH / 2.),
        ) && d < distance
        {
            distance = d;
            nearest.file = Some(f.index);
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
}

fn interaction(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    repo: Res<Repo>,
    nearest: Res<Nearest>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<&mut Transform, PlayerCamera>,
    navigation: Res<city_navigation::Navigation>,
    picker: Res<file_picker::FilePicker>,
) {
    if picker.is_open() {
        return;
    }
    if navigation.guides_camera() {
        return;
    }
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    let hit = mouse.just_pressed(MouseButton::Left);
    let shoot_cut = hit && nearest.function.is_some() && nearest.function == explorer.function;
    if hit {
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
            explorer.page = 0;
            explorer.rebuild = true;
            explorer.notice = "Function selected. X detaches its source; R restores it.".into();
        }
    }
    if keys.just_pressed(KeyCode::KeyE) {
        let enter_wall = explorer.comparison_mode > 0 || !explorer.anchored;
        explorer.comparison_mode = 0;
        explorer.rebuild = true;
        if enter_wall {
            if let Some(index) = nearest.file
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
            "Anchored to facade. WASD crawls; E returns to flight."
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
    if keys.just_pressed(KeyCode::KeyX) || shoot_cut {
        explorer.cut();
        if explorer.pending_fracture {
            let path = repo.0.files[explorer.selected].path.clone();
            explorer.anatomy = Some(repo_world::anatomy::analyze(&path, &explorer.source));
            explorer.function = None;
            explorer.function_page = 0;
            explorer.page = 0;
            explorer.refresh(&repo.0);
        }
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
        if !explorer.reviewed.insert(index) {
            explorer.reviewed.remove(&index);
        }
        explorer.notice = "Review marker changed (this session only).".into();
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
        Transform::from_xyz(x, 6.2, z + listing::FRONT),
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
                RigidBody::Static,
                Collider::cuboid(1., 1., 1.),
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
    mut texts: Query<(
        &mut Text,
        Has<TopHud>,
        Has<BottomHud>,
        Has<StatusHud>,
        Has<AimHud>,
    )>,
) {
    let file = &repo.0.files[explorer.selected];
    for (mut text, top, bottom, status, aim) in &mut texts {
        let value = if top {
            format!(
                "{}  /  {}\n{} files · {} districts · {} → {}",
                set.name,
                repo.0
                    .root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                repo.0.files.len(),
                repo.0.districts.len(),
                repo.0.baseline.as_ref().map_or("empty", |s| &s[..8]),
                &repo.0.revision[..8]
            )
        } else if bottom {
            navigation.prompt(&explorer, &nearest, &repo.0)
        } else if status && listing::Listing::visible(&explorer, &navigation) {
            let (opened, total) = listing.counts(&explorer);
            format!(
                "{}\n{:?} · {}\n{opened} / {total} lines revealed\n{}",
                file.path,
                file.change,
                if explorer.before { "BEFORE" } else { "AFTER" },
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
                "{}\n{:?}  ·  {}{}\n{} functions · {} lines · page {}\n{}\n\n{}\n\n{} / {} files reviewed{}",
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
                format!(
                    "\n{} hunks · {} · launch {}%",
                    explorer.comparison.as_ref().map_or(0, |c| c.hunks.len()),
                    navigation
                        .route_status()
                        .unwrap_or_else(|| match explorer.comparison_mode {
                            1 => "TWO BUILDINGS",
                            2 => "GHOST OVERLAY",
                            _ if explorer.anchored => "SOURCE WALL",
                            _ => "EXPLORING CITY",
                        }
                        .into()),
                    (explorer.charge / 1.5 * 100.) as u32
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
    blaster: Res<physics::Blaster>,
) {
    if check.pending {
        return;
    }
    if check.stage >= 7 {
        if explorer.dirty {
            let expected = explorer.original.clone();
            explorer.reload(&repo.0);
            assert_eq!(explorer.source, expected);
        }
        std::fs::write(
            check.directory.join("passed.txt"),
            "Rendered pair, ghost, syntax wall, city overview and district mirror. A real projectile collision detached the exact declaration, spawned physical fragments, and restore recovered the immutable source and cleared debris.",
        )
        .expect("smoke report");
        println!(
            "Native smoke passed: pair, ghost, syntax, physical fragment, restore, neon and mirror."
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
                    let (section, transform) = sections
                        .iter()
                        .find(|(section, _)| Some(section.index) == explorer.function)
                        .or_else(|| sections.iter().next())
                        .expect("smoke requires an extractable function section");
                    let callable = &explorer.anatomy.as_ref().unwrap().callables[section.index];
                    let mut expected = explorer.source.clone();
                    expected.replace_range(callable.start_byte..callable.end_byte, "");
                    check.expected_draft = Some(expected);
                    let origin = transform.translation + Vec3::new(0., 3., 5.);
                    let travel = 0.4;
                    let velocity =
                        (transform.translation - origin) / travel + Vec3::Y * (9.81 * travel / 2.);
                    physics::spawn_projectile(&mut commands, &blaster, origin, velocity);
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
                _ => {
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
                    *camera =
                        Transform::from_translation(pane.translation + Vec3::new(2., 2., 12.))
                            .looking_at(pane.translation, Vec3::Y);
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
        assert!(explorer.dirty, "projectile did not detach a declaration");
        assert_eq!(
            Some(&explorer.source),
            check.expected_draft.as_ref(),
            "impact detached the wrong declaration"
        );
        assert!(
            !fragments.is_empty(),
            "impact did not spawn physical fragments"
        );
    }
    if check.stage == 4 {
        assert!(!explorer.dirty);
        assert!(debris.is_empty(), "restore left physical debris behind");
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
            if visible_pixels < (rgba.width() as usize * rgba.height() as usize) / 100 {
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
        assert_eq!(
            ray_box(
                Vec3::new(0., 0., 10.),
                Vec3::NEG_Z,
                Vec3::splat(-1.),
                Vec3::splat(1.)
            ),
            Some(9.)
        );
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
