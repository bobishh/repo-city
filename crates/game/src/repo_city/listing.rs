//! In-place source reveal for the city facade. Progress is keyed by snapshot and file.
use avian3d::prelude::{CollisionStart, Position};
use repo_world::comparison::Mark;
use std::collections::HashMap;

use super::*;

const TILE_ROWS: usize = 16;
const ROW_HEIGHT: f32 = 0.28;
pub(super) const WIDTH: f32 = 14.;
pub(super) const DEPTH: f32 = 8.2;
pub(super) const MAX_HEIGHT: f32 = 72.;
const BASE: f32 = 2.;
const PANEL_OFFSET: f32 = 0.06;
pub(super) const FRONT: f32 = DEPTH / 2. + PANEL_OFFSET;

pub(super) fn building_height(file: &repo_world::File) -> f32 {
    let bytes = file
        .after
        .as_ref()
        .or(file.before.as_ref())
        .map_or(0, |blob| blob.size);
    (15. + (bytes as f32).sqrt() * 0.275).min(MAX_HEIGHT)
}

pub(super) fn view(file: &repo_world::File) -> Transform {
    let [x, z] = file.position;
    let y = building_height(file) - 1.7;
    Transform::from_xyz(x, y, z + 15.).looking_at(Vec3::new(x, y, z + FRONT), Vec3::Y)
}

struct Wall {
    top: f32,
    row_height: f32,
}

impl Wall {
    fn new(file: &repo_world::File, count: usize) -> Self {
        let top = building_height(file) - 0.7;
        let row_height = ((top - BASE) / count.max(1) as f32).min(ROW_HEIGHT);
        Self { top, row_height }
    }

    fn row(&self, y: f32, count: usize) -> usize {
        (((self.top - y) / self.row_height).max(0.) as usize).min(count.saturating_sub(1))
    }

    fn y(&self, row: f32) -> f32 {
        self.top - row * self.row_height
    }

    fn reading_height(&self, y: f32, count: usize, delta: f32) -> f32 {
        let margin = count.min(TILE_ROWS) as f32 / 2.;
        (y + delta).clamp(self.y(count as f32 - margin), self.y(margin))
    }
}

#[derive(Resource, Default)]
pub(super) struct Listing {
    revealed: HashSet<(usize, bool, usize)>,
    revealed_counts: HashMap<(usize, bool), usize>,
    diff_side: bool,
    turn: Option<(f32, f32, f32)>,
    rendered: Option<(usize, bool, usize)>,
    source_tiles: Vec<Handle<Image>>,
    dirty_tiles: HashSet<usize>,
    pending_hit: Option<(usize, f32)>,
}

#[derive(Component)]
pub(super) struct ListingSurface;

#[derive(Component)]
pub(super) struct CoverShot;

fn tile_rows(row: usize, total: usize) -> std::ops::Range<usize> {
    let first = row / TILE_ROWS * TILE_ROWS;
    first..(first + TILE_ROWS).min(total)
}

fn rows(explorer: &Explorer) -> Vec<(Option<syntax::Line>, Mark)> {
    explorer
        .highlighted
        .iter()
        .cloned()
        .map(|line| (Some(line), Mark::Context))
        .collect()
}

fn wall_pose(file: &repo_world::File, y: f32, angle: f32, distance: f32) -> Transform {
    let [x, z] = file.position;
    let center = Vec3::new(x, y, z);
    let direction = Vec3::new(angle.sin(), 0., angle.cos());
    Transform::from_translation(center + direction * distance).looking_at(center, Vec3::Y)
}

fn reading_pose(file: &repo_world::File, y: f32, angle: f32) -> Transform {
    wall_pose(file, y, angle, 15. - 5.6 * angle.sin())
}

fn diff_rows(explorer: &Explorer) -> usize {
    explorer.comparison.as_ref().map_or(0, |diff| {
        diff.rows
            .iter()
            .map(|row| {
                usize::from(row.before.is_some() && (row.changed || row.after.is_none()))
                    + usize::from(row.after.is_some())
            })
            .sum()
    })
}

