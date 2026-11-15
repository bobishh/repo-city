//! In-place source reveal for the city facade. Progress is keyed by snapshot and file.
use avian3d::prelude::{CollisionStart, Position};
use repo_world::comparison::Mark;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

use super::*;

const TILE_ROWS: usize = 16;
const TILES_PER_SIDE: usize = 6;
const RETIRING_TILES: usize = 2;
const TEXTURES_PER_FRAME: usize = 2;
pub(super) const WIDTH: f32 = 14.;
pub(super) const DEPTH: f32 = 8.2;
const GUTTER: f32 = 0.7;
pub(super) const MAX_HEIGHT: f32 = 72.;
const BASE: f32 = 0.9;
const ROOF: f32 = 0.88;
const PANEL_OFFSET: f32 = 0.06;
pub(super) const FRONT: f32 = DEPTH / 2. + PANEL_OFFSET;
const OVERVIEW_BANDS: usize = 64;

fn texture_row_height(width: f32) -> f32 {
    width * code_texture::LISTING_ROW_PIXELS as f32 / code_texture::LISTING_WIDTH as f32
}

pub(super) fn block_size(file: &repo_world::File, known_rows: Option<usize>) -> Vec3 {
    let estimated = file
        .after
        .as_ref()
        .or(file.before.as_ref())
        .map_or(1, |blob| (blob.size as usize / 40).max(1));
    let rows = known_rows.unwrap_or(estimated).max(1) as f32;
    let height = (BASE + ROOF + rows * texture_row_height(WIDTH - GUTTER)).min(MAX_HEIGHT);
    let width = ((height - BASE - ROOF) / rows / texture_row_height(1.) + GUTTER).min(WIDTH);
    Vec3::new(width, height, (width * 0.65).clamp(2.4, DEPTH))
}

pub(super) fn view(file: &repo_world::File, count: usize) -> Transform {
    view_from_size(file, count, block_size(file, (count > 0).then_some(count)))
}

pub(super) fn view_from_size(file: &repo_world::File, count: usize, size: Vec3) -> Transform {
    let side = WallSide::new(file, count, false, size);
    side.view(side.wall.y(count.clamp(1, 8) as f32 / 2.))
}

struct Wall {
    top: f32,
    row_height: f32,
}

impl Wall {
    fn new(count: usize, width: f32, height: f32) -> Self {
        let top = height - ROOF;
        let row_height = ((top - BASE) / count.max(1) as f32).min(texture_row_height(width));
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
        let down = (self.y(count as f32 - margin) - y).min(0.);
        let up = (self.y(margin) - y).max(0.);
        y + delta.clamp(down, up)
    }
}

struct RevealProgress {
    rows: usize,
    opened: usize,
    bands: [usize; OVERVIEW_BANDS],
}

impl RevealProgress {
    fn new(rows: usize) -> Self {
        Self {
            rows,
            opened: 0,
            bands: [0; OVERVIEW_BANDS],
        }
    }

    fn bands(&self) -> impl ExactSizeIterator<Item = code_texture::Coverage> + '_ {
        use code_texture::Coverage;
        let bands = self.rows.min(OVERVIEW_BANDS);
        (0..bands).map(move |band| {
            let rows =
                ((band + 1) * self.rows).div_ceil(bands) - (band * self.rows).div_ceil(bands);
            match self.bands[band] {
                0 => Coverage::Closed,
                opened if opened == rows => Coverage::Open,
                _ => Coverage::Partial,
            }
        })
    }
}

#[derive(Resource, Default)]
pub(super) struct Listing {
    revealed: HashSet<(usize, bool, usize)>,
    reveal_progress: HashMap<(usize, bool), RevealProgress>,
    overview_dirty: HashSet<usize>,
    listing_facade: Option<usize>,
    diff_side: bool,
    turn: Option<(f32, Transform, Transform)>,
    scroll_remaining: f32,
    rendered: Option<(usize, bool, usize)>,
    invalidated: bool,
    source_tiles: Tiles,
    diff_tiles: Tiles,
    diff_lines: Vec<(Option<syntax::Line>, Mark)>,
    source_marks: Vec<Mark>,
    pending_hit: Option<(usize, f32)>,
}

#[derive(Component)]
pub(super) struct ListingSurface {
    diff: bool,
}

#[derive(Component)]
pub(super) struct CoverShot;

pub(super) fn overview_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        unlit: true,
        ..default()
    }
}

