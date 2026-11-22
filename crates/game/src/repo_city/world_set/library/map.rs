use bevy::{
    camera::visibility::RenderLayers,
    input_focus::InputFocus,
    picking::Pickable,
    render::view::Msaa,
    ui::{RelativeCursorPosition, UiTargetCamera},
};

use super::*;

const PAPER: Color = Color::srgb_u8(248, 228, 195);
const GOLD: Color = Color::srgb_u8(222, 175, 115);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Folder(usize),
    Library,
}

#[derive(Clone, Copy, Component)]
enum Choice {
    Close,
}

struct Scene {
    atlas: atlas::Atlas,
    mesh: Entity,
    selection: Entity,
    panel: Entity,
    hover: Entity,
}

#[derive(Component)]
pub(super) struct PathLabel;

#[derive(Component)]
pub(super) struct DistrictLabel;

#[derive(Component)]
pub(super) struct LandingShade;

#[derive(Resource, Default)]
pub(super) struct Map {
    level: Option<Level>,
    cursor: usize,
    scene: Option<Scene>,
    terrain: Option<atlas::Terrain>,
    passage: Option<Passage>,
    framing: Option<Framing>,
    ground: Option<(Transform, Projection, Msaa)>,
    close: bool,
    shift: Vec2,
    zoom: f32,
    pointer: Option<Vec2>,
    district: Option<usize>,
    requested_folder: Option<usize>,
    shade: f32,
    pub(super) consumed: bool,
}

struct Framing {
    start: (Vec2, f32),
    elapsed: f32,
}

impl Framing {
    fn advance(&mut self, seconds: f32) -> ((Vec2, f32), bool) {
        self.elapsed += seconds;
        let (start_shift, start_zoom) = self.start;
        let duration = 1.02 + (start_zoom.ln().abs() * 0.18).min(0.95);
        let t = smooth(self.elapsed / duration);
        let zoom = blend_zoom(start_zoom, 1., t);
        // Keep the originating folder in view until the lens has widened.
        let pan = if (1. - start_zoom).abs() > 0.001 {
            ((zoom - start_zoom) / (1. - start_zoom)).clamp(0., 1.)
        } else {
            t
        };
        (
            (start_shift.lerp(Vec2::ZERO, pan), zoom),
            self.elapsed >= duration,
        )
    }
}

struct Passage {
    file: usize,
    start: (Vec2, f32),
    to: Vec2,
    cruise: f32,
    climb: f32,
    zoom: f32,
    duration: f32,
    elapsed: f32,
}

impl Passage {
    const LANDING: f32 = 0.48;

    fn new(file: usize, start: (Vec2, f32), to: Vec2, height: f32) -> Self {
        let distance = start.0.distance(to) / height.max(1.);
        let cruise = start.1.max((distance * 1.15).min(1.));
        let climb = (0.38 * (cruise / start.1).ln()).min(0.95);
        let zoom = (62. / height.max(1.)).clamp(0.018, 1.).min(start.1);
        let zoom_steps = (cruise / zoom).ln();
        Self {
            file,
            start,
            to,
            cruise,
            climb,
            zoom,
            duration: 2.05 + (2.65 * distance + 0.32 * zoom_steps).min(2.25),
            elapsed: 0.,
        }
    }

    fn advance(&mut self, seconds: f32) -> ((Vec2, f32), f32, Option<usize>) {
        self.elapsed += seconds;
        let flying = self.elapsed - self.climb;
        let t = (flying / self.duration).clamp(0., 1.);
        let shift = self.start.0.lerp(self.to, smooth(t));
        // Widen a tight view before crossing the terrain, then zoom in to land.
        let zoom = if flying < 0. {
            blend_zoom(self.start.1, self.cruise, smooth(self.elapsed / self.climb))
        } else {
            blend_zoom(self.cruise, self.zoom, smooth((t - 0.24) / 0.76))
        };
        let shade = smooth((flying - self.duration) / Self::LANDING);
        (
            (shift, zoom),
            shade,
            (flying >= self.duration + Self::LANDING).then_some(self.file),
        )
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + t * 6.))
}