fn projected_row(explorer: &Explorer, row: usize, to_diff: bool) -> usize {
    let Some(diff) = &explorer.comparison else {
        return row;
    };
    let mut side = 0;
    let mut closest = 0;
    for pair in &diff.rows {
        let removed = pair.before.is_some() && (pair.changed || pair.after.is_none());
        let added = pair.after.is_some();
        let count = usize::from(removed) + usize::from(added);
        let number = if explorer.before {
            pair.before
        } else {
            pair.after
        };
        if to_diff {
            if number == Some(row) {
                return side + usize::from(removed && added && !explorer.before);
            }
        } else {
            if let Some(number) = number {
                closest = number;
            }
            if row < side + count {
                return closest;
            }
        }
        side += count;
    }
    if to_diff {
        side.saturating_sub(1)
    } else {
        closest
    }
}

impl Listing {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn invalidate(&mut self) {
        self.rendered = None;
    }

    pub(super) fn turning(&self) -> bool {
        self.turn.is_some()
    }
    pub(super) fn reset_view(&mut self) {
        self.diff_side = false;
        self.turn = None;
    }
    pub(super) fn visible(explorer: &Explorer, navigation: &city_navigation::Navigation) -> bool {
        explorer.comparison_mode == 0 && !explorer.anchored && !navigation.guides_camera()
    }

    pub(super) fn counts(&self, explorer: &Explorer) -> (usize, usize) {
        let total = explorer.highlighted.len();
        let visible = self
            .revealed_counts
            .get(&(explorer.selected, explorer.before))
            .copied()
            .unwrap_or(0)
            .min(total);
        (visible, total)
    }

    fn reveal(&mut self, file: usize, before: bool, lines: impl Iterator<Item = usize>) -> usize {
        let added = lines
            .filter(|line| self.revealed.insert((file, before, *line)))
            .count();
        if added > 0 {
            *self.revealed_counts.entry((file, before)).or_default() += added;
        }
        added
    }

    fn wash(&mut self, explorer: &Explorer, file: &repo_world::File, y: f32) {
        let count = explorer.highlighted.len();
        let row = Wall::new(file, count).row(y, count);
        let rows = tile_rows(row, count);
        if self.reveal(
            explorer.selected,
            explorer.before,
            rows.clone().map(|row| row + 1),
        ) > 0
        {
            self.dirty_tiles.insert(rows.start / TILE_ROWS);
        }
    }
}

pub(super) fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    wheel: Res<AccumulatedMouseScroll>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    mut listing: ResMut<Listing>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
) {
    if picker.is_open() {
        return;
    }
    if !Listing::visible(&explorer, &navigation)
        || !windows.single().is_ok_and(|window| window.focused)
    {
        return;
    }
    let file = &repo.0.files[explorer.selected];
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyG) {
        if explorer.comparison.is_none() {
            explorer.notice = "Diff unavailable for this file.".into();
        } else if listing.turn.is_none() {
            let old_count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let row = Wall::new(file, old_count).row(camera.translation.y, old_count);
            listing.diff_side = !listing.diff_side;
            let count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let target_row = projected_row(&explorer, row, listing.diff_side);
            let y = Wall::new(file, count).y(target_row as f32 + 0.5);
            listing.turn = Some((0., camera.translation.y, y));
        }
    }
    if let Some((elapsed, start, end)) = listing.turn {
        let t = (elapsed + time.delta_secs() / 0.55).min(1.);
        let eased = t * t * (3. - 2. * t);
        let angle =
            if listing.diff_side { eased } else { 1. - eased } * std::f32::consts::FRAC_PI_2;
        *camera = reading_pose(file, start.lerp(end, eased), angle);
        listing.turn = (t < 1.).then_some((t, start, end));
        return;
    }
    let step = f32::from(keys.just_pressed(KeyCode::PageUp))
        - f32::from(keys.just_pressed(KeyCode::PageDown));
    let scroll = if wheel.unit == bevy::input::mouse::MouseScrollUnit::Pixel {
        wheel.delta.y / 40.
    } else {
        wheel.delta.y
    };
    let count = if listing.diff_side {
        diff_rows(&explorer)
    } else {
        explorer.highlighted.len()
    };
    let angle = if listing.diff_side {
        std::f32::consts::FRAC_PI_2
    } else {
        0.
    };
    let wall = Wall::new(file, count);
    if (scroll != 0. || step != 0.) && count > 0 {
        let y = wall.reading_height(
            camera.translation.y,
            count,
            (scroll * 4. + step * TILE_ROWS as f32).clamp(-72., 72.) * wall.row_height,
        );
        *camera = reading_pose(file, y, angle);
    }
    if keys.just_pressed(KeyCode::Home) {
        let y = building_height(file) / 2.;
        let distance = building_height(file) * 1.45 + 7.;
        let pose = wall_pose(file, y, angle, distance);
        let target = pose.translation + pose.forward().as_vec3() * distance;
        *camera = Transform::from_translation(pose.translation.with_y(MAX_HEIGHT + 8.))
            .looking_at(target, Vec3::Y);
    }
}

