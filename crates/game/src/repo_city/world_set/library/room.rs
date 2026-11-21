use bevy::image::ImageSampler;

use super::*;

const PAPER_WIDTH: u32 = 1120;
const SCROLL_WIDTH: f32 = 5.6;
const READ_DISTANCE: f32 = 5.68;
const ROOM_FOV: f32 = std::f32::consts::FRAC_PI_4;
const BAY_LENGTH: f32 = 24.;
const BAY_COUNT: usize = 4;
const CABINETS_PER_BAY: usize = 8;
pub(super) const APPROACHED: f32 = 0.7;
pub(super) const CLIMBED: f32 = 1.02;
pub(super) const UNROLLED: f32 = 1.78;
const STAGED: f32 = 2.08;
const SCROLL_START: f32 = -4.7;
const SCROLL_Y: f32 = 0.05;
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
}

#[derive(Resource)]
pub(super) struct Unrolling {
    pub elapsed: f32,
    pub origin: f32,
}

impl Default for Unrolling {
    fn default() -> Self {
        Self {
            elapsed: 0.,
            origin: SCROLL_START,
        }
    }
}

impl Unrolling {
    pub(super) fn reveal(&self) -> f32 {
        ((self.elapsed - CLIMBED) / (UNROLLED - CLIMBED)).clamp(0., 1.)
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
pub(super) struct Paper(Handle<Image>);

fn material(materials: &mut Assets<StandardMaterial>, color: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.86,
        ..default()
    })
}

