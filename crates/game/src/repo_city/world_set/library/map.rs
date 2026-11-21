use bevy::{
    camera::visibility::RenderLayers, input_focus::InputFocus, render::view::Msaa,
    ui::RelativeCursorPosition,
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
}

#[derive(Component)]
pub(super) struct PathLabel;

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
    from: Vec2,
    to: Vec2,
    elapsed: f32,
}

impl Passage {
    const FLIGHT: f32 = 2.05;
    const SETTLE: f32 = 0.20;

    fn advance(&mut self, seconds: f32) -> ((Vec2, f32), Option<usize>) {
        self.elapsed += seconds;
        let t = smooth(self.elapsed / Self::FLIGHT);
        let approach = self.start.0.lerp(self.from, t);
        let crossing = self.from.lerp(self.to, t);
        let shift = approach.lerp(crossing, t);
        let widen = blend_zoom(self.start.1, 0.55, t);
        let descend = blend_zoom(0.55, 0.20, t);
        (
            (shift, blend_zoom(widen, descend, t)),
            (self.elapsed >= Self::FLIGHT + Self::SETTLE).then_some(self.file),
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
    pub(super) fn open(&self) -> bool {
        self.level.is_some()
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
    mut library: ResMut<Library>,
    mut map: ResMut<Map>,
    review: Res<review_notes::Review>,
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
        Query<&mut Text, With<PathLabel>>,
        Query<&mut Transform, With<Selection>>,
    ),
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
) {
    map.consumed = map.open();
    let previous = map.level;
    if repo.is_changed() {
        map.terrain = None;
    }
    let Ok((camera_entity, mut pose, mut projection, camera, mut layers, mut msaa)) =
        eye.single_mut()
    else {
        return;
    };
    let unavailable = repo.0.files.is_empty();
    #[cfg(not(target_arch = "wasm32"))]
    let unavailable = unavailable || picker.blocks_controls();
    if unavailable
        || (repo.is_changed() && map.passage.is_some())
        || std::mem::take(&mut map.close)
        || (map.open() && keys.any_just_pressed([KeyCode::Escape, KeyCode::Tab]))
    {
        map.level = None;
    } else if library.input.is_none()
        && !review.blocked
        && !review.tablet_open
        && keys.just_pressed(KeyCode::KeyM)
    {
        map.level = match previous {
            None => {
                let from = library.walk_pose.as_ref().unwrap_or(&pose);
                let index = match aim.0 {
                    Some(aim::Target::Cabinet(folder)) => Some(folder),
                    Some(aim::Target::File(file)) | Some(aim::Target::Line { file, .. }) => repo
                        .0
                        .files
                        .get(file)
                        .and_then(|file| library.catalog.folder(file.directory()))
                        .map(|(folder, _, _)| folder),
                    None => layout::nearest(from.translation, library.catalog.folders.len()),
                };
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
            native_panel::close(&mut commands, scene.panel, &mut focus, &parents);
        }
        match map.level {
            None => {
                map.passage = None;
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
                    let start =
                        if matches!((previous, level), (Some(Level::Folder(_)), Level::Library))
                            && let Some(old) = &old_scene
                        {
                            let aspect = windows
                                .single()
                                .map_or(1.6, |window| window.width() / window.height().max(1.));
                            atlas.framing_from(&old.atlas, map.shift, map.zoom, aspect)
                        } else {
                            (Vec2::ZERO, 1.)
                        };
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
    let Some(scene) = map.scene.as_ref() else {
        return;
    };
    let count = scene.atlas.len();
    let mut selected = None;
    let mut arrived = false;
    if let Some(((shift, zoom), destination)) = passage {
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
    if !arrived {
        if let Ok(window) = windows.single() {
            let position = window.cursor_position();
            let on_panel = pointer
                .get(scene.panel)
                .is_ok_and(RelativeCursorPosition::cursor_over);
            if !on_panel {
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
                if (position != map.pointer || buttons.just_pressed(MouseButton::Left))
                    && let Some(position) = position
                    && let Ok(ray) =
                        camera.viewport_to_world(&GlobalTransform::from(*pose), position)
                    && let Some(slot) = scene.atlas.hit(ray)
                {
                    cursor = slot;
                    if buttons.just_pressed(MouseButton::Left) {
                        selected = scene.atlas.file(slot);
                    }
                }
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
        let origin = library
            .walk_pose
            .as_ref()
            .or(map.ground.as_ref().map(|(saved, _, _)| saved))
            .and_then(|ground| layout::nearest(ground.translation, library.catalog.folders.len()));
        if !arrived
            && map.level == Some(Level::Library)
            && let (Some(origin), Some(target)) = (origin, folder)
            && origin != target
            && let Some(&origin_file) = library.catalog.folders[origin].files.first()
            && let (Some(from), Some(to)) = (
                scene.atlas.focus_shift(origin_file),
                scene.atlas.focus_shift(index),
            )
        {
            if let Ok(mut label) = labels.single_mut() {
                label.0 = format!(
                    "{} → {}",
                    library.catalog.folders[origin].path,
                    file.directory()
                );
            }
            #[cfg(target_arch = "wasm32")]
            browser_travel(file.directory());
            map.framing = None;
            map.passage = Some(Passage {
                file: index,
                start: (map.shift, map.zoom),
                from,
                to,
                elapsed: 0.,
            });
            return;
        }
        map.passage = None;
        map.framing = None;
        let ground = folder.map(layout::view);
        if let Some(scene) = map.scene.take() {
            commands.entity(scene.mesh).despawn();
            commands.entity(scene.selection).despawn();
            native_panel::close(&mut commands, scene.panel, &mut focus, &parents);
        }
        if let Some((saved, ground_projection, ground_msaa)) = map.ground.take() {
            *pose = ground.unwrap_or(saved);
            *projection = ground_projection;
            *msaa = ground_msaa;
            *layers = RenderLayers::layer(0);
        }
        map.level = None;
        library.reader = None;
        library.walk_pose = None;
        library.destination = Some(Destination {
            index,
            before: file.after.is_none(),
            line: 1,
            end: None,
            face: listing::Face::Source,
        });
        #[cfg(target_arch = "wasm32")]
        browser_map("", 0);
        return;
    }
    let manual_view = zoom != map.zoom || shift != map.shift;
    if manual_view {
        (*pose, *projection) = scene.atlas.view(shift, zoom);
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
    map.pointer = pointer_position;
    map.zoom = zoom;
    map.shift = shift;
    map.cursor = cursor;
    if manual_view {
        map.framing = None;
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
                "Click or Enter to read\nWheel zoom · Right-drag pan",
                PAPER,
            );
            native_panel::button::<Choice, Map>(body, "Close", PAPER, Choice::Close);
        });
    Scene {
        atlas,
        mesh: model,
        selection,
        panel,
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryMap)]
    fn browser_map(title: &str, files: u32);
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryTravel)]
    fn browser_travel(folder: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passage_frames_both_folders_before_releasing_the_file() {
        let mut passage = Passage {
            file: 71,
            start: (Vec2::ZERO, 1.),
            from: Vec2::new(-20., 10.),
            to: Vec2::new(45., -5.),
            elapsed: 0.,
        };
        let ((start, zoom), destination) = passage.advance(0.);
        assert_eq!((start, zoom, destination), (Vec2::ZERO, 1., None));
        let ((middle, _), destination) = passage.advance(Passage::FLIGHT * 0.5);
        assert!(middle.x > passage.from.x && middle.x < passage.to.x);
        assert_eq!(destination, None);
        let ((end, zoom), destination) = passage.advance(Passage::FLIGHT * 0.5);
        assert!((end - passage.to).length() < 0.001);
        assert!((zoom - 0.20).abs() < 0.001);
        assert_eq!(destination, None);
        assert_eq!(passage.advance(Passage::SETTLE + 0.01).1, Some(71));
    }
}