fn close_to_facade(file: &repo_world::File, camera: &Transform, size: Vec3) -> bool {
    let [x, z] = file.position;
    let distance = (camera.translation.xz() - Vec2::new(x, z)).abs() - size.xz() / 2.;
    distance.max(Vec2::ZERO).length_squared() < 24. * 24.
}

pub(super) fn draw_overview(
    repo: Res<Repo>,
    palette: Res<Palette>,
    mut listing: ResMut<Listing>,
    mut facades: Query<(&Facade, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let active = listing.rendered.map(|(index, _, _)| index);
    if listing.listing_facade != active {
        if let Some(index) = listing.listing_facade {
            listing.overview_dirty.insert(index);
        }
        if let Some(index) = active {
            listing.overview_dirty.insert(index);
        }
        listing.listing_facade = active;
    }
    if listing.overview_dirty.is_empty() {
        return;
    }
    let dirty = std::mem::take(&mut listing.overview_dirty);
    for (facade, mut material) in &mut facades {
        let index = facade.index;
        if !dirty.contains(&index) {
            continue;
        }
        if active == Some(index) {
            material.0 = palette.facade_neutral.clone();
            continue;
        }
        let before = repo.0.files[index].after.is_none();
        let progress = listing.reveal_progress.get(&(index, before));
        match progress {
            None => material.0 = palette.facade_closed.clone(),
            Some(progress) if progress.opened == progress.rows => {
                material.0 = palette.facade_open.clone();
            }
            Some(progress) => {
                let overview = code_texture::facade_overview(progress.bands());
                if material.0 == palette.facade_closed
                    || material.0 == palette.facade_open
                    || material.0 == palette.facade_neutral
                {
                    material.0 = materials.add(overview_material(images.add(overview)));
                } else if let Some(mut texture) = materials
                    .get(&material.0)
                    .and_then(|material| material.base_color_texture.as_ref())
                    .and_then(|texture| images.get_mut(texture))
                {
                    *texture = overview;
                }
            }
        }
    }
}

fn tile_rows(row: usize, total: usize) -> std::ops::Range<usize> {
    let first = row / TILE_ROWS * TILE_ROWS;
    first..(first + TILE_ROWS).min(total)
}

fn wall_pose(file: &repo_world::File, y: f32, angle: f32, distance: f32) -> Transform {
    let [x, z] = file.position;
    let center = Vec3::new(x, y, z);
    let direction = Vec3::new(angle.sin(), 0., angle.cos());
    Transform::from_translation(center + direction * distance).looking_at(center, Vec3::Y)
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
        self.invalidated = true;
    }

    pub(super) fn turning(&self) -> bool {
        self.turn.is_some()
    }
    pub(super) fn reset_view(&mut self) {
        self.diff_side = false;
        self.turn = None;
        self.scroll_remaining = 0.;
    }
    pub(super) fn visible(explorer: &Explorer, navigation: &city_navigation::Navigation) -> bool {
        explorer.comparison_mode == 0 && !explorer.anchored && !navigation.guides_camera()
    }

    pub(super) fn counts(&self, explorer: &Explorer) -> (usize, usize) {
        let total = explorer.highlighted.len();
        let visible = self
            .reveal_progress
            .get(&(explorer.selected, explorer.before))
            .map_or(0, |progress| progress.opened)
            .min(total);
        (visible, total)
    }

    fn wash(&mut self, explorer: &Explorer, file: &repo_world::File, y: f32) {
        let count = explorer.highlighted.len();
        if count == 0 {
            return;
        }
        let row = WallSide::new(
            file,
            count,
            false,
            explorer.facade_size(file, explorer.selected),
        )
        .wall
        .row(y, count);
        let rows = tile_rows(row, count);
        let key = (explorer.selected, explorer.before);
        let progress = self
            .reveal_progress
            .entry(key)
            .or_insert_with(|| RevealProgress::new(count));
        let mut changed = false;
        for row in rows.clone() {
            if self.revealed.insert((key.0, key.1, row + 1)) {
                progress.opened += 1;
                progress.bands[row * count.min(OVERVIEW_BANDS) / count] += 1;
                changed = true;
            }
        }
        if changed {
            self.source_tiles.dirty.insert(rows.start / TILE_ROWS);
            self.source_tiles.preview_dirty = true;
            if explorer.before == file.after.is_none() {
                self.overview_dirty.insert(explorer.selected);
            }
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
    mut camera: Query<&mut Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
) {
    if picker.is_open() {
        listing.scroll_remaining = 0.;
        return;
    }
    if !Listing::visible(&explorer, &navigation)
        || !windows.single().is_ok_and(|window| window.focused)
    {
        listing.scroll_remaining = 0.;
        return;
    }
    let file = &repo.0.files[explorer.selected];
    let size = explorer.facade_size(file, explorer.selected);
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyG) {
        if explorer.comparison.is_none() {
            explorer.notice = "Diff unavailable for this file.".into();
        } else if listing.turn.is_none() {
            listing.scroll_remaining = 0.;
            let old_count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let from = WallSide::new(file, old_count, listing.diff_side, size);
            let row = from.wall.row(from.look_height(&camera), old_count);
            listing.diff_side = !listing.diff_side;
            let count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let side = WallSide::new(file, count, listing.diff_side, size);
            let row = projected_row(&explorer, row, listing.diff_side);
            listing.turn = Some((0., *camera, side.view(side.wall.y(row as f32 + 0.5))));
        }
    }
    if let Some((elapsed, start, end)) = listing.turn {
        let t = (elapsed + time.delta_secs() / 0.7).min(1.);
        let ease = |t: f32| t * t * (3. - 2. * t);
        let [x, z] = file.position;
        let corner = Vec3::new(
            (x + size.x / 2. + 0.5)
                .max(start.translation.x)
                .max(end.translation.x),
            (start.translation.y + end.translation.y) * 0.5,
            (z + size.z / 2. + PANEL_OFFSET + 0.5)
                .max(start.translation.z)
                .max(end.translation.z),
        );
        camera.translation = if t < 0.5 {
            start.translation.lerp(corner, ease(t * 2.))
        } else {
            corner.lerp(end.translation, ease(t * 2. - 1.))
        };
        camera.rotation = start.rotation.slerp(end.rotation, ease(t));
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
    let angle = if listing.diff_side {
        std::f32::consts::FRAC_PI_2
    } else {
        0.
    };
    if keys.just_pressed(KeyCode::Home) {
        listing.scroll_remaining = 0.;
        let y = size.y / 2.;
        let distance = size.y.max(size.x) * 1.45 + 7.;
        let pose = wall_pose(file, y, angle, distance);
        let target = pose.translation + pose.forward().as_vec3() * distance;
        *camera = Transform::from_translation(pose.translation.with_y(size.y * 1.12 + 5.))
            .looking_at(target, Vec3::Y);
        return;
    }
    if scroll != 0. || step != 0. {
        let count = if listing.diff_side {
            listing.diff_lines.len()
        } else {
            explorer.highlighted.len()
        };
        if count > 0 {
            let side = WallSide::new(file, count, listing.diff_side, size);
            let y = side.look_height(&camera);
            let delta =
                (scroll * 4. + step * TILE_ROWS as f32).clamp(-72., 72.) * side.wall.row_height;
            listing.scroll_remaining =
                side.wall
                    .reading_height(y, count, listing.scroll_remaining + delta)
                    - y;
        }
    }
    if listing.scroll_remaining != 0. {
        let remaining = listing.scroll_remaining;
        let step = if remaining.abs() <= 0.001 {
            remaining
        } else {
            remaining * (1. - (-time.delta_secs() / 0.08).exp())
        };
        camera.translation.y = (camera.translation.y + step).max(1.);
        listing.scroll_remaining -= step;
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

#[derive(Default)]
struct Tiles {
    resident: BTreeMap<usize, Tile>,
    dirty: HashSet<usize>,
    preview: Option<Handle<Image>>,
    preview_dirty: bool,
}

#[derive(Clone, Debug)]
struct Tile {
    entity: Entity,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    alpha: f32,
}

struct WallSide {
    top: Vec3,
    wall: Wall,
    width: f32,
    angle: f32,
}

impl WallSide {
    fn new(file: &repo_world::File, count: usize, diff: bool, size: Vec3) -> Self {
        let [x, z] = file.position;
        let (x, z, max_width, angle) = if diff {
            (
                x + size.x / 2. + PANEL_OFFSET,
                z,
                size.z - GUTTER,
                std::f32::consts::FRAC_PI_2,
            )
        } else {
            (x, z + size.z / 2. + PANEL_OFFSET, size.x - GUTTER, 0.)
        };
        let wall = Wall::new(count, max_width, size.y);
        let width = wall.row_height * code_texture::LISTING_WIDTH as f32
            / code_texture::LISTING_ROW_PIXELS as f32;
        Self {
            top: Vec3::new(x, wall.top, z),
            wall,
            width,
            angle,
        }
    }

    fn normal(&self) -> Vec3 {
        Quat::from_rotation_y(self.angle) * Vec3::Z
    }

    fn look_height(&self, camera: &Transform) -> f32 {
        let normal = self.normal();
        let facing = camera.forward().dot(normal);
        let distance = (self.top - camera.translation).dot(normal);
        if facing < -0.04 && distance < 0. {
            camera.translation.y + camera.forward().y * (distance / facing)
        } else {
            camera.translation.y
        }
    }

    fn view(&self, y: f32) -> Transform {
        let target = self.top.with_y(y);
        Transform::from_translation(target + self.normal() * (self.width * 0.84).max(0.18))
            .looking_at(target, Vec3::Y)
    }

    fn window(&self, focus: usize, count: usize) -> Range<usize> {
        let total = count.div_ceil(TILE_ROWS);
        let start = (focus / TILE_ROWS)
            .saturating_sub(TILES_PER_SIDE / 2)
            .min(total.saturating_sub(TILES_PER_SIDE));
        start..(start + TILES_PER_SIDE).min(total)
    }

    fn pose(&self, y: f32) -> Transform {
        Transform::from_translation(self.top.with_y(y))
            .with_rotation(Quat::from_rotation_y(self.angle))
    }
}

struct WallRender<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    images: &'a mut Assets<Image>,
    remaining: usize,
    dt: f32,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Layer {
    Preview,
    Tile,
    Heading,
}

impl Layer {
    fn pose(self, side: &WallSide, y: f32) -> Transform {
        let mut pose = side.pose(y);
        if self == Self::Preview {
            // Behind the readable tiles, still outside the building shell.
            pose.translation -= side.normal() * (side.wall.row_height * 0.08).clamp(0.003, 0.018);
        }
        pose
    }
}

impl WallRender<'_, '_, '_> {
    fn clear(&mut self, tiles: &mut Tiles) {
        for tile in std::mem::take(&mut tiles.resident).into_values() {
            self.images.remove(tile.image.id());
        }
        tiles.dirty.clear();
        if let Some(preview) = tiles.preview.take() {
            self.images.remove(preview.id());
        }
        tiles.preview_dirty = false;
    }

    fn hide_side(&mut self, tiles: &mut Tiles) {
        for tile in std::mem::take(&mut tiles.resident).into_values() {
            self.commands.entity(tile.entity).despawn();
            self.images.remove(tile.image.id());
        }
        tiles.dirty.clear();
    }

    fn plane(&mut self, side: &WallSide, image: Image, y: f32, height: f32, layer: Layer) -> Tile {
        let image = self.images.add(image);
        let alpha = if layer == Layer::Tile { 0. } else { 1. };
        let material = self.materials.add(StandardMaterial {
            base_color: Color::WHITE.with_alpha(alpha),
            base_color_texture: Some(image.clone()),
            alpha_mode: if layer == Layer::Tile {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            unlit: true,
            ..default()
        });
        let entity = self
            .commands
            .spawn((
                ListingSurface {
                    diff: side.angle != 0.,
                },
                Mesh3d(self.meshes.add(Rectangle::new(side.width, height))),
                MeshMaterial3d(material.clone()),
                layer.pose(side, y),
            ))
            .id();
        Tile {
            entity,
            image,
            material,
            alpha,
        }
    }

    fn header(&mut self, side: &WallSide, title: &str, tint: [f32; 3], count: usize) {
        let header = if count == 0 {
            format!("{title} / No readable source")
        } else {
            format!("{title} / {count} rows")
        };
        let image = code_texture::render(&header, &[], tint, 1280, 64);
        let height = side.width * 0.0625;
        self.plane(
            side,
            image,
            side.top.y + height / 2.,
            height,
            Layer::Heading,
        );
    }

    fn preview(
        &mut self,
        side: &WallSide,
        state: &mut Tiles,
        count: usize,
        texture: impl FnOnce() -> Image,
    ) {
        if count == 0 {
            return;
        }
        if let Some(existing) = &state.preview {
            if std::mem::take(&mut state.preview_dirty)
                && let Some(mut image) = self.images.get_mut(existing)
            {
                *image = texture();
            }
        } else {
            let tile = self.plane(
                side,
                texture(),
                side.wall.y(count as f32 / 2.),
                count as f32 * side.wall.row_height,
                Layer::Preview,
            );
            state.preview = Some(tile.image);
            state.preview_dirty = false;
        }
    }

    fn tiles(
        &mut self,
        side: &WallSide,
        state: &mut Tiles,
        read_y: f32,
        count: usize,
        texture: impl Fn(Range<usize>) -> Image,
    ) {
        let focus = side.wall.row(read_y, count);
        let window = side.window(focus, count);
        let mut retiring: Vec<_> = state
            .resident
            .keys()
            .copied()
            .filter(|index| !window.contains(index))
            .collect();
        retiring.sort_unstable_by_key(|index| index.abs_diff(focus / TILE_ROWS));
        retiring.truncate(RETIRING_TILES);
        state.resident.retain(|index, tile| {
            let wanted = window.contains(index);
            if wanted || retiring.contains(index) {
                let sign = if wanted { 1. } else { -1. };
                let alpha = (tile.alpha + sign * self.dt / 0.14).clamp(0., 1.);
                if alpha != tile.alpha {
                    tile.alpha = alpha;
                    if let Some(mut material) = self.materials.get_mut(&tile.material) {
                        material.base_color = Color::WHITE.with_alpha(alpha);
                        material.alpha_mode = if alpha >= 1. {
                            AlphaMode::Opaque
                        } else {
                            AlphaMode::Blend
                        };
                    }
                }
                if wanted || alpha > 0. {
                    return true;
                }
            }
            self.commands.entity(tile.entity).despawn();
            self.images.remove(tile.image.id());
            false
        });
        state
            .dirty
            .retain(|index| state.resident.contains_key(index));
        let mut priority = window.collect::<Vec<_>>();
        priority.sort_unstable_by_key(|index| (index.abs_diff(focus / TILE_ROWS), *index));
        for index in priority {
            if self.remaining == 0 {
                break;
            }
            let rows = tile_rows(index * TILE_ROWS, count);
            if let Some(tile) = state.resident.get(&index) {
                if !state.dirty.remove(&index) {
                    continue;
                }
                if let Some(mut image) = self.images.get_mut(&tile.image) {
                    *image = texture(rows);
                }
            } else {
                let height = rows.len() as f32 * side.wall.row_height;
                let center = side.wall.y((rows.start + rows.end) as f32 / 2.);
                let tile = self.plane(side, texture(rows), center, height, Layer::Tile);
                state.resident.insert(index, tile);
            }
            self.remaining -= 1;
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
    old: Query<(Entity, &ListingSurface)>,
    camera: Query<&Transform, PlayerCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    time: Option<Res<Time>>,
) {
    let visible = Listing::visible(&explorer, &navigation);
    let count = explorer.highlighted.len();
    let key = visible.then_some((explorer.selected, explorer.before, count));
    let changed = key != listing.rendered;
    let diff_changed = std::mem::take(&mut listing.invalidated) || changed;
    let mut render = WallRender {
        commands: &mut commands,
        meshes: &mut meshes,
        materials: &mut materials,
        images: &mut images,
        remaining: TEXTURES_PER_FRAME,
        dt: time.as_ref().map_or(1. / 60., |time| time.delta_secs()),
    };
    if changed {
        listing.rendered = key;
        render.clear(&mut listing.source_tiles);
    }
    if diff_changed {
        let mut source_marks = vec![Mark::Context; count];
        render.clear(&mut listing.diff_tiles);
        listing.diff_lines.clear();
        for (entity, surface) in &old {
            if changed || surface.diff {
                render.commands.entity(entity).despawn();
            }
        }
        if visible && let Some(diff) = &explorer.comparison {
            for row in &diff.rows {
                if row.changed {
                    let index = if explorer.before {
                        row.before
                    } else {
                        row.after
                    };
                    if let Some(mark) = index.and_then(|index| source_marks.get_mut(index)) {
                        *mark = if explorer.before {
                            Mark::Removed
                        } else {
                            Mark::Added
                        };
                    }
                }
                if let Some(index) = row.before
                    && (row.changed || row.after.is_none())
                {
                    listing
                        .diff_lines
                        .push((Some(diff.before[index].clone()), Mark::Removed));
                }
                if let Some(index) = row.after {
                    let mark = if row.changed {
                        Mark::Added
                    } else {
                        Mark::Context
                    };
                    listing
                        .diff_lines
                        .push((Some(diff.after[index].clone()), mark));
                }
            }
        }
        if source_marks != listing.source_marks {
            listing.source_marks = source_marks;
            listing.source_tiles.preview_dirty = true;
            let tiles = &mut listing.source_tiles;
            tiles.dirty.extend(tiles.resident.keys().copied());
        }
    }
    if !visible {
        return;
    }

    let file = &repo.0.files[explorer.selected];
    if count > 0
        && let Some((index, y)) = listing.pending_hit
        && index == explorer.selected
    {
        listing.pending_hit = None;
        listing.wash(&explorer, file, y);
    }

    let size = explorer.facade_size(file, explorer.selected);
    let source = WallSide::new(file, count, false, size);
    let diff_count = listing.diff_lines.len();
    let diff = WallSide::new(file, diff_count, true, size);
    if changed {
        render.header(&source, &file.path, set.ink, count);
    }
    if diff_changed && explorer.comparison.is_some() {
        render.header(&diff, "DIFF / BASE → SNAPSHOT", set.added, diff_count);
    }

    let fallback = view(file, count);
    let camera = camera.single().unwrap_or(&fallback);
    let Listing {
        source_tiles,
        diff_tiles,
        diff_lines,
        source_marks,
        revealed,
        diff_side,
        ..
    } = &mut *listing;
    let is_revealed = |line| revealed.contains(&(explorer.selected, explorer.before, line));
    render.preview(&source, source_tiles, count, || {
        code_texture::listing_preview(
            count,
            |index| (Some(&explorer.highlighted[index]), source_marks[index]),
            is_revealed,
        )
    });
    render.preview(&diff, diff_tiles, diff_count, || {
        code_texture::listing_preview(
            diff_count,
            |index| (diff_lines[index].0.as_ref(), diff_lines[index].1),
            |_| true,
        )
    });
    if !close_to_facade(file, camera, size) {
        render.hide_side(source_tiles);
        render.hide_side(diff_tiles);
        return;
    }
    let source_texture = |rows: Range<usize>| {
        let tile = rows
            .map(|index| {
                (
                    Some(explorer.highlighted[index].clone()),
                    source_marks[index],
                )
            })
            .collect::<Vec<_>>();
        code_texture::listing(&tile, &is_revealed)
    };
    let diff_texture = |rows: Range<usize>| code_texture::listing(&diff_lines[rows], &|_| true);
    if *diff_side {
        render.hide_side(source_tiles);
        render.tiles(
            &diff,
            diff_tiles,
            diff.look_height(camera),
            diff_count,
            &diff_texture,
        );
    } else {
        render.hide_side(diff_tiles);
        render.tiles(
            &source,
            source_tiles,
            source.look_height(camera),
            count,
            &source_texture,
        );
    }
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
    fn readable_tiles_and_previews_keep_the_same_rows_on_distinct_depths() {
        let file = fixture_file();
        for diff in [false, true] {
            let side = WallSide::new(&file, 724, diff, block_size(&file, Some(724)));
            let y = side.wall.y(381.5);
            let clear = Layer::Tile.pose(&side, y);
            let blurred = Layer::Preview.pose(&side, y);
            let separation = (clear.translation - blurred.translation).dot(side.normal());
            assert_eq!(clear.translation.y, blurred.translation.y);
            assert_eq!(clear.rotation, blurred.rotation);
            assert!(separation >= 0.0029 && separation < PANEL_OFFSET);
        }
    }

    #[test]
    fn water_hits_the_same_facade_from_all_sides_and_from_above() {
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(48));
        let file = fixture_file();
        let size = block_size(&file, Some(48));
        let height = size.y;
        let wall = WallSide::new(&file, 48, false, size).wall;
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
            Facade { index: 0 },
            RigidBody::Static,
            Collider::cuboid(size.x, size.y, size.z),
            Transform::from_xyz(0., height / 2., 0.),
        ));
        let positions = [
            (
                Vec3::Z * (size.z / 2. + 0.27) + Vec3::Y * wall.y(6.),
                Vec3::NEG_Z,
            ),
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
    fn overview_marks_coverage_only_at_a_distance_and_keeps_progress() {
        use code_texture::Coverage;
        let file = fixture_file();
        let count = 80;
        let wall = WallSide::new(&file, count, false, block_size(&file, Some(count))).wall;
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let mut explorer = Explorer::new(0);
        explorer.before = true;
        explorer.highlighted = syntax::highlight(&file.path, &"let x = 1;\n".repeat(count));
        let mut images = Assets::default();
        let mut materials = Assets::default();
        let neutral = materials.add(StandardMaterial::default());
        let [closed, open] = [Coverage::Closed, Coverage::Open].map(|state| {
            materials.add(overview_material(
                images.add(code_texture::facade_overview(std::iter::once(state))),
            ))
        });
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(Palette {
                facade_closed: closed.clone(),
                facade_open: open.clone(),
                facade_neutral: neutral.clone(),
                shell: default(),
                edge: default(),
                removed: default(),
                plate: default(),
                unit: default(),
            })
            .insert_resource(materials)
            .insert_resource(images)
            .init_resource::<Listing>()
            .add_systems(Update, draw_overview);
        let facade = app
            .world_mut()
            .spawn((Facade { index: 0 }, MeshMaterial3d(closed.clone())))
            .id();
        app.world_mut().resource_mut::<Listing>().rendered = Some((0, true, count));
        let wash = |app: &mut App, row| {
            app.world_mut()
                .resource_scope(|world, mut listing: Mut<Listing>| {
                    listing.wash(world.resource::<Explorer>(), &file, wall.y(row));
                });
        };
        let current = |app: &App| {
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(facade)
                .unwrap()
                .0
                .clone()
        };
        wash(&mut app, 1.);
        app.update();
        assert_eq!(current(&app), neutral);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 2);
        let bands: Vec<_> = app.world().resource::<Listing>().reveal_progress[&(0, true)]
            .bands()
            .collect();
        assert_eq!(bands[0], Coverage::Open);
        assert_eq!(bands[12], Coverage::Partial);
        assert_eq!(bands[13], Coverage::Closed);

        app.world_mut().resource_mut::<Listing>().rendered = None;
        app.update();
        let partial = current(&app);
        assert_ne!(partial, closed);
        assert_ne!(partial, open);
        wash(&mut app, 17.);
        app.update();
        assert_eq!(current(&app), partial);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 3);
        for row in (TILE_ROWS * 2..count).step_by(TILE_ROWS) {
            wash(&mut app, row as f32 + 0.5);
        }
        app.update();
        assert_eq!(current(&app), open);
        app.world_mut().resource_mut::<Listing>().rendered = Some((0, true, count));
        app.update();
        assert_eq!(current(&app), neutral);
        app.world_mut().resource_mut::<Listing>().rendered = None;
        app.update();
        assert_eq!(current(&app), open);
    }

    #[test]
    fn repeated_hits_only_update_the_affected_texture_on_a_long_facade() {
        let file = fixture_file();
        let count = TILE_ROWS * 12;
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
        for _ in 0..TILES_PER_SIDE.div_ceil(TEXTURES_PER_FRAME) {
            app.update();
        }
        let handles = app
            .world()
            .resource::<Listing>()
            .source_tiles
            .resident
            .clone();
        let first_y = app
            .world()
            .get::<Transform>(handles[&0].entity)
            .unwrap()
            .translation
            .y;
        let next_y = app
            .world()
            .get::<Transform>(handles[&1].entity)
            .unwrap()
            .translation
            .y;
        let wall = WallSide::new(&file, count, false, block_size(&file, Some(count))).wall;
        assert!((first_y - next_y - TILE_ROWS as f32 * wall.row_height).abs() < 0.001);
        let y = wall.y(TILE_ROWS as f32 + 2.5);
        let preview = app
            .world()
            .resource::<Listing>()
            .source_tiles
            .preview
            .clone()
            .unwrap();
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

        assert_eq!(
            app.world().resource::<Listing>().source_tiles.resident[&1].entity,
            handles[&1].entity
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), image_count);
        assert_eq!(modified, [preview.id(), handles[&1].image.id()]);
        let listing = app.world().resource::<Listing>();
        assert_eq!(
            listing.counts(app.world().resource::<Explorer>()),
            (TILE_ROWS, count)
        );
        assert!(!listing.revealed.contains(&(1, false, TILE_ROWS + 1)));
        assert!(!listing.revealed.contains(&(0, true, TILE_ROWS + 1)));
    }

    #[test]
    fn opening_a_long_file_bounds_texture_work_per_frame() {
        let file = fixture_file();
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let count = 1614;
        let source = "let x = 1;\n".repeat(count);
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight(&file.path, &source);
        explorer.comparison = Some(Comparison::new(&file.path, &source, &source).unwrap());
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<Listing>()
            .add_systems(Update, draw);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), view(&file, count)))
            .id();
        app.update();
        assert!(app.world().resource::<Assets<Image>>().len() <= 6);
        let limit = 4 + TILES_PER_SIDE + RETIRING_TILES;
        let source = WallSide::new(&file, count, false, block_size(&file, Some(count)));
        for _ in 0..limit {
            app.update();
        }
        let preview = app
            .world()
            .resource::<Listing>()
            .source_tiles
            .preview
            .clone();
        let mut shared = None;
        for row in [35, 64, count - 1, 384, 35] {
            let target = source.wall.y(row as f32 + 0.5);
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = source.view(target);
            app.update();
            if row == 64 {
                let tiles = &app.world().resource::<Listing>().source_tiles.resident;
                assert!((0. ..1.).contains(&tiles[&0].alpha) && tiles[&0].alpha > 0.);
                assert_eq!(tiles[&6].alpha, 0.);
            }
            for _ in 0..limit {
                app.update();
                assert!(app.world().resource::<Assets<Image>>().len() <= limit);
            }
            let listing = app.world().resource::<Listing>();
            assert_eq!(listing.source_tiles.preview, preview);
            assert!(listing.diff_tiles.resident.is_empty());
            let tiles = &listing.source_tiles.resident;
            assert!(tiles.contains_key(&(row / TILE_ROWS)));
            if row == 35 {
                let tile = &tiles[&(64 / TILE_ROWS)].entity;
                shared = Some((*tile, *app.world().get::<Transform>(*tile).unwrap()));
            } else if row == 64 {
                let tile = &tiles[&(64 / TILE_ROWS)].entity;
                let (entity, pose) = shared.unwrap();
                assert_eq!(*tile, entity);
                assert_eq!(*app.world().get::<Transform>(entity).unwrap(), pose);
            }
        }
        app.world_mut().resource_mut::<Listing>().diff_side = true;
        app.update();
        let listing = app.world().resource::<Listing>();
        assert!(listing.source_tiles.resident.is_empty());
        assert!(!listing.diff_tiles.resident.is_empty());
        assert_eq!(listing.source_tiles.preview, preview);
    }

    #[test]
    fn source_and_block_share_both_edges_and_the_last_row() {
        let file = fixture_file();
        let short = block_size(&file, Some(23));
        let medium = block_size(&file, Some(329));
        let tall = block_size(&file, Some(724));
        assert!(short.y < 8. && tall.x < medium.x && tall.y <= MAX_HEIGHT);
        for rows in [1, 23, 329, 724, 1614] {
            let size = block_size(&file, Some(rows));
            let side = WallSide::new(&file, rows, false, size);
            assert!((side.width + GUTTER - size.x).abs() < 0.001);
            assert!((side.wall.y(rows as f32) - BASE).abs() < 0.001);
        }
    }

    #[test]
    fn scroll_stops_at_first_and_last_strip() {
        let file = fixture_file();
        let count = 47;
        let wall = WallSide::new(&file, count, false, block_size(&file, Some(count))).wall;
        let first = view(&file, count).translation.y;
        let previous = wall.reading_height(first, count, -4.);
        assert!(previous < first);
        let last = wall.reading_height(previous, count, -100.);
        assert!(last < previous);
        assert_eq!(wall.reading_height(last, count, -100.), last);
        let first = wall.reading_height(last, count, 100.);
        assert_eq!(wall.reading_height(first, count, 100.), first);
    }

    #[test]
    fn page_down_eases_along_the_wall_without_reframing_an_oblique_view() {
        let file = fixture_file();
        let mut repo = repository_picker::empty();
        repo.files.push(file);
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(80));
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(16));
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(time)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<Listing>()
            .add_systems(Update, controls);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let start =
            Transform::from_xyz(3., 13., 30.).looking_at(Vec3::new(1., 10., FRONT), Vec3::Y);
        let camera = app.world_mut().spawn((Camera3d::default(), start)).id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::PageDown);
        app.update();
        let first = *app.world().get::<Transform>(camera).unwrap();
        assert!(first.translation.y < start.translation.y);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        for _ in 0..40 {
            app.update();
            let pose = app.world().get::<Transform>(camera).unwrap();
            assert_eq!(pose.rotation, start.rotation);
            assert_eq!(pose.translation.xz(), start.translation.xz());
        }
        let end = app.world().get::<Transform>(camera).unwrap();
        assert!(end.translation.y < first.translation.y);
    }
}