fn blend_zoom(from: f32, to: f32, t: f32) -> f32 {
    from.ln().lerp(to.ln(), t).exp()
}

impl Map {
    pub(super) fn show_folder(&mut self, folder: usize) {
        self.requested_folder = Some(folder);
    }
    pub(super) fn open(&self) -> bool {
        self.level.is_some()
    }

    pub(super) fn flight_frame(&self, aspect: f32) -> Option<f32> {
        let scene = self.scene.as_ref()?;
        (self.level == Some(Level::Library) || self.passage.is_some())
            .then(|| scene.atlas.view_height(aspect) * self.zoom)
    }
}

pub(super) fn shade(commands: &mut Commands, camera: Entity) {
    commands.spawn((
        LandingShade,
        UiTargetCamera(camera),
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.),
            height: percent(100.),
            ..default()
        },
        BackgroundColor(Color::NONE),
        GlobalZIndex(20),
    ));
}

pub(super) fn draw_shade(
    map: Res<Map>,
    room: Res<room::Unrolling>,
    mut shade: Query<&mut BackgroundColor, With<LandingShade>>,
) {
    if let Ok(mut shade) = shade.single_mut() {
        let opacity = if map.open() {
            map.shade
        } else {
            smooth(map.shade).max(room.shade())
        };
        shade.set_if_neq(BackgroundColor(Color::srgba(0.07, 0.055, 0.043, opacity)));
    }
}

impl native_panel::Actions<Choice> for Map {
    fn activate(&mut self, _: Choice) {
        self.close = true;
    }
}

type Eye<'a> = (
    Entity,
    &'a mut Transform,
    &'a mut Projection,
    &'a Camera,
    &'a mut RenderLayers,
    &'a mut Msaa,
);

