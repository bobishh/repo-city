use bevy::{camera::visibility::RenderLayers, image::ImageSampler, render::view::Msaa};

use super::*;
use layout::{CABINET_X, COURT_LENGTH, DEPTHS, EYE, FRONT};

const PAPER_WIDTH: u32 = 1120;
const SCROLL_WIDTH: f32 = 5.6;
pub(super) const PAPER_WIDTH_WORLD: f32 = SCROLL_WIDTH;
const READ_DISTANCE: f32 = EYE;
const ROOM_FOV: f32 = std::f32::consts::FRAC_PI_4;
const BAY_COUNT: usize = 4;
pub(super) const ROLLS_PER_CABINET: usize = 24;
pub(super) const APPROACHED: f32 = 0.7;
pub(super) const CLIMBED: f32 = 1.02;
pub(super) const UNROLLED: f32 = 2.15;
const STAGED: f32 = 2.40;
const SCROLL_START: f32 = -12.9;
const TABLE_LENGTH: f32 = 5.5;
const SCROLL_Y: f32 = 0.05;
pub(super) const PAPER_Y: f32 = SCROLL_Y + 0.018;
const UNIT_PER_PIXEL: f32 = SCROLL_WIDTH / PAPER_WIDTH as f32;
const ROW_HEIGHT: u32 = 26;
const HEADER_HEIGHT: u32 = 102;
const TRAILER_HEIGHT: u32 = 36;
const TILE_MARGIN: usize = 22;
const TILE_HEIGHT: u32 =
    HEADER_HEIGHT + (SCROLL_ROWS + 2 * TILE_MARGIN) as u32 * ROW_HEIGHT + TRAILER_HEIGHT;
const PAPER: [u8; 3] = [235, 216, 178];
const INK: [f32; 3] = [0.20, 0.14, 0.09];

#[derive(Component)]
pub(super) struct Camera;

#[derive(Component)]
pub(super) enum ScrollPart {
    Body,
    Text,
    Head,
    Tail,
    Step,
}

#[derive(Component)]
pub(super) struct Bay(usize);

#[derive(Component)]
pub(super) struct Cabinet {
    bay: usize,
    slot: usize,
    pub side: f32,
    pub directory: usize,
    pub page: usize,
    pub label: String,
}

impl Cabinet {
    pub(super) fn bounds(&self, pose: &GlobalTransform) -> (Vec3, Vec3) {
        let (scale, _, translation) = pose.to_scale_rotation_translation();
        (
            translation + Vec3::new(self.side * CABINET_X, 2.05 * scale.y, 0.),
            Vec3::new(1.66, 4.12 * scale.y, 3.66),
        )
    }
}

#[derive(Component)]
pub(super) struct CabinetFile(pub usize);

#[derive(Resource)]
pub(super) struct Unrolling {
    pub elapsed: f32,
    pub origin: f32,
    pub x: f32,
    anchor: f32,
    bob: f32,
}

impl Default for Unrolling {
    fn default() -> Self {
        Self {
            elapsed: 0.,
            origin: SCROLL_START,
            x: 0.,
            anchor: SCROLL_START,
            bob: 0.,
        }
    }
}

impl Unrolling {
    pub(super) fn reveal(&self) -> f32 {
        ((self.elapsed - CLIMBED) / (UNROLLED - CLIMBED)).clamp(0., 1.)
    }
    pub(super) fn stationary(&self) -> bool {
        self.elapsed >= STAGED
    }
    fn reading_blend(&self) -> f32 {
        ((self.elapsed - UNROLLED) / (STAGED - UNROLLED)).clamp(0., 1.)
    }
}

#[derive(Component)]
pub(super) struct CatalogPanel;

#[derive(Component)]
pub(super) enum Label {
    Catalog,
    Help,
}

#[derive(Resource)]
pub(super) struct Paper {
    image: Handle<Image>,
    ink: Handle<StandardMaterial>,
}

fn material(materials: &mut Assets<StandardMaterial>, color: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.86,
        ..default()
    })
}

pub(super) fn entrance() -> Transform {
    Transform::from_xyz(0., EYE, layout::ENTRANCE_Z).looking_at(Vec3::new(0., EYE, 7.), Vec3::Y)
}

