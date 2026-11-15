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
const TEXTURES_PER_FRAME: usize = 2;
const ROW_HEIGHT: f32 = 0.28;
pub(super) const WIDTH: f32 = 14.;
pub(super) const DEPTH: f32 = 8.2;
// The side view reads from the gap between adjacent file blocks.
const DIFF_WIDTH: f32 = 3.1;
pub(super) const MAX_HEIGHT: f32 = 72.;
const BASE: f32 = 2.;
const PANEL_OFFSET: f32 = 0.06;
pub(super) const FRONT: f32 = DEPTH / 2. + PANEL_OFFSET;
const OVERVIEW_BANDS: usize = 64;

fn texture_row_height(width: f32) -> f32 {
    width * code_texture::LISTING_ROW_PIXELS as f32 / code_texture::LISTING_WIDTH as f32
}

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
    close_facade: Option<usize>,
    diff_side: bool,
    turn: Option<(f32, f32, f32)>,
    reading: Option<((usize, bool), f32)>,
    scroll_remaining: f32,
    rendered: Option<(usize, bool, usize)>,
    invalidated: bool,
    source_tiles: Tiles,
    diff_tiles: Tiles,
    diff_lines: Vec<(Option<syntax::Line>, Mark)>,
    pending_hit: Option<(usize, f32)>,
}

#[derive(Component)]
pub(super) struct ListingSurface;

#[derive(Component)]
pub(super) struct CoverShot;

pub(super) fn overview_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        unlit: true,
        ..default()
    }
}

fn close_to_facade(file: &repo_world::File, camera: &Transform, was_close: bool) -> bool {
    let [x, z] = file.position;
    let distance =
        (camera.translation.xz() - Vec2::new(x, z)).abs() - Vec2::new(WIDTH / 2., DEPTH / 2.);
    let limit = if was_close { 24. } else { 20. };
    distance.max(Vec2::ZERO).length_squared() < limit * limit
}