pub(super) fn cover_impacts(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    mut shots: Query<(&mut physics::Projectile, &Position), With<CoverShot>>,
    facades: Query<&Facade>,
    explorer: Res<Explorer>,
    repo: Res<Repo>,
    mut listing: ResMut<Listing>,
    mut navigation: ResMut<city_navigation::Navigation>,
) {
    for event in collisions.read() {
        let (facade, shot) = if let Ok(facade) = facades.get(event.collider1) {
            (facade, event.collider2)
        } else if let Ok(facade) = facades.get(event.collider2) {
            (facade, event.collider1)
        } else {
            continue;
        };
        let Ok((mut projectile, position)) = shots.get_mut(shot) else {
            continue;
        };
        if projectile.used {
            continue;
        }
        projectile.used = true;
        commands.entity(shot).despawn();
        physics::splash(&mut commands, position.0);
        if !Listing::visible(&explorer, &navigation) {
            continue;
        }
        if facade.index != explorer.selected {
            navigation.shot_destination = Some(facade.index);
            listing.pending_hit = Some((facade.index, position.y));
        } else if explorer.highlighted.is_empty() {
            listing.pending_hit = Some((facade.index, position.y));
        } else {
            listing.wash(&explorer, &repo.0.files[facade.index], position.y);
        }
    }
}

pub(super) fn draw(
    mut commands: Commands,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    set: Res<WorldSet>,
    mut listing: ResMut<Listing>,
    old: Query<Entity, With<ListingSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let visible = Listing::visible(&explorer, &navigation) || navigation.flying_to_facade();
    let count = explorer.highlighted.len();
    let key = visible.then_some((explorer.selected, explorer.before, count));
    if visible
        && count > 0
        && let Some((file, y)) = listing.pending_hit
        && file == explorer.selected
    {
        listing.pending_hit = None;
        listing.wash(&explorer, &repo.0.files[file], y);
    }
    let dirty = std::mem::take(&mut listing.dirty_tiles);
    if key == listing.rendered {
        if visible {
            for index in dirty {
                let Some(handle) = listing.source_tiles.get(index) else {
                    continue;
                };
                let range = tile_rows(index * TILE_ROWS, count);
                let tile = explorer.highlighted[range]
                    .iter()
                    .cloned()
                    .map(|line| (Some(line), Mark::Context))
                    .collect::<Vec<_>>();
                if let Some(mut image) = images.get_mut(handle) {
                    *image = code_texture::listing(&tile, &|line| {
                        listing
                            .revealed
                            .contains(&(explorer.selected, explorer.before, line))
                    });
                }
            }
        }
        return;
    }
    listing.rendered = key;
    listing.source_tiles.clear();
    for entity in &old {
        commands.entity(entity).despawn();
    }
    if !visible {
        return;
    }

    let file = &repo.0.files[explorer.selected];
    let [x, z] = file.position;
    let current = rows(&explorer);
    let wall = Wall::new(file, count);
    listing.source_tiles = draw_side(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &current,
        Vec3::new(x, wall.top, z + FRONT),
        wall.row_height,
        WIDTH,
        &file.path,
        set.ink,
        |line| {
            listing
                .revealed
                .contains(&(explorer.selected, explorer.before, line))
        },
        0.,
    );
    if let Some(diff) = &explorer.comparison {
        let mut changed = Vec::with_capacity(diff_rows(&explorer));
        for row in &diff.rows {
            if let Some(index) = row.before
                && (row.changed || row.after.is_none())
            {
                changed.push((Some(diff.before[index].clone()), Mark::Removed));
            }
            if let Some(index) = row.after {
                changed.push((
                    Some(diff.after[index].clone()),
                    if row.changed {
                        Mark::Added
                    } else {
                        Mark::Context
                    },
                ));
            }
        }
        let wall = Wall::new(file, changed.len());
        draw_side(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &changed,
            Vec3::new(x + WIDTH / 2. + PANEL_OFFSET, wall.top, z),
            wall.row_height,
            DEPTH,
            "DIFF / BASE → SNAPSHOT",
            set.added,
            |_| true,
            std::f32::consts::FRAC_PI_2,
        );
    }
}