pub(super) fn update(
    mut commands: Commands,
    (keys, time): (Res<ButtonInput<KeyCode>>, Res<Time>),
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    aim: Res<aim::Aim>,
    (mut library, mut filter): (ResMut<Library>, ResMut<file_facade::ChangesOnly>),
    mut map: ResMut<Map>,
    (review, menu): (Res<review_notes::Review>, Res<city_menu::Menu>),
    mut eye: Query<Eye<'_>, (With<room::Camera>, Without<Selection>)>,
    (mut focus, parents): (ResMut<InputFocus>, Query<&ChildOf>),
    (buttons, motion, wheel): (
        Res<ButtonInput<MouseButton>>,
        Res<AccumulatedMouseMotion>,
        Res<AccumulatedMouseScroll>,
    ),
    (windows, pointer): (
        Query<&Window, With<PrimaryWindow>>,
        Query<&RelativeCursorPosition>,
    ),
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    (mut labels, mut highlights): (
        Query<&mut Text, (With<PathLabel>, Without<DistrictLabel>)>,
        Query<&mut Transform, With<Selection>>,
    ),
    mut hover_label: Query<(&mut Node, &mut Text), With<DistrictLabel>>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
) {
    map.consumed = map.open();
    let previous = map.level;
    let requested_folder = map.requested_folder.take();
    map.shade = (map.shade - time.delta_secs() / 0.55).max(0.);
    if repo.is_changed() {
        map.terrain = None;
    }
    let Ok((camera_entity, mut pose, mut projection, camera, mut layers, mut msaa)) =
        eye.single_mut()
    else {
        return;
    };
    let aspect = windows
        .single()
        .map_or(1.6, |window| window.width() / window.height().max(1.));
    let unavailable = repo.0.files.is_empty();
    #[cfg(not(target_arch = "wasm32"))]
    let unavailable = unavailable || picker.blocks_controls();
    if unavailable
        || (repo.is_changed() && map.passage.is_some())
        || std::mem::take(&mut map.close)
        || (map.open() && keys.any_just_pressed([KeyCode::Escape, KeyCode::Tab]))
    {
        map.level = None;
    } else if let Some(folder) = requested_folder {
        map.level = Some(Level::Folder(folder));
    } else if library.input.is_none()
        && !review.blocked
        && !review.tablet_open
        && !menu.blocks_controls()
        && keys.just_pressed(KeyCode::KeyM)
    {
        map.level = match previous {
            None => {
                let from = library.walk_pose.as_ref().unwrap_or(&pose);
                let aimed = match aim.0 {
                    Some(aim::Target::Cabinet(folder)) => Some(folder),
                    Some(aim::Target::File(file)) | Some(aim::Target::Line { file, .. }) => repo
                        .0
                        .files
                        .get(file)
                        .and_then(|file| library.catalog.folder(file.directory()))
                        .map(|(folder, _, _)| folder),
                    None => None,
                };
                let index = library
                    .reader
                    .as_ref()
                    .and_then(|_| repo.0.files.get(explorer.selected))
                    .and_then(|file| library.catalog.folder(file.directory()))
                    .map(|(folder, _, _)| folder)
                    .or(aimed)
                    .or_else(|| layout::nearest(from.translation, library.catalog.folders.len()));
                index.map(Level::Folder).or(Some(Level::Library))
            }
            Some(Level::Folder(_)) => Some(Level::Library),
            Some(Level::Library) => None,
        };
    }
    if previous != map.level || (map.open() && repo.is_changed()) {
        let selected = map
            .scene
            .as_ref()
            .and_then(|scene| scene.atlas.file(map.cursor))
            .unwrap_or(explorer.selected);
        let old_scene = map.scene.take();
        if let Some(scene) = &old_scene {
            commands.entity(scene.mesh).despawn();
            commands.entity(scene.selection).despawn();
            commands.entity(scene.hover).despawn();
            native_panel::close(&mut commands, scene.panel, &mut focus, &parents);
        }
        set_district(&mut map, &library.catalog, None);
        map.passage = None;
        map.shade = 0.;
        match map.level {
            None => {
                map.framing = None;
                if let Some((ground, ground_projection, ground_msaa)) = map.ground.take() {
                    *pose = ground;
                    *projection = ground_projection;
                    *msaa = ground_msaa;
                    *layers = RenderLayers::layer(0);
                }
                #[cfg(target_arch = "wasm32")]
                browser_map("", 0);
            }
            Some(level) => {
                let folder = match level {
                    Level::Folder(index) => library.catalog.folders.get(index),
                    Level::Library => None,
                };
                let terrain = map
                    .terrain
                    .get_or_insert_with(|| atlas::Atlas::terrain(&library.catalog));
                if let Some(atlas) = atlas::Atlas::new(
                    &repo.0,
                    &library.catalog,
                    terrain,
                    match level {
                        Level::Folder(index) => Some(index),
                        Level::Library => None,
                    },
                ) {
                    map.ground
                        .get_or_insert_with(|| (*pose, projection.clone(), *msaa));
                    *layers = RenderLayers::layer(atlas::LAYER);
                    *msaa = Msaa::Off;
                    map.cursor = atlas.slot(selected).unwrap_or(0);
                    let start = old_scene
                        .as_ref()
                        .filter(|_| previous != map.level)
                        .map_or((Vec2::ZERO, 1.), |old| {
                            atlas.framing_from(&old.atlas, map.shift, map.zoom, aspect)
                        });
                    map.framing =
                        (start != (Vec2::ZERO, 1.)).then_some(Framing { start, elapsed: 0. });
                    (map.shift, map.zoom) = start;
                    map.pointer = None;
                    (*pose, *projection) = atlas.view(map.shift, map.zoom);
                    let title = folder.map_or_else(
                        || "Archive courtyards".to_owned(),
                        |folder| format!("Folder · {}", folder.path),
                    );
                    #[cfg(target_arch = "wasm32")]
                    browser_map(&title, atlas.len() as u32);
                    map.scene = Some(spawn(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        atlas,
                        camera_entity,
                        &title,
                        level == Level::Library,
                    ));
                } else {
                    map.close = true;
                }
                map.consumed = true;
                return;
            }
        }
    }
    if let Some(((shift, zoom), done)) = map
        .framing
        .as_mut()
        .map(|framing| framing.advance(time.delta_secs()))
    {
        (map.shift, map.zoom) = (shift, zoom);
        if done {
            map.framing = None;
        }
        if let Some(scene) = &map.scene {
            (*pose, *projection) = scene.atlas.view(shift, zoom);
        }
    }
    let passage = map
        .passage
        .as_mut()
        .map(|passage| passage.advance(time.delta_secs()));
    if let Some(((shift, zoom), shade, _)) = passage {
        (map.shift, map.zoom) = (shift, zoom);
        map.shade = shade;
    }
    let Some(scene) = map.scene.as_ref() else {
        return;
    };
    let count = scene.atlas.len();
    let mut selected = None;
    let mut arrived = false;
    if let Some(((shift, zoom), _, destination)) = passage {
        (*pose, *projection) = scene.atlas.view(shift, zoom);
        if let Some(file) = destination {
            selected = Some(file);
            arrived = true;
        } else {
            return;
        }
    }
    let mut cursor = map.cursor;
    let mut zoom = map.zoom;
    let mut shift = map.shift;
    let mut pointer_position = map.pointer;
    let mut district = map.district;
    let mut requested_folder = None;
    if !arrived {
        if let Ok(window) = windows.single() {
            let on_panel = pointer
                .get(scene.panel)
                .is_ok_and(RelativeCursorPosition::cursor_over);
            let position = window.cursor_position().filter(|_| !on_panel);
            if let Some(position) = position {
                let amount = match wheel.unit {
                    bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y,
                    bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y / 40.,
                };
                if amount != 0. {
                    zoom = (zoom * (-amount * 0.16).exp()).clamp(0.025, 1.0);
                }
                if zoom == 1. {
                    shift = Vec2::ZERO;
                } else if buttons.pressed(MouseButton::Right)
                    && let Projection::Orthographic(lens) = &*projection
                {
                    let units = lens.area.size()
                        / Vec2::new(window.width(), window.height()).max(Vec2::ONE);
                    shift += Vec2::new(-motion.delta.x, motion.delta.y) * units;
                }
                if (Some(position) != map.pointer || buttons.just_pressed(MouseButton::Left))
                    && let Ok(ray) =
                        camera.viewport_to_world(&GlobalTransform::from(*pose), position)
                {
                    let hit = scene.atlas.hit(ray);
                    district = scene.atlas.directory(ray, hit);
                    if let Some(slot) = hit {
                        cursor = slot;
                        if buttons.just_pressed(MouseButton::Left) {
                            selected = scene.atlas.file(slot);
                        }
                    } else if map.level == Some(Level::Library)
                        && buttons.just_pressed(MouseButton::Left)
                    {
                        requested_folder = district;
                    }
                }
            } else {
                district = None;
            }
            pointer_position = position;
        }
        if keys.just_pressed(KeyCode::Home) {
            cursor = 0;
        } else if keys.just_pressed(KeyCode::End) {
            cursor = count - 1;
        } else if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::ArrowDown]) {
            cursor = (cursor + 1).min(count - 1);
        } else if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::ArrowUp]) {
            cursor = cursor.saturating_sub(1);
        } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])
            && !focus.get().is_some_and(|entity| {
                parents
                    .iter_ancestors(entity)
                    .any(|parent| parent == scene.panel)
            })
        {
            selected = scene.atlas.file(cursor);
        }
    }
    if let Some(index) = selected {
        let file = &repo.0.files[index];
        let folder = library
            .catalog
            .folder(file.directory())
            .map(|(folder, _, _)| folder);
        if !arrived && let Some(to) = scene.atlas.focus_shift(index) {
            if let Ok(mut label) = labels.single_mut() {
                label.0.clone_from(&file.path);
            }
            if let Ok((mut node, _)) = hover_label.get_mut(scene.hover) {
                node.display = Display::None;
            }
            let height = scene.atlas.view_height(aspect);
            set_district(&mut map, &library.catalog, None);
            #[cfg(target_arch = "wasm32")]
            browser_travel(file.directory());
            map.framing = None;
            map.passage = Some(Passage::new(index, (map.shift, map.zoom), to, height));
            return;
        }
        map.passage = None;
        map.framing = None;
        let ground = folder.map(layout::doorway);
        if let Some(scene) = map.scene.take() {
            commands.entity(scene.mesh).despawn();
            commands.entity(scene.selection).despawn();
            commands.entity(scene.hover).despawn();
            native_panel::close(&mut commands, scene.panel, &mut focus, &parents);
        }
        if let Some((saved, ground_projection, ground_msaa)) = map.ground.take() {
            *pose = ground.map_or(saved, |destination| {
                let mut approach = destination;
                approach.translation += Vec3::Y * 7. - Vec3::from(destination.forward()) * 4.8;
                approach.look_at(
                    destination.translation + Vec3::from(destination.forward()) * 4.,
                    Vec3::Y,
                );
                library.flight = Some(room::Flight::landing(approach, destination));
                approach
            });
            *projection = ground_projection;
            *msaa = ground_msaa;
            *layers = RenderLayers::layer(0);
        }
        map.level = None;
        library.reader = None;
        library.walk_pose = None;
        library.retreat = None;
        if !filter.includes(file) {
            filter.toggle();
        }
        library.catalog.query.clear();
        library.catalog.collect(&repo.0, Some(index), &filter);
        #[cfg(target_arch = "wasm32")]
        browser_map("", 0);
        return;
    }
    let manual_view = zoom != map.zoom || shift != map.shift;
    if manual_view {
        (*pose, *projection) = scene.atlas.view(shift, zoom);
    }
    if let Ok((mut node, mut label)) = hover_label.get_mut(scene.hover) {
        let label_at = district
            .zip(pointer_position)
            .zip(camera.logical_viewport_size());
        if let Some(((folder, point), size)) = label_at {
            let path = &library.catalog.folders[folder].path;
            let width = (path.chars().count() as f32 * 9.4 + 18.).min((size.x - 8.).max(0.));
            let left = (point.x + 14.).min((size.x - width - 4.).max(4.));
            let top = (point.y - 32.).max(4.);
            let layout = (Display::Flex, px(left), px(top), px(width));
            if (node.display, node.left, node.top, node.width) != layout {
                (node.display, node.left, node.top, node.width) = layout;
            }
            if label.0 != *path {
                label.0.clone_from(path);
            }
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
    if let Some(scene) = map.scene.as_ref() {
        if let Some((center, size)) = scene.atlas.lot_bounds(cursor)
            && let Ok(mut highlight) = highlights.get_mut(scene.selection)
        {
            highlight.set_if_neq(selection_pose(center, size));
        }
        if let Some(file) = scene.atlas.file(cursor)
            && let Ok(mut label) = labels.single_mut()
            && label.0 != repo.0.files[file].path
        {
            label.0.clone_from(&repo.0.files[file].path);
        }
    }
    map.pointer = if manual_view || map.framing.is_some() {
        None
    } else {
        pointer_position
    };
    set_district(&mut map, &library.catalog, district);
    map.zoom = zoom;
    map.shift = shift;
    map.cursor = cursor;
    map.requested_folder = requested_folder;
    if manual_view {
        map.framing = None;
    }
}

fn set_district(map: &mut Map, _catalog: &Catalog, district: Option<usize>) {
    if map.district != district {
        map.district = district;
        #[cfg(target_arch = "wasm32")]
        browser_hover(district.map_or("", |index| &_catalog.folders[index].path));
    }
}

#[derive(Component)]
pub(super) struct Selection;

fn selection_pose(center: Vec3, size: Vec3) -> Transform {
    Transform::from_translation(center + Vec3::Y * (size.y / 2. + 0.2)).with_scale(Vec3::new(
        size.x + 1.,
        0.4,
        size.z + 1.,
    ))
}

fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    atlas: atlas::Atlas,
    camera: Entity,
    title: &str,
    overview: bool,
) -> Scene {
    let model = commands
        .spawn((
            Mesh3d(meshes.add(atlas.mesh())),
            MeshMaterial3d(materials.add(StandardMaterial {
                unlit: true,
                fog_enabled: false,
                cull_mode: None,
                ..default()
            })),
            RenderLayers::layer(atlas::LAYER),
        ))
        .id();
    let selection = commands
        .spawn((
            Selection,
            Mesh3d(meshes.add(Cuboid::from_size(Vec3::ONE))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: GOLD,
                unlit: true,
                fog_enabled: false,
                ..default()
            })),
            RenderLayers::layer(atlas::LAYER),
            Transform::from_xyz(0., -1000., 0.),
        ))
        .id();
    let panel = native_panel::bottom_bar(commands, camera);
    let hover = commands
        .spawn((
            DistrictLabel,
            UiTargetCamera(camera),
            Pickable::IGNORE,
            Text::default(),
            native_panel::panel_font(),
            TextColor(PAPER),
            BackgroundColor(Color::srgb_u8(40, 29, 22)),
            BorderColor::all(GOLD),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                height: px(25.),
                padding: UiRect::horizontal(px(7.)),
                border: UiRect::all(px(1.)),
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(10),
        ))
        .id();
    commands
        .entity(panel)
        .insert(RelativeCursorPosition::default())
        .with_children(|body| {
            native_panel::text(body, &format!("{title}   ·   {} files", atlas.len()), GOLD);
            body.spawn((
                PathLabel,
                Text::default(),
                native_panel::panel_font(),
                TextColor(PAPER),
                Node {
                    min_width: px(0.),
                    flex_grow: 1.,
                    overflow: Overflow::clip(),
                    max_height: px(22.),
                    ..default()
                },
            ));
            native_panel::text(
                body,
                if overview {
                    "Scroll lands at library · Terrace opens folder map\nWheel zoom · Right-drag pan"
                } else {
                    "Click or Enter to land at entrance\nWheel zoom · Right-drag pan"
                },
                PAPER,
            );
            native_panel::button::<Choice, Map>(body, "Close", PAPER, Choice::Close);
        });
    Scene {
        atlas,
        mesh: model,
        selection,
        panel,
        hover,
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryMap)]
    fn browser_map(title: &str, files: u32);
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryTravel)]
    fn browser_travel(folder: &str);
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryMapHover)]
    fn browser_hover(folder: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passage_frames_both_folders_before_releasing_the_file() {
        let from = Vec2::new(-20., 10.);
        let to = Vec2::new(45., -5.);
        let mut passage = Passage::new(71, (from, 1.), to, 240.);
        assert_eq!(passage.advance(0.), ((from, 1.), 0., None));
        let ((middle, zoom), shade, destination) = passage.advance(passage.duration * 0.5);
        assert_eq!(middle, from.midpoint(to));
        assert!(zoom < 1. && zoom > passage.zoom);
        assert_eq!((shade, destination), (0., None));
        let ((end, zoom), shade, destination) = passage.advance(passage.duration * 0.5);
        assert_eq!(end, to);
        assert!((zoom - passage.zoom).abs() < 0.001);
        assert_eq!((shade, destination), (0., None));
        let (_, shade, destination) = passage.advance(Passage::LANDING + 0.01);
        assert_eq!((shade, destination), (1., Some(71)));
    }

    #[test]
    fn a_distant_flight_widens_a_zoomed_in_map_before_panning() {
        let mut passage = Passage::new(71, (Vec2::ZERO, 0.10), Vec2::new(170., 0.), 210.);
        let ((shift, zoom), _, destination) = passage.advance(passage.climb * 0.5);
        assert_eq!(shift, Vec2::ZERO);
        assert!(zoom > 0.10 && zoom < passage.cruise);
        assert!(destination.is_none());
    }
}