pub(super) fn draw_overview(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    palette: Res<Palette>,
    camera: Query<&Transform, With<Camera3d>>,
    mut listing: ResMut<Listing>,
    mut facades: Query<(&Facade, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let close = camera
        .single()
        .is_ok_and(|camera| {
            close_to_facade(
                &repo.0.files[explorer.selected],
                camera,
                listing.close_facade == Some(explorer.selected),
            )
        })
        .then_some(explorer.selected);
    if listing.close_facade != close {
        if let Some(index) = listing.close_facade {
            listing.overview_dirty.insert(index);
        }
        if let Some(index) = close {
            listing.overview_dirty.insert(index);
        }
        listing.close_facade = close;
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
        let before = repo.0.files[index].after.is_none();
        let progress = listing
            .reveal_progress
            .get(&(index, before))
            .filter(|_| close != Some(index));
        match progress {
            None => material.0 = palette.facade_closed.clone(),
            Some(progress) if progress.opened == progress.rows => {
                material.0 = palette.facade_open.clone();
            }
            Some(progress) => {
                let overview = code_texture::facade_overview(progress.bands());
                if material.0 == palette.facade_closed || material.0 == palette.facade_open {
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
        self.invalidated = true;
    }

    pub(super) fn turning(&self) -> bool {
        self.turn.is_some()
    }
    pub(super) fn reset_view(&mut self) {
        if self.diff_side {
            self.reading = None;
        }
        self.diff_side = false;
        self.turn = None;
        self.scroll_remaining = 0.;
    }
    pub(super) fn start_file(&mut self) {
        self.reset_view();
        self.reading = None;
    }
    fn reading_y(&self, explorer: &Explorer, file: &repo_world::File) -> f32 {
        self.reading
            .filter(|(key, _)| *key == (explorer.selected, explorer.before))
            .map_or_else(|| view(file).translation.y, |(_, y)| y)
    }
    fn set_reading_y(&mut self, explorer: &Explorer, y: f32) {
        self.reading = Some(((explorer.selected, explorer.before), y));
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
        let row = self
            .source_tiles
            .page
            .and_then(|page| page.hit_row(y, count))
            .unwrap_or_else(|| Wall::new(file, count).row(y, count));
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
    mut camera: Query<&mut Transform, With<Camera3d>>,
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
            let row = Wall::new(file, old_count).row(listing.reading_y(&explorer, file), old_count);
            listing.diff_side = !listing.diff_side;
            let count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let target_row = projected_row(&explorer, row, listing.diff_side);
            let y = Wall::new(file, count).y(target_row as f32 + 0.5);
            listing.set_reading_y(&explorer, y);
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
    if keys.just_pressed(KeyCode::Home) {
        listing.scroll_remaining = 0.;
        let y = building_height(file) / 2.;
        let distance = building_height(file) * 1.45 + 7.;
        let pose = wall_pose(file, y, angle, distance);
        let target = pose.translation + pose.forward().as_vec3() * distance;
        *camera = Transform::from_translation(pose.translation.with_y(MAX_HEIGHT + 8.))
            .looking_at(target, Vec3::Y);
        return;
    }
    if (scroll != 0. || step != 0.) && count > 0 {
        let wall = Wall::new(file, count);
        let y = listing.reading_y(&explorer, file);
        let delta = (scroll * 4. + step * TILE_ROWS as f32).clamp(-72., 72.) * wall.row_height;
        listing.scroll_remaining =
            wall.reading_height(y, count, listing.scroll_remaining + delta) - y;
    }
    if listing.scroll_remaining != 0. {
        let remaining = listing.scroll_remaining;
        let step = if remaining.abs() <= 0.001 {
            remaining
        } else {
            remaining * (1. - (-time.delta_secs() / 0.08).exp())
        };
        let y = listing.reading_y(&explorer, file);
        listing.set_reading_y(&explorer, y + step);
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
    resident: BTreeMap<usize, (Entity, Handle<Image>)>,
    dirty: HashSet<usize>,
    page: Option<WallPage>,
}

#[derive(Clone, Copy, PartialEq)]
struct WallPage {
    first_tile: usize,
    end_tile: usize,
    top: f32,
    row_height: f32,
}

impl WallPage {
    fn y(&self, row: f32) -> f32 {
        self.top - (row - (self.first_tile * TILE_ROWS) as f32) * self.row_height
    }

    fn hit_row(&self, y: f32, count: usize) -> Option<usize> {
        let local = (self.top - y) / self.row_height;
        let row = self.first_tile * TILE_ROWS + local.max(0.) as usize;
        (local >= 0. && row < count.min(self.end_tile * TILE_ROWS)).then_some(row)
    }
}

struct WallSide {
    top: Vec3,
    row_height: f32,
    width: f32,
    angle: f32,
}

impl WallSide {
    fn new(file: &repo_world::File, count: usize, diff: bool) -> Self {
        let [x, z] = file.position;
        let wall = Wall::new(file, count);
        let (x, z, width, angle) = if diff {
            (
                x + WIDTH / 2. + PANEL_OFFSET,
                z,
                DIFF_WIDTH,
                std::f32::consts::FRAC_PI_2,
            )
        } else {
            (x, z + FRONT, WIDTH, 0.)
        };
        Self {
            top: Vec3::new(x, wall.top, z),
            row_height: wall.row_height,
            width,
            angle,
        }
    }

    fn page(&self, focus: f32, count: usize) -> WallPage {
        let row_height = texture_row_height(self.width);
        let tile_height = TILE_ROWS as f32 * row_height;
        let number_of_tiles = ((self.top.y - BASE) / tile_height).floor() as usize;
        let capacity = number_of_tiles.clamp(1, TILES_PER_SIDE);
        let total = count.div_ceil(TILE_ROWS);
        let first_tile = (focus as usize / TILE_ROWS)
            .saturating_sub(capacity / 2)
            .min(total.saturating_sub(capacity));
        let end_tile = (first_tile + capacity).min(total);
        let first_row = first_tile * TILE_ROWS;
        let end_row = count.min(end_tile * TILE_ROWS);
        let height = (end_row - first_row) as f32 * row_height;
        let focus_y = self.top.y - focus * self.row_height;
        let top =
            (focus_y + (focus - first_row as f32) * row_height).clamp(BASE + height, self.top.y);
        WallPage {
            first_tile,
            end_tile,
            top,
            row_height,
        }
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
}

impl WallRender<'_, '_, '_> {
    fn clear(&mut self, tiles: &mut Tiles) {
        for (_, texture) in std::mem::take(&mut tiles.resident).into_values() {
            self.images.remove(texture.id());
        }
        tiles.dirty.clear();
        tiles.page = None;
    }

    fn hide_side(&mut self, tiles: &mut Tiles) {
        for (entity, _) in tiles.resident.values() {
            self.commands.entity(*entity).despawn();
        }
        self.clear(tiles);
    }

    fn plane(
        &mut self,
        side: &WallSide,
        image: Image,
        y: f32,
        height: f32,
    ) -> (Entity, Handle<Image>) {
        let texture = self.images.add(image);
        let entity = self
            .commands
            .spawn((
                ListingSurface,
                Mesh3d(self.meshes.add(Rectangle::new(side.width, height))),
                MeshMaterial3d(self.materials.add(StandardMaterial {
                    base_color_texture: Some(texture.clone()),
                    unlit: true,
                    cull_mode: None,
                    ..default()
                })),
                side.pose(y),
            ))
            .id();
        (entity, texture)
    }

    fn header(&mut self, side: &WallSide, title: &str, tint: [f32; 3], count: usize) {
        let header = if count == 0 {
            format!("{title} / No readable source")
        } else {
            format!("{title} / {count} rows")
        };
        let image = code_texture::render(&header, &[], tint, 1280, 64);
        self.plane(side, image, side.top.y + 0.4, 0.8);
    }

    fn tiles(
        &mut self,
        side: &WallSide,
        state: &mut Tiles,
        read_y: f32,
        count: usize,
        texture: impl Fn(Range<usize>) -> Image,
    ) {
        let focus =
            ((side.top.y - read_y) / side.row_height).clamp(0., count.saturating_sub(1) as f32);
        let page = side.page(focus, count);
        let reposition = state.page.replace(page) != Some(page);
        let window = page.first_tile..page.end_tile;
        state.resident.retain(|index, (entity, texture)| {
            if window.contains(index) {
                if reposition {
                    let rows = tile_rows(index * TILE_ROWS, count);
                    let center = page.y((rows.start + rows.end) as f32 / 2.);
                    self.commands.entity(*entity).insert(side.pose(center));
                }
                return true;
            }
            self.commands.entity(*entity).despawn();
            self.images.remove(texture.id());
            false
        });
        state
            .dirty
            .retain(|index| state.resident.contains_key(index));
        let mut priority = window.collect::<Vec<_>>();
        priority.sort_unstable_by_key(|index| (index.abs_diff(focus as usize / TILE_ROWS), *index));
        for index in priority {
            if self.remaining == 0 {
                break;
            }
            let rows = tile_rows(index * TILE_ROWS, count);
            if let Some((_, handle)) = state.resident.get(&index) {
                if !state.dirty.remove(&index) {
                    continue;
                }
                if let Some(mut image) = self.images.get_mut(handle) {
                    *image = texture(rows);
                }
            } else {
                let height = rows.len() as f32 * page.row_height;
                let center = page.y((rows.start + rows.end) as f32 / 2.);
                let tile = self.plane(side, texture(rows), center, height);
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
    old: Query<Entity, With<ListingSurface>>,
    camera: Query<&Transform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let visible = Listing::visible(&explorer, &navigation);
    let count = explorer.highlighted.len();
    let key = visible.then_some((explorer.selected, explorer.before, count));
    let changed = std::mem::take(&mut listing.invalidated) || key != listing.rendered;
    let mut render = WallRender {
        commands: &mut commands,
        meshes: &mut meshes,
        materials: &mut materials,
        images: &mut images,
        remaining: TEXTURES_PER_FRAME,
    };
    if changed {
        listing.rendered = key;
        render.clear(&mut listing.source_tiles);
        render.clear(&mut listing.diff_tiles);
        listing.diff_lines.clear();
        for entity in &old {
            render.commands.entity(entity).despawn();
        }
        if visible && let Some(diff) = &explorer.comparison {
            for row in &diff.rows {
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

    let source = WallSide::new(file, count, false);
    let diff_count = listing.diff_lines.len();
    let diff = WallSide::new(file, diff_count, true);
    if changed {
        render.header(&source, &file.path, set.ink, count);
        if explorer.comparison.is_some() {
            render.header(&diff, "DIFF / BASE → SNAPSHOT", set.added, diff_count);
        }
    }

    let fallback = view(file);
    let camera = camera.single().unwrap_or(&fallback);
    if !close_to_facade(
        file,
        camera,
        listing.close_facade == Some(explorer.selected),
    ) {
        render.hide_side(&mut listing.source_tiles);
        render.hide_side(&mut listing.diff_tiles);
        return;
    }
    let read_y = listing.reading_y(&explorer, file);
    let Listing {
        source_tiles,
        diff_tiles,
        diff_lines,
        revealed,
        diff_side,
        ..
    } = &mut *listing;
    let source_texture = |rows: Range<usize>| {
        let tile = explorer.highlighted[rows]
            .iter()
            .cloned()
            .map(|line| (Some(line), Mark::Context))
            .collect::<Vec<_>>();
        code_texture::listing(&tile, &|line| {
            revealed.contains(&(explorer.selected, explorer.before, line))
        })
    };
    let diff_texture = |rows: Range<usize>| code_texture::listing(&diff_lines[rows], &|_| true);
    if *diff_side {
        render.hide_side(source_tiles);
        render.tiles(&diff, diff_tiles, read_y, diff_count, &diff_texture);
    } else {
        render.hide_side(diff_tiles);
        render.tiles(&source, source_tiles, read_y, count, &source_texture);
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
    fn overview_marks_coverage_only_at_a_distance_and_keeps_progress() {
        use code_texture::Coverage;
        let file = fixture_file();
        let count = 80;
        let wall = Wall::new(&file, count);
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let mut explorer = Explorer::new(0);
        explorer.before = true;
        explorer.highlighted = syntax::highlight(&file.path, &"let x = 1;\n".repeat(count));
        let mut images = Assets::default();
        let mut materials = Assets::default();
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
            .spawn((
                Facade {
                    index: 0,
                    height: building_height(&file),
                },
                MeshMaterial3d(closed.clone()),
            ))
            .id();
        let close = view(&file);
        let camera = app.world_mut().spawn((Camera3d::default(), close)).id();
        let far = Transform::from_translation(close.translation + Vec3::Z * 40.);
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
        assert_eq!(current(&app), closed);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 2);
        let bands: Vec<_> = app.world().resource::<Listing>().reveal_progress[&(0, true)]
            .bands()
            .collect();
        assert_eq!(bands[0], Coverage::Open);
        assert_eq!(bands[12], Coverage::Partial);
        assert_eq!(bands[13], Coverage::Closed);

        *app.world_mut().get_mut::<Transform>(camera).unwrap() = far;
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
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = close;
        app.update();
        assert_eq!(current(&app), closed);
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = far;
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
            .get::<Transform>(handles[&0].0)
            .unwrap()
            .translation
            .y;
        let next_y = app
            .world()
            .get::<Transform>(handles[&1].0)
            .unwrap()
            .translation
            .y;
        assert!((first_y - next_y - TILE_ROWS as f32 * texture_row_height(WIDTH)).abs() < 0.001);
        let page = app.world().resource::<Listing>().source_tiles.page.unwrap();
        let y = page.y(TILE_ROWS as f32 + 2.5);
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
            app.world().resource::<Listing>().source_tiles.resident,
            handles
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), image_count);
        assert_eq!(modified, [handles[&1].1.id()]);
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
        let source = "let x = 1;\n".repeat(TILE_ROWS * 40);
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
            .spawn((Camera3d::default(), view(&file)))
            .id();
        app.update();
        assert!(app.world().resource::<Assets<Image>>().len() <= 4);
        let limit = 2 + TILES_PER_SIDE;
        let wall = Wall::new(&file, TILE_ROWS * 40);
        for _ in 0..limit {
            app.update();
        }
        let initial = app.world().resource::<Listing>().source_tiles.page;
        let resident = app
            .world()
            .resource::<Listing>()
            .source_tiles
            .resident
            .clone();
        let mut pose = view(&file);
        pose.translation += Vec3::new(2., -8., 0.);
        pose.rotate_x(-0.2);
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = pose;
        app.update();
        let listing = app.world().resource::<Listing>();
        assert!(
            listing.source_tiles.page == initial,
            "camera movement moved the listing"
        );
        assert_eq!(listing.source_tiles.resident, resident);
        for first_row in [0, TILE_ROWS * 39, TILE_ROWS * 12, 0] {
            app.world_mut().resource_mut::<Listing>().reading =
                Some(((0, false), wall.y(first_row as f32 + 0.5)));
            for _ in 0..limit {
                app.update();
                assert!(app.world().resource::<Assets<Image>>().len() <= limit);
            }
            let listing = app.world().resource::<Listing>();
            assert!(
                listing
                    .source_tiles
                    .resident
                    .contains_key(&(first_row / TILE_ROWS))
            );
            assert!(listing.diff_tiles.resident.is_empty());
        }
        app.world_mut().resource_mut::<Listing>().diff_side = true;
        app.update();
        let listing = app.world().resource::<Listing>();
        assert!(listing.source_tiles.resident.is_empty());
        assert!(!listing.diff_tiles.resident.is_empty());
        let page = listing.diff_tiles.page.unwrap();
        assert_eq!(page.row_height, texture_row_height(DIFF_WIDTH));
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