pub(super) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
    set: Res<WorldSet>,
) {
    load_ui_font(&mut fonts);
    let stone = forms::Stone::new(&mut fonts);
    #[cfg(not(target_arch = "wasm32"))]
    repository_picker::spawn(
        &mut commands,
        &font(13.),
        Color::srgb_u8(246, 222, 176),
        &mut images,
        Some(("Greek Library", stone.heading(34.))),
    );
    commands.insert_resource(stone);
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.93, 0.88, 0.79),
        brightness: 650.,
        ..default()
    });
    let eye = commands
        .spawn((
            Camera3d::default(),
            RenderLayers::layer(0),
            Msaa::Sample4,
            Pilot::default(),
            DistanceFog {
                color: color(set.background),
                falloff: FogFalloff::Linear {
                    start: 42.,
                    end: 78.,
                },
                ..default()
            },
            entrance(),
            Camera,
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1., 0.87, 0.71),
            illuminance: 9200.,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-8., 17., 6.).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let limestone = material(&mut materials, color(set.shell));
    let floor = material(&mut materials, color(set.ground));
    let trim = material(&mut materials, Color::srgb(0.46, 0.32, 0.23));
    let wood = material(&mut materials, Color::srgb(0.30, 0.16, 0.095));
    let dark = material(&mut materials, Color::srgb(0.15, 0.11, 0.085));
    let bronze = material(&mut materials, color(set.accent));
    let parchment = material(&mut materials, Color::srgb(0.86, 0.71, 0.49));
    let leaves = material(&mut materials, Color::srgb(0.31, 0.39, 0.23));
    athens::spawn(&mut commands, &mut meshes, &mut materials);
    spawn_bays(
        &mut commands,
        &mut meshes,
        [
            &limestone, &floor, &trim, &wood, &dark, &bronze, &parchment, &leaves,
        ],
    );

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(34., 5.5, 0.5))),
        MeshMaterial3d(limestone),
        Transform::from_xyz(0., 2.75, -18.),
    ));
    let texture = images.add(paper_canvas(0).image());
    let sheet = meshes.add(Rectangle::new(SCROLL_WIDTH, 1.));
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(PAPER[0], PAPER[1], PAPER[2]),
        unlit: true,
        ..default()
    });
    let text = materials.add(StandardMaterial {
        base_color_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        ..default()
    });
    commands.insert_resource(Paper {
        image: texture,
        ink: text.clone(),
    });
    for (part, surface) in [(ScrollPart::Body, body), (ScrollPart::Text, text)] {
        commands.spawn((
            part,
            Mesh3d(sheet.clone()),
            MeshMaterial3d(surface),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    let rod = meshes.add(Cylinder::new(1., SCROLL_WIDTH + 0.27));
    let seam = meshes.add(Rectangle::new(1.24, 0.09));
    for part in [ScrollPart::Head, ScrollPart::Tail] {
        commands
            .spawn((
                part,
                Mesh3d(rod.clone()),
                MeshMaterial3d(parchment.clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .with_children(|roll| {
                for side in [-1., 1.] {
                    roll.spawn((
                        Mesh3d(seam.clone()),
                        MeshMaterial3d(bronze.clone()),
                        Transform::from_xyz(0., side * (SCROLL_WIDTH + 0.272) / 2., 0.)
                            .with_rotation(Quat::from_rotation_x(
                                -side * std::f32::consts::FRAC_PI_2,
                            )),
                    ));
                }
            });
    }

    librarian::spawn(&mut commands, eye, &mut meshes, &mut materials, &mut images);
    aim::spawn(&mut commands);
    signs::spawn(&mut commands, eye);
    commands.spawn((
        ScrollPart::Step,
        Mesh3d(meshes.add(Cuboid::new(3.1, 0.18, 0.84))),
        MeshMaterial3d(material(&mut materials, color(set.shell))),
        Transform::default(),
        Visibility::Hidden,
    ));

    commands
        .spawn((
            CatalogPanel,
            Node {
                position_type: PositionType::Absolute,
                top: px(24),
                right: px(24),
                width: percent(34),
                max_width: px(435),
                min_width: px(240),
                padding: UiRect::all(px(15)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.11, 0.085, 0.07, 0.93)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Label::Catalog,
                Text::new(""),
                font(15.),
                TextColor(Color::srgb(1., 0.89, 0.70)),
            ));
        });
    commands.spawn((
        Label::Help,
        Text::new(""),
        font(15.),
        TextColor(Color::srgb(1., 0.89, 0.70)),
        BackgroundColor(Color::srgba(0.11, 0.085, 0.07, 0.94)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(16),
            left: px(20),
            max_width: percent(94),
            padding: UiRect::axes(px(13), px(8)),
            ..default()
        },
    ));
    #[cfg(not(target_arch = "wasm32"))]
    commands.spawn((
        repository_picker::RepositoryStatus,
        Text::new(""),
        font(15.),
        TextColor(Color::srgb(1., 0.89, 0.70)),
        BackgroundColor(Color::srgb(0.11, 0.085, 0.07)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            top: px(24),
            left: px(24),
            max_width: percent(48),
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));
}

fn spawn_bays(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    surfaces: [&Handle<StandardMaterial>; 8],
) {
    let [
        limestone,
        floor,
        trim,
        wood,
        dark,
        bronze,
        parchment,
        leaves,
    ] = surfaces;
    let mut blocks = Vec::new();
    let mut cube = |size, surface: &Handle<StandardMaterial>| {
        (meshes.add(Cuboid::from_size(size)), surface.clone())
    };
    let paving = cube(Vec3::new(35., 0.25, COURT_LENGTH), floor);
    let plaza = cube(Vec3::new(12.5, 0.16, 29.), limestone);
    let floor_line = cube(Vec3::new(0.08, 0.02, 28.), bronze);
    let crosswalk = cube(Vec3::new(14.6, 0.028, 2.3), limestone);
    let planter = cube(Vec3::new(2.1, 0.44, 2.9), trim);
    let foundation = cube(Vec3::new(9., 0.39, 14.1), limestone);
    let back_wall = cube(Vec3::new(0.45, 5.3, 28.8), limestone);
    let partition = cube(Vec3::new(8.0, 5.3, 0.45), limestone);
    let front_pier = cube(Vec3::new(0.54, 5.3, 2.75), limestone);
    let lintel = cube(Vec3::new(1.18, 0.54, 13.9), trim);
    let architrave = cube(Vec3::new(1.15, 0.38, 6.0), limestone);
    let capital = cube(Vec3::new(1.22, 0.34, 1.22), limestone);
    let roof = cube(Vec3::new(5.15, 0.25, 14.4), trim);
    let cabinet_back = cube(Vec3::new(0.18, 4.1, 3.45), dark);
    let cabinet_edge = cube(Vec3::new(1.6, 4.1, 0.18), wood);
    let shelf = cube(Vec3::new(1.65, 0.17, 3.65), wood);
    let column = (
        meshes.add(Cylinder::new(0.49, 4.75).mesh().resolution(12)),
        limestone.clone(),
    );
    let court_tree = (
        meshes.add(Sphere::new(0.93).mesh().ico(1).expect("courtyard tree")),
        leaves.clone(),
    );
    let trunk = (meshes.add(Cylinder::new(0.10, 1.57)), wood.clone());
    let scroll = meshes.add(Cylinder::new(0.11, 0.85));
    let mut block = |item: &(Handle<Mesh>, Handle<StandardMaterial>), x, y, z| {
        blocks.push((item.0.clone(), item.1.clone(), Transform::from_xyz(x, y, z)));
    };
    block(&paving, 0., -0.27, 0.);
    block(&plaza, 0., -0.075, 0.);
    for z in [-7.3, 7.3] {
        block(&crosswalk, 0., 0.005, z);
    }
    for side in [-1., 1.] {
        let front = side * FRONT;
        block(&floor_line, side * 5.7, 0.02, 0.);
        block(&back_wall, side * 16., 2.45, 0.);
        block(&planter, side * 5.45, 0.18, 0.);
        block(&trunk, side * 5.45, 1.06, 0.);
        for offset in [-0.62, 0.62] {
            block(&court_tree, side * 5.45, 2.01, offset);
        }
        for z in [-14.4, 0., 14.4] {
            block(&partition, side * 12.1, 2.45, z);
        }
        for hall in [-1., 1.] {
            let z = hall * 7.3;
            block(&foundation, side * 11.65, -0.205, z);
            block(&lintel, front, 5.36, z);
            block(&architrave, front, 4.96, z);
            for edge in [-5.5, 5.5] {
                block(&front_pier, front, 2.52, z + edge);
            }
            for offset in [-2.6, 2.6] {
                block(&column, front, 2.42, z + offset);
                block(&capital, front, 4.83, z + offset);
            }
        }
    }

    for index in 0..BAY_COUNT {
        commands
            .spawn((
                Bay(index),
                Transform::from_xyz(0., 0., index as f32 * COURT_LENGTH),
                Visibility::Visible,
            ))
            .with_children(|bay| {
                for (mesh, material, transform) in &blocks {
                    bay.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                    ));
                }
                for folder in 0..layout::FOLDERS_PER_COURT {
                    let point = layout::folder(folder);
                    for slope in [-1., 1.] {
                        bay.spawn((
                            Mesh3d(roof.0.clone()),
                            MeshMaterial3d(roof.1.clone()),
                            Transform::from_xyz(
                                point.x.signum() * 11.63 + slope * 2.47,
                                5.96,
                                point.y,
                            )
                            .with_rotation(Quat::from_rotation_z(-slope * 0.19)),
                        ));
                    }
                }
                for (position, z) in DEPTHS.into_iter().enumerate() {
                    for (aisle_side, side) in [-1., 1.].into_iter().enumerate() {
                        let slot = position * 2 + aisle_side;
                        bay.spawn((
                            Cabinet {
                                bay: index,
                                slot,
                                side,
                                directory: layout::folder_at(index, slot),
                                page: 0,
                                label: String::new(),
                            },
                            Transform::from_xyz(0., 0., z),
                            Visibility::Hidden,
                        ))
                        .with_children(|cabinet| {
                            let mut piece =
                                |item: &(Handle<Mesh>, Handle<StandardMaterial>), x, y, z| {
                                    cabinet.spawn((
                                        Mesh3d(item.0.clone()),
                                        MeshMaterial3d(item.1.clone()),
                                        Transform::from_xyz(x, y, z),
                                    ));
                                };
                            let x = side * CABINET_X;
                            piece(&cabinet_back, side * (CABINET_X + 0.73), 2.05, 0.);
                            for edge in [-1.72, 1.72] {
                                piece(&cabinet_edge, x, 2.05, edge);
                            }
                            for y in [0.28, 1.30, 2.33, 3.36, 4.12] {
                                piece(&shelf, x, y, 0.);
                            }
                            for (row, y) in [0.48, 1.50, 2.53, 3.56].into_iter().enumerate() {
                                for roll in 0..6 {
                                    let material = if (row + roll) % 4 == 0 {
                                        bronze
                                    } else {
                                        parchment
                                    };
                                    let depth = ((roll + row * 3) % 4) as f32 * 0.05;
                                    cabinet.spawn((
                                        CabinetFile(row * 6 + roll),
                                        Visibility::Hidden,
                                        Mesh3d(scroll.clone()),
                                        MeshMaterial3d(material.clone()),
                                        Transform::from_xyz(
                                            side * (CABINET_X - 0.17 + depth),
                                            y,
                                            -1.26 + roll as f32 * 0.50,
                                        )
                                        .with_rotation(
                                            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                                        ),
                                    ));
                                }
                            }
                        });
                    }
                }
            });
    }
}

fn font(size: f32) -> TextFont {
    let mut font = native_panel::panel_font();
    font.font_size = FontSize::Px(size);
    font
}

pub(super) fn walk(
    camera: &mut Transform,
    keys: &ButtonInput<KeyCode>,
    seconds: f32,
    mouse: Vec2,
    joystick: Vec2,
) {
    let axis = |positive, negative| {
        i8::from(keys.pressed(positive)) as f32 - i8::from(keys.pressed(negative)) as f32
    };
    let delta = mouse;
    let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
    let yaw = yaw + axis(KeyCode::ArrowLeft, KeyCode::ArrowRight) * 1.7 * seconds - delta.x * 0.004;
    let pitch = (pitch - delta.y * 0.004).clamp(-1.15, 1.15);
    camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.);
    let steering = Vec3::new(
        axis(KeyCode::KeyD, KeyCode::KeyA) + joystick.x,
        0.,
        axis(KeyCode::KeyS, KeyCode::KeyW) - joystick.y,
    );
    let position = camera.translation
        + Quat::from_rotation_y(yaw) * steering.clamp_length_max(1.) * (4.4 * seconds);
    let on_x = Vec3::new(position.x, EYE, camera.translation.z);
    if layout::walkable(on_x) {
        camera.translation.x = on_x.x;
    }
    let on_z = Vec3::new(camera.translation.x, EYE, position.z);
    if layout::walkable(on_z) {
        camera.translation.z = on_z.z;
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct Page {
    face: listing::Face,
    first: usize,
    column: usize,
    target: Option<usize>,
    ink: Vec<RowInk>,
}

#[derive(PartialEq, Eq)]
struct RowInk {
    readable: bool,
    guided: bool,
    paint: Option<[u8; 3]>,
}

pub(super) fn draw(
    library: Res<Library>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    (paper, mut images, mut materials): (
        Res<Paper>,
        ResMut<Assets<Image>>,
        ResMut<Assets<StandardMaterial>>,
    ),
    (settings, listing, review, mut tour, aim): (
        Res<reading::Settings>,
        Res<listing::Listing>,
        Res<review_notes::Review>,
        ResMut<guide::Tour>,
        Res<aim::Aim>,
    ),
    (time, map): (Res<Time>, Res<map::Map>),
    mut scroll: Query<
        (&ScrollPart, &mut Transform, &mut Visibility),
        (Without<Bay>, Without<Camera>),
    >,
    mut bays: Query<(&Bay, &mut Transform), (Without<ScrollPart>, Without<Camera>)>,
    mut panels: Query<&mut Node, With<CatalogPanel>>,
    mut labels: Query<(&Label, &mut Text)>,
    mut cameras: Query<
        (&mut Projection, &mut Transform),
        (With<Camera>, Without<Bay>, Without<ScrollPart>),
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut printing: Local<(Option<Page>, blame::Blame)>,
    mut unrolling: ResMut<Unrolling>,
    #[cfg(target_arch = "wasm32")] mut last_prompt: Local<&'static str>,
) {
    if map.open() {
        if let Ok(mut panel) = panels.single_mut() {
            panel.display = Display::None;
        }
        for (label, mut text) in &mut labels {
            if matches!(label, Label::Help) && !text.0.is_empty() {
                text.0.clear();
            }
        }
        return;
    }
    let catalog = &library.catalog;
    let reading = library.reader.is_some();
    #[cfg(target_arch = "wasm32")]
    {
        let prompt = match library.input {
            Some(Input::Catalog) => "Find a file. Press Enter to open it.",
            Some(Input::Line(_)) => "Enter a line number. Press Enter to go.",
            Some(Input::Text(_)) => "Find text in this scroll. Press Enter to search.",
            None => "",
        };
        if *last_prompt != prompt {
            *last_prompt = prompt;
            browser_input(prompt);
        }
    }
    let (printed, blame) = &mut *printing;
    let reader = library.reader.as_ref();
    let face = reader.map_or(listing::Face::Source, |reader| reader.face);
    let content = paper::Rows::new(&explorer, face);
    let blame_changed = if face == listing::Face::Blame {
        blame.update(
            &repo.0,
            &explorer,
            reader.map(|reader| reader.page(content.len()).start),
        )
    } else {
        blame.clear();
        false
    };
    let extent = Extent::new(content.len());
    let tile = reader.map(|reader| extent.tile(reader));
    let dt = time.delta_secs();
    let mut camera_z = SCROLL_START;
    if let Ok((mut projection, mut camera)) = cameras.single_mut() {
        if let (Ok(window), Projection::Perspective(perspective)) =
            (windows.single(), &mut *projection)
        {
            let aspect = window.width().max(1.) / window.height().max(1.);
            let fit_width = 2. * (SCROLL_WIDTH * 0.58 / (READ_DISTANCE * aspect)).atan();
            let reading_fov = ROOM_FOV.max(fit_width).min(2.2);
            let fov = ROOM_FOV + (reading_fov - ROOM_FOV) * unrolling.reading_blend();
            if perspective.fov != fov {
                perspective.fov = fov;
            }
        }
        if let Some(reader) = reader {
            if unrolling.elapsed == 0. {
                let from = library.walk_pose.as_ref().unwrap_or(&*camera);
                let local = ((from.translation.z + COURT_LENGTH / 2.).rem_euclid(COURT_LENGTH)
                    - COURT_LENGTH / 2.)
                    .signum();
                let court_center = ((from.translation.z + COURT_LENGTH / 2.) / COURT_LENGTH)
                    .floor()
                    * COURT_LENGTH;
                unrolling.anchor = court_center + local * 7.5 - 2.75;
                unrolling.x = if from.translation.x > 0. {
                    10.65
                } else {
                    -10.65
                };
                if reader.first > 0 {
                    unrolling.elapsed = STAGED;
                }
            }
            unrolling.elapsed = (unrolling.elapsed + dt).min(STAGED);
            let start = reader.page(extent.rows).start as f32 * line_height();
            unrolling.origin = unrolling.anchor - start;
            let origin = unrolling.origin;
            let close = extent.reading_view(reader, origin, unrolling.x);
            if unrolling.elapsed < STAGED {
                let from = library.walk_pose.as_ref().copied().unwrap_or_else(entrance);
                let approach = smooth(unrolling.elapsed / APPROACHED);
                let step = smooth((unrolling.elapsed - APPROACHED) / (CLIMBED - APPROACHED));
                let opening = extent.deployed(unrolling.reveal()).min(TABLE_LENGTH);
                let watch = Transform::from_xyz(
                    unrolling.x - 0.92,
                    EYE + 0.10 * step,
                    origin + 4.3 + 0.21 * step,
                )
                .looking_at(
                    Vec3::new(unrolling.x, SCROLL_Y, origin + 0.75 + opening.min(1.8)),
                    Vec3::Y,
                );
                let bob = (unrolling.elapsed * 16.).sin() * (1. - approach) * 0.04;
                let read = smooth(unrolling.reading_blend());
                let position = from.translation.lerp(watch.translation, approach) + Vec3::Y * bob;
                camera.translation = position.lerp(close.translation, read);
                camera.rotation = from
                    .rotation
                    .slerp(watch.rotation, approach)
                    .slerp(close.rotation, read);
            } else {
                let delta = close.translation.z - camera.translation.z;
                if delta.abs() > 1.6 {
                    camera.translation = close.translation;
                    unrolling.bob = 0.;
                } else {
                    let distance = delta * (1. - (-20. * dt).exp());
                    unrolling.bob = (unrolling.bob + distance.abs() * 13.) % std::f32::consts::TAU;
                    camera.translation.z += distance;
                    camera.translation.x = close.translation.x;
                    camera.translation.y =
                        close.translation.y + unrolling.bob.sin() * delta.abs().min(0.4) * 0.075;
                }
                camera.rotation = close.rotation;
            }
        } else if unrolling.elapsed != 0. {
            *unrolling = Unrolling::default();
        }
        camera_z = camera.translation.z;
    }
    keep_hall(camera_z, &mut bays);
    let origin = unrolling.origin;
    let first = (unrolling.anchor - origin).max(0.);
    let reveal = unrolling.reveal();
    let length = extent.deployed(reveal);
    let shown = (length - first).clamp(0., TABLE_LENGTH);
    let ink = tile.as_ref().map(|tile| tile.section(first, first + shown));
    if let Some((top, size)) = ink
        && let Some(tile) = &tile
        && let Some(mut material) = materials.get_mut(&paper.ink)
    {
        let units = TILE_HEIGHT as f32 * UNIT_PER_PIXEL;
        let uv = bevy::math::Affine2::from_scale_angle_translation(
            Vec2::new(1., size / units),
            0.,
            Vec2::new(0., (top - tile.top as f32 * UNIT_PER_PIXEL) / units),
        );
        if material.uv_transform != uv {
            material.uv_transform = uv;
        }
    }
    for (part, mut mesh, mut visibility) in &mut scroll {
        let visible = match part {
            ScrollPart::Body if reading && shown > 0. => {
                *mesh = strip(unrolling.x, origin, first, shown, SCROLL_Y);
                true
            }
            ScrollPart::Text if ink.is_some_and(|(_, size)| size > 0.) => {
                let (top, size) = ink.expect("visible ink section");
                *mesh = strip(unrolling.x, origin, top, size, PAPER_Y);
                true
            }
            ScrollPart::Head if reading => {
                let radius = if reveal > 0. { 0.09 } else { extent.radius() };
                *mesh = coil(unrolling.x, unrolling.anchor, radius);
                true
            }
            ScrollPart::Tail if reading && reveal > 0. => {
                let remaining = (1. - (first + shown) / extent.length()).clamp(0., 1.);
                let radius = (0.11_f32.powi(2)
                    + (extent.radius().powi(2) - 0.11_f32.powi(2)) * remaining)
                    .sqrt();
                *mesh = coil(unrolling.x, unrolling.anchor + shown, radius);
                mesh.rotate_local_y(-(first + shown) / extent.radius());
                true
            }
            ScrollPart::Step if reading => {
                *mesh = Transform::from_xyz(unrolling.x, 0.09, unrolling.anchor - 0.74);
                true
            }
            _ => false,
        };
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut panel) = panels.single_mut() {
        let display = if reading || review.tablet_open || map.open() {
            Display::None
        } else {
            Display::Flex
        };
        if panel.display != display {
            panel.display = display;
        }
    }
    let hint = tour.library_hint(
        time.elapsed_secs(),
        aim.0.is_some(),
        reading,
        unrolling.stationary(),
        review.washing,
    );
    for (label, mut text) in &mut labels {
        if (reading || review.tablet_open || map.open()) && matches!(label, Label::Catalog) {
            continue;
        }
        let content = match label {
            Label::Catalog => catalog_text(catalog, &repo.0, matches!(library.input, Some(Input::Catalog))),
            Label::Help if review.tablet_open || map.open() => String::new(),
            Label::Help if !hint.is_empty() && library.input.is_none() => hint.into(),
            Label::Help if reading => match &library.input {
                Some(Input::Line(line)) => format!("Go to line  ›  {line}▏    Enter go · Esc cancel"),
                Some(Input::Text(text)) => format!("Find in scroll  ›  {text}▏    Enter search · Esc cancel"),
                _ => {
                    let timer = listing.reading_seconds()
                        .map_or(String::new(), |(elapsed, total)| format!(" · {:.1}s left", (total - elapsed).max(0.)));
                    let matches = if reader.is_some_and(|reader| !reader.matches.is_empty()) {
                        format!(" · {} matches · Enter next", reader.unwrap().matches.len())
                    } else { String::new() };
                    let controls = format!("Scroll · G diff · B blame · K heat · J go to line · M map · Esc close{timer}{matches}");
                    let here = content.line_range(reader.expect("reading").page(content.len()));
                    match repo.0.files.get(explorer.selected) {
                        Some(file) => format!("{}   ·   {}   ·   line {}\n{controls}", file.path, paper::face_name(face), here.start()),
                        _ => controls,
                    }
                },
            },
            Label::Help if repo.0.files.is_empty() => if cfg!(target_arch = "wasm32") {
                "Open a GitHub repository from the Repo City menu".into()
            } else {
                "Cmd+O open repository".into()
            },
            Label::Help => "WASD walk · ←/→ turn · Right drag look · ↑/↓ select · PgUp/PgDn shelves · / find · Enter read".into(),
        };
        if text.0 != content {
            text.0 = content;
        }
    }

    let page = library.reader.as_ref().map(|reader| Page {
        face: reader.face,
        first: reader.first,
        column: reader.column,
        target: reader.target,
        ink: tile
            .as_ref()
            .into_iter()
            .flat_map(|tile| tile.rows.clone())
            .filter_map(|index| {
                let row = content.row(index)?;
                let guided = paper::guided(reader, row.line.number, time.elapsed_secs_f64());
                let paint = repo.0.files.get(explorer.selected).and_then(|file| {
                    review
                        .notes
                        .iter()
                        .rev()
                        .find(|note| {
                            note.anchor.path == file.path
                                && note.anchor.before == row.before
                                && (note.anchor.start..=note.anchor.end).contains(&row.line.number)
                                && review.shows_paint(note, &repo.0)
                        })
                        .map(|note| note.preset.color)
                });
                Some(RowInk {
                    readable: paper::readable(
                        reader.face,
                        &row,
                        explorer.selected,
                        index,
                        guided,
                        *settings,
                        &listing,
                    ),
                    guided,
                    paint,
                })
            })
            .collect(),
    });
    if *printed == page
        && !blame_changed
        && (!reading || (!explorer.is_changed() && !repo.is_changed()))
    {
        return;
    }
    let mut page_image = match (&library.reader, repo.0.files.get(explorer.selected)) {
        (Some(reader), Some(file)) => {
            #[cfg(target_arch = "wasm32")]
            {
                let rows = content.line_range(reader.page(content.len()));
                browser_reading(
                    &file.path,
                    *rows.start() as u32,
                    *rows.end() as u32,
                    if explorer.anatomy.is_none() {
                        &explorer.notice
                    } else {
                        ""
                    },
                    paper::face_name(reader.face),
                );
            }
            print(
                reader,
                tile.as_ref().expect("reader tile"),
                &file.path,
                &explorer,
                &content,
                blame,
                &page.as_ref().expect("reader page").ink,
            )
        }
        _ => {
            #[cfg(target_arch = "wasm32")]
            browser_reading("", 0, 0, "", "Source");
            paper_canvas(0).image()
        }
    };
    page_image.sampler = ImageSampler::linear();
    if let Some(mut image) = images.get_mut(&paper.image) {
        *image = page_image;
    }
    *printed = page;
}

fn keep_hall(
    camera_z: f32,
    bays: &mut Query<(&Bay, &mut Transform), (Without<ScrollPart>, Without<Camera>)>,
) {
    let first = first_bay(camera_z);
    for (bay, mut location) in bays {
        let z = (first + bay.0) as f32 * COURT_LENGTH;
        if location.translation.z != z {
            location.translation.z = z;
        }
    }
}

fn first_bay(camera_z: f32) -> usize {
    let center = ((camera_z + COURT_LENGTH * 0.5) / COURT_LENGTH)
        .floor()
        .max(0.) as usize;
    center.saturating_sub(1)
}

fn cabinet_height(files: usize) -> f32 {
    (0.67 + 0.79 * (files as f32 + 1.).log2()).min(6.3)
}

pub(super) fn cabinet_marker(index: usize, files: usize) -> Vec3 {
    let point = layout::folder(index);
    Vec3::new(point.x, cabinet_height(files) + 0.6, point.y)
}

pub(super) fn arrange_cabinets(
    library: Res<Library>,
    repo: Res<Repo>,
    filter: Res<file_facade::ChangesOnly>,
    map: Res<map::Map>,
    camera: Query<&Transform, With<Camera>>,
    mut cabinets: Query<
        (&mut Cabinet, &mut Transform, &mut Visibility, &Children),
        Without<Camera>,
    >,
    mut rolls: Query<(&CabinetFile, &mut Visibility), Without<Cabinet>>,
) {
    let Ok(eye) = camera.single() else { return };
    if map.open() {
        return;
    }
    let first = first_bay(eye.translation.z);
    let selected = library.catalog.files.get(library.catalog.selected).copied();
    for (mut cabinet, mut location, mut visibility, children) in &mut cabinets {
        let index = layout::folder_at(first + cabinet.bay, cabinet.slot);
        let new_directory = cabinet.directory != index || repo.is_changed();
        cabinet.directory = index;
        let group = library.catalog.folders.get(index);
        let shown = if let Some(group) = group {
            let batch_size = 2 * ROLLS_PER_CABINET;
            let batch = selected
                .and_then(|selected| group.files.iter().position(|&file| file == selected))
                .map_or(0, |index| index / batch_size * batch_size);
            let page = batch + layout::rack(cabinet.slot) * ROLLS_PER_CABINET;
            let slots = group
                .files
                .len()
                .saturating_sub(page)
                .min(ROLLS_PER_CABINET);
            location.scale.y = cabinet_height(slots) / 4.12;
            if slots > 0 && (new_directory || cabinet.page != page || cabinet.label.is_empty()) {
                let initial = |offset: usize| {
                    let path = &repo.0.files[group.files[page + offset]].path;
                    path.rsplit('/')
                        .next()
                        .and_then(|name| name.chars().next())
                        .unwrap_or('–')
                        .to_ascii_uppercase()
                };
                cabinet.label = format!("{} · {}–{}", group.path, initial(0), initial(slots - 1));
            }
            cabinet.page = page;
            for child in children.iter() {
                if let Ok((slot, mut visibility)) = rolls.get_mut(child) {
                    let visible = if group
                        .files
                        .get(page + slot.0)
                        .is_some_and(|&file| filter.includes(&repo.0.files[file]))
                    {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if *visibility != visible {
                        *visibility = visible;
                    }
                }
            }
            if slots > 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        } else {
            Visibility::Hidden
        };
        if *visibility != shown {
            *visibility = shown;
        }
    }
}

pub(super) fn line_height() -> f32 {
    ROW_HEIGHT as f32 * UNIT_PER_PIXEL
}

pub(super) fn line_center(index: usize, origin: f32) -> f32 {
    origin + HEADER_HEIGHT as f32 * UNIT_PER_PIXEL + (index as f32 + 0.5) * line_height()
}

fn code_x(face: listing::Face) -> u32 {
    if face == listing::Face::Blame {
        370
    } else {
        120
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn visible_columns(
    camera: &bevy::camera::Camera,
    pose: &GlobalTransform,
    face: listing::Face,
    row: usize,
    scroll: &Unrolling,
) -> Option<usize> {
    let start = code_x(face) as f32;
    let x = scroll.x + start * UNIT_PER_PIXEL - SCROLL_WIDTH / 2.;
    let unit = 12.0 * UNIT_PER_PIXEL;
    let z = line_center(row, scroll.origin);
    let viewport = camera.logical_viewport_rect()?;
    let fits = |x| {
        camera
            .world_to_viewport(pose, Vec3::new(x, PAPER_Y, z))
            .ok()
            .is_some_and(|point| viewport.contains(point))
    };
    if !fits(x) {
        return None;
    }
    let capacity = ((PAPER_WIDTH as f32 - start - 14.) / 12.) as usize;
    Some(
        (1..=capacity)
            .take_while(|column| fits(x + *column as f32 * unit))
            .count(),
    )
}

pub(super) fn line_at(z: f32, origin: f32, rows: usize) -> Option<(usize, f32)> {
    let start = origin + HEADER_HEIGHT as f32 * UNIT_PER_PIXEL;
    let offset = (z - start) / line_height();
    if offset >= 0. && offset < rows as f32 {
        let line = offset.floor();
        Some((line as usize + 1, start + (line + 0.5) * line_height()))
    } else {
        None
    }
}

fn strip(x: f32, origin: f32, top: f32, length: f32, y: f32) -> Transform {
    Transform::from_xyz(x, y, origin + top + length / 2.)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::new(1., length.max(0.001), 1.))
}

fn coil(x: f32, z: f32, radius: f32) -> Transform {
    Transform::from_xyz(x, SCROLL_Y + radius, z)
        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::new(radius, 1., radius))
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

struct Extent {
    rows: usize,
}

impl Extent {
    fn new(rows: usize) -> Self {
        Self { rows }
    }

    fn pixels(&self) -> u32 {
        HEADER_HEIGHT + self.rows.max(4) as u32 * ROW_HEIGHT + TRAILER_HEIGHT
    }

    fn length(&self) -> f32 {
        self.pixels() as f32 * UNIT_PER_PIXEL
    }

    fn deployed(&self, phase: f32) -> f32 {
        let total = self.length();
        let near = total.min(8.);
        near * smooth(phase / 0.65) + (total - near) * smooth((phase - 0.65) / 0.35)
    }

    fn radius(&self) -> f32 {
        (0.15 + 0.16 * (1. + self.rows as f32 / 20.).log2()).min(1.12)
    }

    fn reading_view(&self, reader: &Reader, origin: f32, x: f32) -> Transform {
        let first = reader.page(self.rows).start as f32;
        let y = HEADER_HEIGHT as f32 + (first + SCROLL_ROWS as f32 / 2.) * ROW_HEIGHT as f32;
        let center = (y * UNIT_PER_PIXEL)
            .min(self.length() * 0.5 + first * ROW_HEIGHT as f32 * UNIT_PER_PIXEL * 0.5);
        let focus = Vec3::new(x, SCROLL_Y, origin + center);
        let mut camera = Transform::from_xyz(x, SCROLL_Y + READ_DISTANCE, focus.z)
            .looking_at(focus, Vec3::NEG_Z);
        camera.rotate_local(Quat::from_euler(
            EulerRot::YXZ,
            reader.look.x,
            reader.look.y,
            0.,
        ));
        camera
    }

    fn tile(&self, reader: &Reader) -> Tile {
        let band = reader.page(self.rows);
        let start = band.start.saturating_sub(TILE_MARGIN);
        let end = (band.end + TILE_MARGIN).min(self.rows);
        let top = if start == 0 {
            0
        } else {
            HEADER_HEIGHT + start as u32 * ROW_HEIGHT
        };
        let bottom = if end == self.rows {
            self.pixels()
        } else {
            HEADER_HEIGHT + end as u32 * ROW_HEIGHT
        };
        Tile {
            top,
            height: bottom - top,
            rows: start..end,
        }
    }
}

struct Tile {
    top: u32,
    height: u32,
    rows: std::ops::Range<usize>,
}

impl Tile {
    fn section(&self, from: f32, to: f32) -> (f32, f32) {
        let top = self.top as f32 * UNIT_PER_PIXEL;
        let start = top.max(from);
        let end = to.min(top + self.height as f32 * UNIT_PER_PIXEL);
        (start, (end - start).max(0.))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryReading)]
    fn browser_reading(path: &str, first: u32, last: u32, status: &str, face: &str);
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryInput)]
    fn browser_input(prompt: &str);
}

fn catalog_text(catalog: &Catalog, repo: &Repository, editing: bool) -> String {
    let mut menu = String::new();
    if editing || !catalog.query.is_empty() {
        menu.push_str(&format!(
            "\nFind  ›  {}{}",
            catalog.query,
            if editing { "▏" } else { "" }
        ));
    }
    if catalog.files.is_empty() {
        menu.push_str(if repo.files.is_empty() {
            "\n\nNo repository open"
        } else {
            "\n\nNo matching files"
        });
        return menu;
    }
    if let Some(&selected) = catalog.files.get(catalog.selected)
        && let Some((_, directory, files)) = catalog.folder(repo.files[selected].directory())
    {
        menu.push_str(&format!(
            "\n{directory} · {files} {}",
            if files == 1 { "file" } else { "files" }
        ));
    }
    let current = catalog.selected / SHELF_SIZE + 1;
    let total = catalog.files.len().div_ceil(SHELF_SIZE);
    menu.push_str(&format!("\nShelf {current} / {total}\n"));
    for (row, &file) in catalog.shelf().iter().enumerate() {
        let indicator = if row == catalog.selected % SHELF_SIZE {
            "▸ "
        } else {
            "  "
        };
        menu.push('\n');
        menu.push_str(indicator);
        let path = &repo.files[file].path;
        let characters = path.chars().count();
        if characters > 42 {
            menu.push('…');
        }
        menu.extend(path.chars().skip(characters.saturating_sub(42)));
    }
    menu.trim_start().to_owned()
}

fn paper_canvas(height: u32) -> code_texture::Canvas {
    let mut paper = code_texture::Canvas::new(PAPER_WIDTH, TILE_HEIGHT);
    paper.rect(0, 0, PAPER_WIDTH, height, PAPER);
    paper
}

fn print(
    reader: &Reader,
    tile: &Tile,
    path: &str,
    explorer: &Explorer,
    content: &paper::Rows,
    blame: &blame::Blame,
    ink: &[RowInk],
) -> Image {
    use repo_world::comparison::Mark;

    let mut paper = paper_canvas(tile.height);
    let note_column = code_x(reader.face) as f32;
    if tile.top == 0 {
        paper.text(path, 36., 43., 25., |_| INK);
        let snapshot = if explorer.before { "Before" } else { "Current" };
        let description = if reader.face == listing::Face::Blame {
            format!("{snapshot} · {}", blame.caption())
        } else {
            format!(
                "{snapshot} · {} · {} rows",
                paper::face_name(reader.face),
                content.len()
            )
        };
        paper.text(&description, 37., 72., 18., |_| [0.38, 0.27, 0.17]);
        paper.rect(32, 88, PAPER_WIDTH - 64, 2, [180, 143, 97]);
    }
    if content.len() == 0 {
        let message = match reader.face {
            listing::Face::Diff if explorer.comparison.is_some() => {
                "No changes in this comparison."
            }
            _ if explorer.anatomy.is_none() => explorer.notice.as_str(),
            listing::Face::Diff => "This comparison is unavailable.",
            _ => "Empty file",
        };
        paper.text(message, 38., 134., 22., |_| INK);
    }
    for (row, style) in tile.rows.clone().zip(ink) {
        let Some(entry) = content.row(row) else {
            continue;
        };
        let line = entry.line;
        let top = HEADER_HEIGHT + ROW_HEIGHT * row as u32 - tile.top;
        let tint = match entry.mark {
            Mark::Added => Some([210, 226, 167]),
            Mark::Removed => Some([239, 182, 146]),
            Mark::Context
                if (style.guided || Some(line.number) == reader.target)
                    && entry.before == explorer.before =>
            {
                Some([241, 194, 120])
            }
            Mark::Context => None,
        };
        if let Some(tint) = tint {
            paper.rect(30, top, PAPER_WIDTH - 60, ROW_HEIGHT, tint);
        }
        if let Some(paint) = style.paint {
            paper.rect(27, top + 3, 5, ROW_HEIGHT - 6, paint);
        }
        let sign = match entry.mark {
            Mark::Added => '+',
            Mark::Removed => '−',
            Mark::Context => ' ',
        };
        paper.text(
            &format!("{sign}{:>5}", line.number),
            34.,
            (top + 20) as f32,
            19.,
            |_| [0.44, 0.33, 0.24],
        );
        if reader.face == listing::Face::Blame
            && let Some(note) = blame
                .notes()
                .and_then(|notes| notes.get(line.number.saturating_sub(1)))
        {
            let name: String = note.chars().take(23).collect();
            paper.text(&name, 125., (top + 20) as f32, 17., |_| [0.37, 0.30, 0.21]);
        }
        let offset = line
            .text
            .char_indices()
            .nth(reader.column)
            .map_or(line.text.len(), |(byte, _)| byte);
        paper.text(
            &line.text[offset..],
            note_column,
            (top + 20) as f32,
            20.,
            |byte| {
                let kind = line
                    .spans
                    .iter()
                    .find(|span| span.bytes.contains(&(offset + byte)))
                    .map(|span| span.kind);
                match kind.unwrap_or(syntax::Kind::Plain) {
                    syntax::Kind::Plain => INK,
                    syntax::Kind::Keyword => [0.44, 0.18, 0.40],
                    syntax::Kind::String => [0.22, 0.36, 0.17],
                    syntax::Kind::Comment => [0.42, 0.41, 0.30],
                    syntax::Kind::Number => [0.58, 0.27, 0.12],
                    syntax::Kind::Type => [0.36, 0.31, 0.11],
                    syntax::Kind::Function => [0.20, 0.29, 0.43],
                }
            },
        );
        if !style.readable {
            paper.blur_ink(note_column as u32, top, ROW_HEIGHT);
        }
    }
    paper.image()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabinet_height_responds_to_file_count_and_fits_between_column_and_cornice() {
        assert!(cabinet_height(50) > 2. * cabinet_height(1));
        assert_eq!(cabinet_height(10_000), 6.3);
        assert_eq!(first_bay(0.), 0);
        assert!(first_bay(240.) >= 5);
    }

    #[test]
    fn a_long_file_unrolls_beyond_the_room_without_needing_a_full_height_texture() {
        let short = Extent::new(20);
        let long = Extent::new(100_000);
        assert!(long.length() > 500. * short.length());
        assert!(long.radius() > 3. * short.radius());
        let tile = long.tile(&Reader::new(58_007));
        assert!(tile.rows.contains(&58_006));
        assert!(tile.height < 2000);
        assert!(tile.top > 1_000_000);
    }

    #[test]
    fn reading_late_rows_keeps_the_camera_square_to_the_same_patch_of_floor() {
        let extent = Extent::new(100_000);
        let first = Reader::new(1);
        let late = Reader::new(58_007);
        let anchor = 43.;
        let offset = late.first as f32 * line_height();
        let early_view = extent.reading_view(&first, anchor, 9.92);
        let late_view = extent.reading_view(&late, anchor - offset, 9.92);
        assert!(early_view.translation.distance(late_view.translation) < 0.002);
        assert!(late_view.forward().dot(Vec3::NEG_Y) > 0.999);
        let tile = extent.tile(&late);
        let (top, visible) = tile.section(offset, offset + TABLE_LENGTH);
        assert_eq!(top, offset);
        assert!(visible > SCROLL_ROWS as f32 * line_height());
    }

    #[test]
    fn a_long_scroll_opens_near_the_reader_before_its_tail_leaves_the_aisle() {
        let extent = Extent::new(2400);
        let state = Unrolling {
            elapsed: CLIMBED + (UNROLLED - CLIMBED) * 0.15,
            ..default()
        };
        let opened = extent.deployed(state.reveal());
        assert!(
            (0.2..4.).contains(&opened),
            "opening edge jumped {opened} metres"
        );
        let tile = extent.tile(&Reader::new(1));
        let (top, size) = tile.section(0., opened);
        assert_eq!((top, size), (0., opened));
        assert_eq!(extent.deployed(1.), extent.length());
    }
}