fn draw_side(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    lines: &[(Option<syntax::Line>, Mark)],
    top: Vec3,
    row_height: f32,
    width: f32,
    title: &str,
    tint: [f32; 3],
    revealed: impl Fn(usize) -> bool,
    angle: f32,
) -> Vec<Handle<Image>> {
    let header = if lines.is_empty() {
        format!("{title} / No readable source")
    } else {
        format!("{title} / {} rows", lines.len())
    };
    let header_texture = code_texture::render(&header, &[], tint, 1280, 64);
    plane(
        commands,
        meshes,
        materials,
        images,
        header_texture,
        top + Vec3::Y * 0.4,
        0.8,
        width,
        angle,
    );
    lines
        .chunks(TILE_ROWS)
        .enumerate()
        .map(|(index, tile)| {
            let offset = index * TILE_ROWS;
            let height = tile.len() as f32 * row_height;
            plane(
                commands,
                meshes,
                materials,
                images,
                code_texture::listing(tile, &revealed),
                top - Vec3::Y * (offset as f32 * row_height + height / 2.),
                height,
                width,
                angle,
            )
        })
        .collect()
}

fn plane(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    image: Image,
    center: Vec3,
    height: f32,
    width: f32,
    angle: f32,
) -> Handle<Image> {
    let texture = images.add(image);
    commands.spawn((
        ListingSurface,
        Mesh3d(meshes.add(Rectangle::new(width, height))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture.clone()),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(center).with_rotation(Quat::from_rotation_y(angle)),
    ));
    texture
}

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::{CollisionEventsEnabled, LinearVelocity, SweptCcd};
    use bevy::time::TimeUpdateStrategy;

    fn fixture_file() -> repo_world::File {
        repo_world::File {
            path: "test.rs".into(),
            before: None,
            after: None,
            change: Change::Added,
            position: [0., 0.],
        }
    }

    #[test]
    fn water_hits_the_same_facade_from_all_sides_and_from_above() {
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(48));
        let file = fixture_file();
        let height = building_height(&file);
        let wall = Wall::new(&file, 48);
        let mut repo = repository_picker::empty();
        repo.files.push(file);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            PhysicsPlugins::default(),
        ))
        .insert_resource(Repo(repo))
        .insert_resource(explorer)
        .init_resource::<Listing>()
        .init_resource::<city_navigation::Navigation>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 60.),
        ))
        .add_systems(Update, cover_impacts);
        app.finish();
        app.world_mut().spawn((
            Facade { index: 0, height },
            RigidBody::Static,
            Collider::cuboid(WIDTH, height, DEPTH),
            Transform::from_xyz(0., height / 2., 0.),
        ));
        let positions = [
            (Vec3::Z * 12. + Vec3::Y * wall.y(6.), Vec3::NEG_Z),
            (Vec3::NEG_Z * 12. + Vec3::Y * wall.y(22.), Vec3::Z),
            (Vec3::X * 12. + Vec3::Y * wall.y(38.), Vec3::NEG_X),
            (Vec3::NEG_X * 12. + Vec3::Y * wall.y(6.), Vec3::X),
            (Vec3::Y * (height + 8.), Vec3::ZERO),
        ];
        for (origin, direction) in positions {
            let shot = app
                .world_mut()
                .spawn((
                    CoverShot,
                    physics::Projectile { used: false },
                    RigidBody::Dynamic,
                    Collider::sphere(0.22),
                    SweptCcd::default(),
                    CollisionEventsEnabled,
                    LinearVelocity(direction * 42.),
                    Transform::from_translation(origin),
                ))
                .id();
            for _ in 0..100 {
                app.update();
            }
            assert!(
                app.world().get_entity(shot).is_err(),
                "shot from {origin:?} missed"
            );
        }
        let listing = app.world().resource::<Listing>();
        assert_eq!(listing.counts(app.world().resource::<Explorer>()), (48, 48));
        let mut physics_bodies = app.world_mut().query::<&RigidBody>();
        assert!(
            physics_bodies
                .iter(app.world())
                .all(|body| *body == RigidBody::Static)
        );
    }

    #[test]
    fn repeated_hits_only_update_the_affected_texture_on_a_long_facade() {
        let file = fixture_file();
        let count = TILE_ROWS * 12;
        let y = Wall::new(&file, count).y(TILE_ROWS as f32 + 2.);
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(count));
        let mut app = App::new();
        app.add_plugins(bevy::asset::AssetPlugin::default())
            .insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_asset::<Image>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<Listing>()
            .add_systems(Update, draw);
        app.update();
        let handles = app.world().resource::<Listing>().source_tiles.clone();
        assert_eq!(handles.len(), 12);
        let mut reader = app
            .world()
            .resource::<Messages<AssetEvent<Image>>>()
            .get_cursor_current();
        let mut modified = Vec::new();
        let mesh_count = app.world().resource::<Assets<Mesh>>().len();
        let image_count = app.world().resource::<Assets<Image>>().len();

        for _ in 0..64 {
            app.world_mut()
                .resource_scope(|world, mut listing: Mut<Listing>| {
                    listing.wash(world.resource::<Explorer>(), &file, y);
                });
            app.update();
            let events = app.world().resource::<Messages<AssetEvent<Image>>>();
            modified.extend(reader.read(events).filter_map(|event| match event {
                AssetEvent::Modified { id } => Some(*id),
                _ => None,
            }));
        }

        assert_eq!(app.world().resource::<Listing>().source_tiles, handles);
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), image_count);
        assert_eq!(modified, [handles[1].id()]);
        let listing = app.world().resource::<Listing>();
        assert_eq!(
            listing.counts(app.world().resource::<Explorer>()),
            (TILE_ROWS, count)
        );
        assert!(!listing.revealed.contains(&(1, false, TILE_ROWS + 1)));
        assert!(!listing.revealed.contains(&(0, true, TILE_ROWS + 1)));
    }

    #[test]
    fn scroll_stops_at_first_and_last_strip() {
        let file = fixture_file();
        let count = 47;
        let wall = Wall::new(&file, count);
        let first = view(&file).translation.y;
        let previous = wall.reading_height(first, count, -4.);
        assert!(previous < first);
        let last = wall.reading_height(previous, count, -100.);
        assert!(last < previous);
        assert_eq!(wall.reading_height(last, count, -100.), last);
        let first = wall.reading_height(last, count, 100.);
        assert_eq!(wall.reading_height(first, count, 100.), first);
    }
}