pub(super) fn entrance() -> Transform {
    Transform::from_xyz(0., 2.15, -10.7).looking_at(Vec3::new(0., 1.9, 8.0), Vec3::Y)
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
    #[cfg(not(target_arch = "wasm32"))]
    commands.spawn((
        Text::new(APP_NAME),
        TextFont {
            font: fonts
                .add(Font::from_bytes(
                    include_bytes!("../../../../../../assets/fonts/GFSDidot-Regular.ttf").to_vec(),
                ))
                .into(),
            ..TextFont::from_font_size(34.)
        },
        TextColor(Color::srgb_u8(246, 222, 176)),
        Node {
            position_type: PositionType::Absolute,
            top: px(20),
            left: px(24),
            ..default()
        },
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.93, 0.88, 0.79),
        brightness: 650.,
        ..default()
    });
    commands.spawn((
        Camera3d::default(),
        DistanceFog {
            color: color(set.background),
            falloff: FogFalloff::Linear {
                start: 30.,
                end: 62.,
            },
            ..default()
        },
        entrance(),
        Camera,
    ));
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
    spawn_bays(
        &mut commands,
        &mut meshes,
        [&limestone, &floor, &trim, &wood, &dark, &bronze, &parchment],
    );

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(18., 7.6, 0.5))),
        MeshMaterial3d(limestone),
        Transform::from_xyz(0., 3.8, -11.7),
    ));
    let texture = images.add(paper_canvas(0).image());
    commands.insert_resource(Paper(texture.clone()));
    let sheet = meshes.add(Rectangle::new(SCROLL_WIDTH, 1.));
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(PAPER[0], PAPER[1], PAPER[2]),
        unlit: true,
        ..default()
    });
    let text = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        ..default()
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
    for part in [ScrollPart::Head, ScrollPart::Tail] {
        commands.spawn((
            part,
            Mesh3d(rod.clone()),
            MeshMaterial3d(bronze.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    librarian::spawn(&mut commands, &mut meshes, &mut materials);
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
    surfaces: [&Handle<StandardMaterial>; 7],
) {
    let [limestone, floor, trim, wood, dark, bronze, parchment] = surfaces;
    let mut blocks = Vec::new();
    let mut cube = |size, surface: &Handle<StandardMaterial>| {
        (meshes.add(Cuboid::from_size(size)), surface.clone())
    };
    let paving = cube(Vec3::new(18., 0.25, BAY_LENGTH), floor);
    let side_wall = cube(Vec3::new(0.55, 7.6, BAY_LENGTH), limestone);
    let floor_line = cube(Vec3::new(0.12, 0.02, BAY_LENGTH), bronze);
    let cornice = cube(Vec3::new(0.73, 0.42, BAY_LENGTH), trim);
    let cabinet_back = cube(Vec3::new(0.18, 4.1, 3.45), dark);
    let cabinet_edge = cube(Vec3::new(1.6, 4.1, 0.18), wood);
    let shelf = cube(Vec3::new(1.65, 0.17, 3.65), wood);
    let capital = cube(Vec3::new(1.35, 0.32, 1.35), limestone);
    let column = (meshes.add(Cylinder::new(0.45, 6.25)), limestone.clone());
    let scroll = meshes.add(Cylinder::new(0.11, 0.85));
    let mut block = |item: &(Handle<Mesh>, Handle<StandardMaterial>), x, y, z| {
        blocks.push((item.0.clone(), item.1.clone(), Transform::from_xyz(x, y, z)));
    };
    block(&paving, 0., -0.13, 0.);
    for side in [-1., 1.] {
        block(&side_wall, side * 8.8, 3.8, 0.);
        block(&floor_line, side * 2.98, 0.01, 0.);
        block(&cornice, side * 8.45, 6.9, 0.);
        for z in [-6., 0., 6., 11.] {
            for y in [0.16, 6.48] {
                block(&capital, side * 5.8, y, z - 1.5);
            }
            block(&column, side * 5.8, 3.28, z - 1.5);
        }
    }

    for index in 0..BAY_COUNT {
        commands
            .spawn((
                Bay(index),
                Transform::from_xyz(0., 0., index as f32 * BAY_LENGTH),
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
                for (position, z) in [-6., 0., 6., 11.].into_iter().enumerate() {
                    for (aisle_side, side) in [-1., 1.].into_iter().enumerate() {
                        let slot = position * 2 + aisle_side;
                        bay.spawn((
                            Cabinet { bay: index, slot },
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
                            let x = side * 7.35;
                            piece(&cabinet_back, side * 8.08, 2.05, 0.);
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
                                        Mesh3d(scroll.clone()),
                                        MeshMaterial3d(material.clone()),
                                        Transform::from_xyz(
                                            side * (7.18 + depth),
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
    mouse: Option<Vec2>,
) {
    let axis = |positive, negative| {
        i8::from(keys.pressed(positive)) as f32 - i8::from(keys.pressed(negative)) as f32
    };
    let delta = mouse.unwrap_or_default();
    let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
    let yaw = yaw + axis(KeyCode::ArrowLeft, KeyCode::ArrowRight) * 1.7 * seconds - delta.x * 0.004;
    let pitch = (pitch - delta.y * 0.004).clamp(-1.15, 1.15);
    camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.);
    let steering = Vec3::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        0.,
        axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    let position = camera.translation
        + Quat::from_rotation_y(yaw) * steering.normalize_or_zero() * (4.4 * seconds);
    camera.translation = Vec3::new(position.x.clamp(-5.05, 5.05), 2.15, position.z.max(-10.9));
}

#[derive(PartialEq, Eq)]
pub(super) struct Page {
    first: usize,
    column: usize,
    target: Option<usize>,
}

pub(super) fn draw(
    library: Res<Library>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    set: Res<WorldSet>,
    paper: Res<Paper>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
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
    mut printed: Local<Option<Page>>,
    mut unrolling: ResMut<Unrolling>,
    #[cfg(target_arch = "wasm32")] mut asking_for_line: Local<bool>,
) {
    let catalog = &library.catalog;
    let reading = library.reader.is_some();
    #[cfg(target_arch = "wasm32")]
    {
        let prompt = matches!(library.input, Some(Input::Line(_)));
        if *asking_for_line != prompt {
            *asking_for_line = prompt;
            browser_line_prompt(prompt);
        }
    }
    let lines = &explorer.highlighted;
    let reader = library.reader.as_ref();
    let extent = Extent::new(lines.len());
    let tile = reader.map(|reader| extent.tile(reader));
    let dt = time.delta_secs();
    let mut camera_z = SCROLL_START;
    if let Ok((mut projection, mut camera)) = cameras.single_mut() {
        if let (Ok(window), Projection::Perspective(perspective)) =
            (windows.single(), &mut *projection)
        {
            let aspect = window.width().max(1.) / window.height().max(1.);
            let fit_width = 2. * (SCROLL_WIDTH * 0.58 / (READ_DISTANCE * aspect)).atan();
            let fov = if reading {
                ROOM_FOV.max(fit_width)
            } else {
                ROOM_FOV
            };
            if perspective.fov != fov {
                perspective.fov = fov;
            }
        }
        if let Some(reader) = reader {
            if unrolling.elapsed == 0. {
                let from = library.walk_pose.as_ref().unwrap_or(&*camera);
                unrolling.origin = (from.translation.z + 4.7).max(SCROLL_START);
            }
            unrolling.elapsed = if reader.first == 0 {
                (unrolling.elapsed + dt).min(STAGED)
            } else {
                STAGED
            };
            let origin = unrolling.origin;
            let close = extent.reading_view(reader, origin);
            if unrolling.elapsed < STAGED {
                let from = library.walk_pose.as_ref().copied().unwrap_or_else(entrance);
                let watch = Transform::from_xyz(-3.1, 3.6, origin - 6.6)
                    .looking_at(Vec3::new(-0.2, 0.6, origin + 2.0), Vec3::Y);
                let approach = (unrolling.elapsed / APPROACHED).clamp(0., 1.);
                let read = ((unrolling.elapsed - UNROLLED) / (STAGED - UNROLLED)).clamp(0., 1.);
                camera.translation = from
                    .translation
                    .lerp(watch.translation, approach)
                    .lerp(close.translation, read);
                camera.rotation = from
                    .rotation
                    .slerp(watch.rotation, approach)
                    .slerp(close.rotation, read);
            } else {
                *camera = close;
            }
        } else if unrolling.elapsed != 0. {
            *unrolling = Unrolling::default();
        }
        camera_z = camera.translation.z;
    }
    keep_hall(camera_z, &mut bays);
    let origin = if reading {
        unrolling.origin
    } else {
        SCROLL_START
    };
    let reveal = unrolling.reveal();
    let length = extent.length() * reveal;
    for (part, mut mesh, mut visibility) in &mut scroll {
        let visible = match part {
            ScrollPart::Body if reading => {
                *mesh = strip(origin, 0., length, SCROLL_Y);
                true
            }
            ScrollPart::Text if reading && reveal == 1. => {
                let tile = tile.as_ref().expect("reader tile");
                *mesh = strip(
                    origin,
                    tile.top as f32 * UNIT_PER_PIXEL,
                    TILE_HEIGHT as f32 * UNIT_PER_PIXEL,
                    SCROLL_Y + 0.018,
                );
                true
            }
            ScrollPart::Head => {
                let radius = if reading && unrolling.elapsed >= CLIMBED {
                    0.12
                } else {
                    extent.radius()
                };
                *mesh = coil(origin, radius);
                true
            }
            ScrollPart::Tail if reading && reveal > 0. => {
                let radius = 0.12 + (extent.radius() - 0.12) * (1. - reveal);
                *mesh = coil(origin + length, radius);
                true
            }
            ScrollPart::Step => {
                *mesh = Transform::from_xyz(0., 0.09, origin - 0.74);
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
        let display = if reading {
            Display::None
        } else {
            Display::Flex
        };
        if panel.display != display {
            panel.display = display;
        }
    }
    for (label, mut text) in &mut labels {
        if reading && matches!(label, Label::Catalog) {
            continue;
        }
        let content = match label {
            Label::Catalog => catalog_text(&set.name, catalog, &repo.0, matches!(library.input, Some(Input::Catalog))),
            Label::Help if reading => match &library.input {
                Some(Input::Line(line)) => format!("Go to line  ›  {line}▏    Enter go · Esc cancel"),
                _ => {
                    let controls = "Scroll or ↑/↓ · PgUp/PgDn · ←/→ pan · J go to line · Esc close";
                    let here = reader.expect("reading").page(lines.len());
                    match repo.0.files.get(explorer.selected) {
                        Some(file) if !here.is_empty() => format!("{}   ·   {} / {} lines\n{controls}", file.path, here.start + 1, lines.len()),
                        _ => controls.into(),
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
        first: reader.first,
        column: reader.column,
        target: reader.target,
    });
    if *printed == page && (!reading || (!explorer.is_changed() && !repo.is_changed())) {
        return;
    }
    let mut page_image = match (&library.reader, repo.0.files.get(explorer.selected)) {
        (Some(reader), Some(file)) => {
            #[cfg(target_arch = "wasm32")]
            {
                let rows = reader.page(explorer.highlighted.len());
                browser_reading(
                    &file.path,
                    if rows.is_empty() {
                        0
                    } else {
                        rows.start as u32 + 1
                    },
                    rows.end as u32,
                    if explorer.anatomy.is_none() {
                        &explorer.notice
                    } else {
                        ""
                    },
                );
            }
            print(
                reader,
                tile.as_ref().expect("reader tile"),
                &file.path,
                &explorer,
            )
        }
        _ => {
            #[cfg(target_arch = "wasm32")]
            browser_reading("", 0, 0, "");
            paper_canvas(0).image()
        }
    };
    page_image.sampler = ImageSampler::linear();
    if let Some(mut image) = images.get_mut(&paper.0) {
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
        let z = (first + bay.0) as f32 * BAY_LENGTH;
        if location.translation.z != z {
            location.translation.z = z;
        }
    }
}

fn first_bay(camera_z: f32) -> usize {
    let center = ((camera_z + BAY_LENGTH * 0.5) / BAY_LENGTH).floor().max(0.) as usize;
    center.saturating_sub(1)
}

fn cabinet_height(files: usize) -> f32 {
    (0.67 + 0.79 * (files as f32 + 1.).log2()).min(6.3)
}

pub(super) fn arrange_cabinets(
    library: Res<Library>,
    camera: Query<&Transform, With<Camera>>,
    mut cabinets: Query<(&Cabinet, &mut Transform, &mut Visibility), Without<Camera>>,
) {
    let Ok(eye) = camera.single() else { return };
    let first = first_bay(eye.translation.z);
    for (cabinet, mut location, mut visibility) in &mut cabinets {
        let index = (first + cabinet.bay) * CABINETS_PER_BAY + cabinet.slot;
        let group = library.catalog.cabinets.get(index);
        let shown = if let Some((_, count)) = group {
            let scale = cabinet_height(*count) / 4.12;
            if location.scale.y != scale {
                location.scale.y = scale;
            }
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != shown {
            *visibility = shown;
        }
    }
}

fn strip(origin: f32, top: f32, length: f32, y: f32) -> Transform {
    Transform::from_xyz(0., y, origin + top + length / 2.)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::new(1., length.max(0.001), 1.))
}

fn coil(z: f32, radius: f32) -> Transform {
    Transform::from_xyz(0., SCROLL_Y + radius, z)
        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::new(radius, 1., radius))
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

    fn radius(&self) -> f32 {
        (0.15 + 0.16 * (1. + self.rows as f32 / 20.).log2()).min(1.12)
    }

    fn reading_view(&self, reader: &Reader, origin: f32) -> Transform {
        let first = reader.page(self.rows).start as f32;
        let y = HEADER_HEIGHT as f32 + (first + SCROLL_ROWS as f32 / 2.) * ROW_HEIGHT as f32;
        let center = (y * UNIT_PER_PIXEL)
            .min(self.length() * 0.5 + first * ROW_HEIGHT as f32 * UNIT_PER_PIXEL * 0.5);
        Transform::from_xyz(0., SCROLL_Y + READ_DISTANCE, origin + center)
            .looking_to(Vec3::NEG_Y, Vec3::NEG_Z)
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

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryReading)]
    fn browser_reading(path: &str, first: u32, last: u32, status: &str);
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryLinePrompt)]
    fn browser_line_prompt(open: bool);
}

fn catalog_text(name: &str, catalog: &Catalog, repo: &Repository, editing: bool) -> String {
    let mut menu = name.to_owned();
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
        && let Some((_, directory, files)) = catalog.cabinet(repo.files[selected].directory())
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
    menu
}

fn paper_canvas(height: u32) -> code_texture::Canvas {
    let mut paper = code_texture::Canvas::new(PAPER_WIDTH, TILE_HEIGHT);
    paper.rect(0, 0, PAPER_WIDTH, height, PAPER);
    paper
}

fn print(reader: &Reader, tile: &Tile, path: &str, explorer: &Explorer) -> Image {
    let mut paper = paper_canvas(tile.height);
    let rows = &explorer.highlighted;
    if tile.top == 0 {
        paper.text(path, 36., 43., 25., |_| INK);
        let snapshot = if explorer.before { "Before" } else { "Source" };
        paper.text(
            &format!("{snapshot}   ·   {} lines", rows.len()),
            37.,
            72.,
            18.,
            |_| [0.38, 0.27, 0.17],
        );
        paper.rect(32, 88, PAPER_WIDTH - 64, 2, [180, 143, 97]);
    }
    if rows.is_empty() {
        let message = if explorer.anatomy.is_some() {
            "Empty file"
        } else {
            explorer.notice.as_str()
        };
        paper.text(message, 38., 134., 22., |_| INK);
    }
    for (row, line) in rows[tile.rows.clone()].iter().enumerate() {
        let top = HEADER_HEIGHT + ROW_HEIGHT * (row + tile.rows.start) as u32 - tile.top;
        if Some(line.number) == reader.target {
            paper.rect(30, top, PAPER_WIDTH - 60, 26, [241, 194, 120]);
        }
        paper.text(
            &format!("{:>6}", line.number),
            34.,
            (top + 20) as f32,
            19.,
            |_| [0.44, 0.33, 0.24],
        );
        let offset = line
            .text
            .char_indices()
            .nth(reader.column)
            .map_or(line.text.len(), |(byte, _)| byte);
        paper.text(&line.text[offset..], 120., (top + 20) as f32, 20., |byte| {
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
                syntax::Kind::Type => [0.10, 0.38, 0.37],
                syntax::Kind::Function => [0.17, 0.33, 0.53],
            }
        });
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
        assert!(first_bay(160.) >= 5);
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
}
