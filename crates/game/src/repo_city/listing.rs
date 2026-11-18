//! In-place source reveal for the city facade. Progress is keyed by snapshot and file.
use repo_world::comparison::Mark;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

use super::*;

mod cover;
pub(super) mod find;
mod paint;
pub(super) use cover::Plugin as CoverPlugin;
pub(super) use paint::draw as paint_notes;

pub(super) const TILE_ROWS: usize = 16;
const TILES_PER_SIDE: usize = 6;
const MAX_VISIBLE_TILES: usize = 10;
const RETIRING_TILES: usize = 2;
const TEXTURES_PER_FRAME: usize = 2;
pub(super) const WIDTH: f32 = 14.;
const HEADING_RATIO: f32 = 0.055;
pub(super) const MAX_HEIGHT: f32 = 72.;
pub(super) const PANEL_OFFSET: f32 = 0.06;
const OVERVIEW_BANDS: usize = 64;
const DETAIL_RAY_REACH: f32 = 24.;

pub(super) fn tool_range(width: f32) -> f32 {
    (width * 0.96).max(0.2)
}

fn texture_row_height(width: f32) -> f32 {
    width * code_texture::LISTING_ROW_PIXELS as f32 / code_texture::LISTING_WIDTH as f32
}

pub(super) fn block_size(file: &repo_world::File) -> Vec3 {
    size_from_rows(estimated_rows(file))
}

fn estimated_rows(file: &repo_world::File) -> usize {
    file.after
        .as_ref()
        .or(file.before.as_ref())
        .map_or(1, |blob| (blob.size / 40).max(1))
}

fn size_from_rows(rows: usize) -> Vec3 {
    let rows = rows.max(1) as f32;
    let aspect = rows * texture_row_height(1.) + HEADING_RATIO;
    let height = (WIDTH * aspect).min(MAX_HEIGHT);
    let width = height / aspect;
    Vec3::new(width, height, width)
}

pub(super) fn view(file: &repo_world::File, count: usize) -> Transform {
    view_from_size(file, count, block_size(file))
}

pub(super) fn view_from_size(file: &repo_world::File, count: usize, size: Vec3) -> Transform {
    let side = WallSide::new(file, count, Face::Source, size);
    let row = if count == 0 {
        4.
    } else {
        count.min(8) as f32 / 2.
    };
    side.view(side.wall.y(row).max(side.wall.top * 0.5))
}

struct Wall {
    top: f32,
    row_height: f32,
}

impl Wall {
    fn new(count: usize, top: f32) -> Self {
        let row_height = top / count.max(1) as f32;
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
                _ if self.opened == self.rows => Coverage::Open,
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
    viewing: Option<Face>,
    destination: Option<Face>,
    wash_preview: Option<WashPreview>,
    water_on: bool,
    blame: super::blame::Blame,
    turn: Option<(f32, Transform, Transform)>,
    scroll_remaining: f32,
    rendered: Option<(usize, bool, usize)>,
    invalidated: bool,
    recolored: bool,
    source_tiles: Tiles,
    diff_tiles: Tiles,
    blame_tiles: Tiles,
    blame_header: Option<Tile>,
    diff_lines: Vec<(Option<syntax::Line>, Mark)>,
    source_marks: Vec<Mark>,
    preview_marks: Vec<(review_notes::Anchor, Vec4)>,
}

#[derive(Clone, Debug)]
struct WashPreview {
    file: usize,
    before: bool,
    rows: Range<usize>,
    elapsed: f32,
    seconds: f32,
}

impl WashPreview {
    fn advance(
        active: &mut Option<Self>,
        file: usize,
        before: bool,
        rows: Range<usize>,
        delta: f32,
        estimate: impl FnOnce() -> f32,
    ) -> bool {
        if active
            .as_ref()
            .is_none_or(|p| p.file != file || p.before != before || p.rows != rows)
        {
            *active = Some(Self {
                file,
                before,
                rows,
                elapsed: 0.,
                seconds: estimate(),
            });
        }
        let preview = active.as_mut().expect("the current tile is previewed");
        preview.elapsed += delta;
        preview.elapsed >= preview.seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Face {
    Source,
    Diff,
    Blame,
}

#[derive(Component)]
pub(super) struct ListingSurface(Face);

pub(super) fn overview_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        unlit: true,
        ..default()
    }
}

pub(super) fn draw_overview(
    repo: Res<Repo>,
    palette: Res<Palette>,
    mut listing: ResMut<Listing>,
    mut facades: Query<(&Facade, &mut MeshMaterial3d<StandardMaterial>), Without<FacadeRim>>,
    mut rims: Query<(&FacadeRim, &mut MeshMaterial3d<StandardMaterial>), Without<Facade>>,
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
    let progress = |index| {
        let file: &repo_world::File = &repo.0.files[index];
        listing.reveal_progress.get(&(index, file.after.is_none()))
    };
    for (rim, mut material) in &mut rims {
        if dirty.contains(&rim.0) {
            let state = match progress(rim.0) {
                None => code_texture::Coverage::Closed,
                Some(progress) if progress.opened == progress.rows => code_texture::Coverage::Open,
                Some(_) => code_texture::Coverage::Partial,
            };
            material.0 = palette.facade_rims[state as usize].clone();
        }
    }
    for (facade, mut material) in &mut facades {
        let index = facade.index;
        if !dirty.contains(&index) {
            continue;
        }
        if active == Some(index) {
            material.0 = palette.facade_neutral.clone();
            continue;
        }
        match progress(index) {
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

fn nearest_unopened(preferred: usize, count: usize, opened: impl Fn(usize) -> bool) -> usize {
    (0..count)
        .find_map(|offset| {
            preferred
                .checked_sub(offset)
                .filter(|&row| !opened(row))
                .or_else(|| {
                    preferred
                        .checked_add(offset)
                        .filter(|&row| row < count && !opened(row))
                })
        })
        .unwrap_or(0)
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

fn diff_row_width(row: &repo_world::comparison::Row) -> usize {
    usize::from(row.before.is_some() && (row.changed || row.after.is_none()))
        + usize::from(row.after.is_some())
}

fn diff_rows(explorer: &Explorer) -> usize {
    explorer
        .comparison
        .as_ref()
        .filter(|diff| !diff.hunks.is_empty())
        .map_or(0, |diff| diff.rows.iter().map(diff_row_width).sum())
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
        let count = diff_row_width(pair);
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
    pub(super) fn building_loaded(&mut self, index: usize) {
        if self.reveal_progress.contains_key(&(index, true))
            || self.reveal_progress.contains_key(&(index, false))
            || self.rendered.is_some_and(|(active, _, _)| active == index)
        {
            self.overview_dirty.insert(index);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn invalidate(&mut self) {
        self.invalidated = true;
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn recolor(&mut self) {
        self.recolored = true;
        for tiles in [
            &mut self.source_tiles,
            &mut self.diff_tiles,
            &mut self.blame_tiles,
        ] {
            tiles.dirty.extend(tiles.resident.keys().copied());
        }
    }

    pub(super) fn turning(&self) -> bool {
        self.turn.is_some()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn live_reset(&self) -> Self {
        Self {
            overview_dirty: self.listing_facade.into_iter().collect(),
            ..default()
        }
    }
    pub(super) fn reset_view(&mut self) {
        self.viewing = None;
        self.destination = None;
        self.stop_water();
        self.turn = None;
        self.scroll_remaining = 0.;
    }
    pub(super) fn water_on(&self) -> bool {
        self.water_on
    }

    pub(super) fn stop_water(&mut self) {
        self.water_on = false;
        self.wash_preview = None;
    }

    pub(super) fn visible(explorer: &Explorer, navigation: &city_navigation::Navigation) -> bool {
        explorer.comparison_mode == 0 && !explorer.anchored && !navigation.guides_camera()
    }

    pub(super) fn visible_face(&self) -> Option<Face> {
        self.rendered.and(self.viewing)
    }

    /// Zero-based rows in the source wall's viewport, excluding hidden or distant walls.
    pub(super) fn visible_source_rows(
        &self,
        file: &repo_world::File,
        camera: &Transform,
        explorer: &Explorer,
        aspect: f32,
    ) -> Option<Range<usize>> {
        let count = explorer.highlighted.len();
        if count == 0
            || self.rendered != Some((explorer.selected, explorer.before, count))
            || self.viewing != Some(Face::Source)
            || explorer.comparison_mode != 0
            || explorer.anchored
        {
            return None;
        }
        let side = WallSide::new(file, count, Face::Source, block_size(file));
        side.detail_distance(camera, aspect, true)?;
        let rows = side.visible_rows(camera, count, aspect);
        (!rows.is_empty()).then_some(rows)
    }

    /// A visible row may still be covered; only revealed or actively washed rows can be read.
    pub(super) fn source_row_is_readable(&self, explorer: &Explorer, row: usize) -> bool {
        self.revealed
            .contains(&(explorer.selected, explorer.before, row + 1))
            || self.wash_preview.as_ref().is_some_and(|preview| {
                preview.file == explorer.selected
                    && preview.before == explorer.before
                    && preview.rows.contains(&row)
            })
    }

    pub(super) fn arrival_view(
        &self,
        explorer: &Explorer,
        file: &repo_world::File,
        size: Vec3,
        aim: Option<file_facade::Aim>,
        eye: Vec3,
    ) -> Transform {
        let surface = aim.and_then(|aim| aim.surface);
        let face = match surface.map(|surface| surface.normal) {
            Some(Vec3::Y | Vec3::NEG_Z) => {
                return aim.expect("surface has an aim").view(file, size, eye);
            }
            Some(Vec3::X) => Face::Diff,
            Some(Vec3::NEG_X) => Face::Blame,
            _ => Face::Source,
        };
        let count = if face == Face::Diff {
            diff_rows(explorer)
        } else {
            explorer.highlighted.len()
        };
        let side = WallSide::new(file, count, face, size);
        let y = if count == 0 {
            side.wall.y(0.5)
        } else {
            let preferred =
                surface.map_or(0, |surface| side.wall.row(size.y * surface.height, count));
            let row = if face == Face::Source && self.counts(explorer).0 < count {
                nearest_unopened(preferred, count, |row| {
                    self.revealed
                        .contains(&(explorer.selected, explorer.before, row + 1))
                })
            } else {
                0
            };
            side.wall.y(row as f32 + 0.5)
        };
        side.view(y)
    }

    pub(super) fn diff_view(explorer: &Explorer, file: &repo_world::File, row: usize) -> Transform {
        let count = diff_rows(explorer);
        let size = block_size(file);
        let side = WallSide::new(file, count, Face::Diff, size);
        let row = explorer.comparison.as_ref().map_or(0, |diff| {
            diff.rows
                .iter()
                .take(row)
                .map(diff_row_width)
                .sum::<usize>()
        });
        side.view(side.wall.y(row.min(count.saturating_sub(1)) as f32 + 0.5))
    }

    pub(super) fn review_line_view(
        explorer: &Explorer,
        file: &repo_world::File,
        line: usize,
    ) -> Option<Transform> {
        let source_row = line.checked_sub(1)?;
        let diff_count = diff_rows(explorer);
        if diff_count > 0 {
            let mut row = 0;
            for pair in &explorer.comparison.as_ref()?.rows {
                let removed = pair.before.is_some() && (pair.changed || pair.after.is_none());
                let target = if explorer.before {
                    (removed && pair.before == Some(source_row)).then_some(row)
                } else {
                    (pair.after == Some(source_row)).then_some(row + usize::from(removed))
                };
                if let Some(target) = target {
                    let side = WallSide::new(file, diff_count, Face::Diff, block_size(file));
                    return Some(side.view(side.wall.y(target as f32 + 0.5)));
                }
                row += diff_row_width(pair);
            }
        }
        let count = explorer.highlighted.len();
        if source_row >= count {
            return None;
        }
        let side = WallSide::new(file, count, Face::Source, block_size(file));
        Some(side.view(side.wall.y(source_row as f32 + 0.5)))
    }

    fn readable_hit(
        &self,
        explorer: &Explorer,
        side: &WallSide,
        camera: &Transform,
        count: usize,
    ) -> Option<Vec3> {
        if self.viewing != Some(side.face)
            || self.rendered
                != Some((
                    explorer.selected,
                    explorer.before,
                    explorer.highlighted.len(),
                ))
        {
            return None;
        }
        let hit = side.wash_hit(camera, count)?;
        let tiles = match side.face {
            Face::Source => &self.source_tiles,
            Face::Diff => &self.diff_tiles,
            Face::Blame => return None,
        };
        let tile = side.wall.row(hit.y, count) / TILE_ROWS;
        tiles
            .resident
            .get(&tile)
            .filter(|tile| tile.alpha >= 0.95)
            .map(|_| hit)
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

    fn wash_time_remaining(&self) -> Option<f32> {
        self.wash_preview
            .as_ref()
            .map(|preview| (preview.seconds - preview.elapsed).max(0.))
    }

    fn wash(
        &mut self,
        explorer: &Explorer,
        file: &repo_world::File,
        y: f32,
    ) -> Option<progress::Mark> {
        let count = explorer.highlighted.len();
        if count == 0 {
            return None;
        }
        let row = WallSide::new(file, count, Face::Source, block_size(file))
            .wall
            .row(y, count);
        let range = tile_rows(row, count);
        self.reveal(
            explorer.selected,
            explorer.before,
            file,
            range.clone(),
            count,
        )
        .then(|| progress::Mark::Reveal {
            path: file.path.clone(),
            before: explorer.before,
            start: range.start,
            end: range.end,
            rows: count,
        })
    }

    pub(super) fn reveal(
        &mut self,
        index: usize,
        before: bool,
        file: &repo_world::File,
        rows: Range<usize>,
        count: usize,
    ) -> bool {
        if rows.is_empty()
            || rows.end > count
            || rows.len() > TILE_ROWS
            || rows.start % TILE_ROWS != 0
            || count > 1024 * 1024
        {
            return false;
        }
        let progress = self
            .reveal_progress
            .entry((index, before))
            .or_insert_with(|| RevealProgress::new(count));
        if progress.rows != count || progress.opened == count {
            return false;
        }
        let mut changed = false;
        for row in rows.clone() {
            if self.revealed.insert((index, before, row + 1)) {
                progress.opened += 1;
                progress.bands[row * count.min(OVERVIEW_BANDS) / count] += 1;
                changed = true;
            }
        }
        if changed {
            self.source_tiles.preview_dirty = true;
            if before == file.after.is_none() {
                self.overview_dirty.insert(index);
            }
        }
        changed
    }

    pub(super) fn reveal_document(&mut self, index: usize, before: bool, count: usize) -> bool {
        if count == 0 || self.document_revealed(index, before, count) {
            return false;
        }
        let mut coverage = RevealProgress::new(count);
        coverage.opened = count;
        self.reveal_progress.insert((index, before), coverage);
        self.source_tiles.preview_dirty = true;
        self.overview_dirty.insert(index);
        true
    }

    fn document_revealed(&self, index: usize, before: bool, rows: usize) -> bool {
        self.reveal_progress
            .get(&(index, before))
            .is_some_and(|coverage| coverage.rows == rows && coverage.opened == rows)
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn revealed_count(&self) -> usize {
        self.reveal_progress
            .values()
            .map(|coverage| coverage.opened)
            .sum()
    }

    pub(super) fn clear_progress(&mut self) {
        self.stop_water();
        self.overview_dirty
            .extend(self.reveal_progress.keys().map(|&(index, _)| index));
        self.revealed.clear();
        self.reveal_progress.clear();
        self.source_tiles.preview_dirty = true;
    }
}

pub(super) fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    wheel: Res<AccumulatedMouseScroll>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    mut listing: ResMut<Listing>,
    mut camera: Query<&mut Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    picker: Res<file_picker::FilePicker>,
    review: Res<review_notes::Review>,
) {
    if picker.is_open() || review.blocked {
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
    let size = block_size(file);
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    let source_count = explorer.highlighted.len();
    let active = Face::at_camera(file, size, &camera, DETAIL_RAY_REACH)
        .or(listing.destination)
        .or(listing.viewing)
        .unwrap_or(Face::Source);
    let count_for = |face| {
        if face == Face::Diff {
            diff_rows(&explorer)
        } else {
            source_count
        }
    };
    let toggle = if keys.just_pressed(KeyCode::KeyG) {
        Some(Face::Diff)
    } else if keys.just_pressed(KeyCode::KeyB) {
        Some(Face::Blame)
    } else {
        None
    };
    if let Some(selected) = toggle {
        let destination = if listing.destination.unwrap_or(active) == selected {
            Face::Source
        } else {
            selected
        };
        listing.scroll_remaining = 0.;
        let old_count = count_for(active);
        let from = WallSide::new(file, old_count, active, size);
        let row = from.wall.row(from.look_height(&camera), old_count);
        let source_row = if active == Face::Diff && old_count > 0 {
            projected_row(&explorer, row, false)
        } else {
            row
        };
        let count = count_for(destination);
        let side = WallSide::new(file, count, destination, size);
        let row = if destination == Face::Diff && count > 0 {
            projected_row(&explorer, source_row, true)
        } else {
            source_row
        };
        let y = if count == 0 {
            side.wall.top * 0.5
        } else {
            side.wall.y(row as f32 + 0.5)
        };
        listing.destination = Some(destination);
        listing.turn = Some((0., *camera, side.view(y)));
    }
    if let Some((elapsed, start, end)) = listing.turn {
        let t = (elapsed + time.delta_secs() / 0.72).min(1.);
        *camera = turn_pose(file, size, start, end, t);
        listing.turn = (t < 1.).then_some((t, start, end));
        if t >= 1. {
            listing.viewing = listing.destination.take();
        }
        return;
    }
    let step = f32::from(keys.just_pressed(KeyCode::PageUp))
        - f32::from(keys.just_pressed(KeyCode::PageDown));
    let scroll = if review_notes::command_scroll(&keys) {
        0.
    } else if wheel.unit == bevy::input::mouse::MouseScrollUnit::Pixel {
        wheel.delta.y / 40.
    } else {
        wheel.delta.y
    };
    if keys.just_pressed(KeyCode::Home) {
        listing.scroll_remaining = 0.;
        let y = size.y / 2.;
        let distance = size.y.max(size.x) * 1.45 + 7.;
        let pose = wall_pose(file, y, active.angle(), distance);
        let target = pose.translation + pose.forward().as_vec3() * distance;
        *camera = Transform::from_translation(pose.translation.with_y(size.y * 1.12 + 5.))
            .looking_at(target, Vec3::Y);
        return;
    }
    if scroll != 0. || step != 0. {
        let count = count_for(active);
        if count > 0 {
            let side = WallSide::new(file, count, active, size);
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
        camera.translation.y = (camera.translation.y + step).max(CAMERA_FLOOR);
        listing.scroll_remaining -= step;
    }
}

fn turn_pose(
    file: &repo_world::File,
    size: Vec3,
    start: Transform,
    end: Transform,
    t: f32,
) -> Transform {
    let origin = Vec2::from_array(file.position);
    let from = start.translation.xz() - origin;
    let to = end.translation.xz() - origin;
    let ease = |value: f32| value * value * (3. - 2. * value);
    let radius = size.xz().length() / 2. + 0.5;
    let point = if t < 0.2 {
        from.lerp(from.normalize_or(Vec2::Y) * radius, ease(t / 0.2))
    } else if t < 0.8 {
        let angle = from.to_angle();
        let delta = (to.to_angle() - angle + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        Vec2::from_angle(angle + delta * ease((t - 0.2) / 0.6)) * radius
    } else {
        (to.normalize_or(Vec2::Y) * radius).lerp(to, ease((t - 0.8) / 0.2))
    } + origin;
    Transform::from_xyz(
        point.x,
        start.translation.y.lerp(end.translation.y, ease(t)),
        point.y,
    )
    .with_rotation(start.rotation.slerp(end.rotation, ease(t)))
}

pub(super) fn wash_stream(
    mut gizmos: Gizmos,
    time: Res<Time>,
    explorer: Res<Explorer>,
    repo: Res<Repo>,
    mut listing: ResMut<Listing>,
    navigation: Res<city_navigation::Navigation>,
    nearest: Res<Nearest>,
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    picker: Res<file_picker::FilePicker>,
    mut progress: ResMut<progress::Progress>,
    mut review: ResMut<review_notes::Review>,
    menu: Res<city_menu::Menu>,
    search: Res<find::Search>,
    settings: Res<reading::Settings>,
) {
    let Ok((camera, pilot)) = camera.single() else {
        listing.stop_water();
        return;
    };
    if !pilot.captured
        || menu.is_open()
        || search.blocks_controls()
        || review.slot != 0
        || review.blocked
        || picker.is_open()
        || listing.turning()
        || !Listing::visible(&explorer, &navigation)
    {
        listing.stop_water();
        return;
    }
    if pilot.shot && listing.water_on {
        listing.stop_water();
        return;
    }
    if !pilot.shot && !listing.water_on {
        return;
    }
    let count = explorer.highlighted.len();
    let file = &repo.0.files[explorer.selected];
    let side = WallSide::new(file, count, Face::Source, block_size(file));
    let Some(hit) = listing
        .readable_hit(&explorer, &side, camera, count)
        .filter(|_| {
            nearest
                .file
                .is_none_or(|aim| aim.index == explorer.selected)
        })
    else {
        listing.wash_preview = None;
        return;
    };
    listing.water_on = true;
    review.washing = true;
    let row = side.wall.row(hit.y, count);
    let rows = tile_rows(row, count);
    let file_index = explorer.selected;
    let before = explorer.before;
    if listing.document_revealed(file_index, before, count)
        || rows
            .clone()
            .all(|row| listing.revealed.contains(&(file_index, before, row + 1)))
    {
        listing.wash_preview = None;
    } else if settings.mode == reading::Mode::Immediate
        || WashPreview::advance(
            &mut listing.wash_preview,
            file_index,
            before,
            rows.clone(),
            time.delta_secs(),
            || {
                settings.seconds(
                    explorer.highlighted[rows]
                        .iter()
                        .map(|line| line.text.as_str()),
                )
            },
        )
    {
        if let Some(mark) = listing.wash(&explorer, file, hit.y) {
            progress.mark(mark, true);
        }
        listing.wash_preview = None;
    }
    if review.nozzle >= 0.85 {
        physics::spray_stream(
            &mut gizmos,
            camera,
            hit,
            side.wall.row_height,
            time.elapsed_secs(),
            None,
            review.nozzle,
        );
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
    material: TileMaterial,
    alpha: f32,
}

#[derive(Clone, Debug)]
enum TileMaterial {
    Plain(Handle<StandardMaterial>),
    Covered(Handle<cover::Material>),
}

struct WallSide {
    top: Vec3,
    wall: Wall,
    width: f32,
    face: Face,
    heading_height: f32,
}

impl WallSide {
    fn new(file: &repo_world::File, count: usize, face: Face, size: Vec3) -> Self {
        let [x, z] = file.position;
        let (x, z, width) = match face {
            Face::Source => (x, z + size.z / 2. + PANEL_OFFSET, size.x),
            Face::Diff => (x + size.x / 2. + PANEL_OFFSET, z, size.z),
            Face::Blame => (x - size.x / 2. - PANEL_OFFSET, z, size.z),
        };
        // Fill nominal short files; keep sparse content within the existing row-height cap.
        let row_limit =
            (texture_row_height(width) * 1.5).max(size.y / (estimated_rows(file) as f32 + 1.));
        let row_height = (size.y / (count.max(1) as f32 + 1.)).min(row_limit);
        let heading_height = row_height;
        let wall = Wall {
            top: size.y - heading_height,
            row_height,
        };
        Self {
            top: Vec3::new(x, wall.top, z),
            wall,
            width,
            face,
            heading_height,
        }
    }

    fn normal(&self) -> Vec3 {
        self.face.normal()
    }

    fn glyph_scale(&self) -> Vec2 {
        let vertical_stretch = self.wall.row_height / texture_row_height(self.width);
        Vec2::new(vertical_stretch.min(1.), vertical_stretch.recip().min(1.))
    }

    fn ray_hit(&self, camera: &Transform, facing_limit: f32, range: f32) -> Option<(Vec3, f32)> {
        let normal = self.normal();
        let facing = camera.forward().dot(normal);
        let gap = (camera.translation - self.top).dot(normal);
        if facing >= -facing_limit || gap <= 0. {
            return None;
        }
        let distance = gap / -facing;
        if distance > range {
            return None;
        }
        let point = camera.translation + camera.forward() * distance;
        let lateral = (point - self.top).dot(Vec3::Y.cross(normal)).abs();
        (lateral <= self.width / 2. && point.y >= 0. && point.y <= self.top.y + self.heading_height)
            .then_some((point, distance))
    }

    fn detail_range(&self) -> f32 {
        (self.width * 2.4).min(DETAIL_RAY_REACH)
    }

    fn detail_distance(&self, camera: &Transform, aspect: f32, retained: bool) -> Option<f32> {
        let normal = self.normal();
        let offset = camera.translation - self.top;
        let across = Vec3::Y.cross(normal);
        if offset.dot(normal) <= 0. {
            return None;
        }
        let along = offset.dot(across).clamp(-self.width / 2., self.width / 2.);
        let height = self.top.y + self.heading_height;
        let nearest = self.top.with_y(camera.translation.y.clamp(0., height)) + across * along;
        let distance = camera.translation.distance_squared(nearest);
        let range = self.detail_range().max(2.) * if retained { 1.18 } else { 1. };
        if distance > range.powi(2) {
            return None;
        }
        // The wall can fill the edge of the screen even when the reticle misses it.
        let center = self.top.with_y(height * 0.5) - camera.translation;
        let forward = camera.forward().as_vec3();
        let up = camera.up().as_vec3();
        let right = camera.right().as_vec3();
        let slope = std::f32::consts::FRAC_PI_8.tan() * if retained { 1.10 } else { 1. };
        [
            forward,
            forward * slope + up,
            forward * slope - up,
            forward * (slope * aspect) + right,
            forward * (slope * aspect) - right,
        ]
        .into_iter()
        .all(|plane| {
            center.dot(plane)
                + across.dot(plane).abs() * self.width * 0.5
                + plane.y.abs() * height * 0.5
                >= 0.
        })
        .then_some(distance)
    }

    fn visible_rows(&self, camera: &Transform, count: usize, aspect: f32) -> Range<usize> {
        let slope = std::f32::consts::FRAC_PI_8.tan();
        let forward = camera.forward().as_vec3();
        let vertical = camera.up().as_vec3() * slope;
        let horizontal = camera.right().as_vec3() * (slope * aspect);
        let normal = self.normal();
        let gap = (camera.translation - self.top).dot(normal);
        let edge = |direction: Vec3| {
            let facing = -direction.dot(normal);
            if facing > 0.001 {
                (camera.translation.y + direction.y * (gap / facing)).clamp(0., self.top.y)
            } else if direction.y > 0. {
                self.top.y
            } else {
                0.
            }
        };
        let [a, b, c, d] = [
            forward + vertical + horizontal,
            forward + vertical - horizontal,
            forward - vertical + horizontal,
            forward - vertical - horizontal,
        ]
        .map(|ray| self.wall.row(edge(ray), count));
        a.min(b).min(c).min(d)..a.max(b).max(c).max(d).saturating_add(1).min(count)
    }

    fn wash_hit(&self, camera: &Transform, count: usize) -> Option<Vec3> {
        // Judge reach by stand-off from the wall so tilting along it keeps the tool usable.
        let gap = (camera.translation - self.top).dot(self.normal());
        if gap > tool_range(self.width) {
            return None;
        }
        let (point, _) = self.ray_hit(camera, 0.12, self.detail_range())?;
        (count > 0 && (self.wall.y(count as f32)..self.wall.top).contains(&point.y))
            .then_some(point)
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
        // Keep the left gutter in view when compressed rows require a closer approach.
        let leading = self.width * (1. - self.glyph_scale().x) / 2.;
        let target = self.top.with_y(y) - Vec3::Y.cross(self.normal()) * leading;
        let eye = target + self.normal() * (self.width * 0.84 * self.glyph_scale().x).max(0.18);
        // Clear district curbs when framing a very short file near the ground.
        Transform::from_translation(eye.with_y(eye.y.max(1.))).looking_at(target, Vec3::Y)
    }

    fn window(&self, visible: Range<usize>, focus: usize, count: usize) -> Range<usize> {
        let total = count.div_ceil(TILE_ROWS);
        let visible_tiles = visible.end.div_ceil(TILE_ROWS) - visible.start / TILE_ROWS;
        let capacity = (visible_tiles + 2).clamp(TILES_PER_SIDE, MAX_VISIBLE_TILES);
        let start = (focus / TILE_ROWS)
            .saturating_sub(capacity / 2)
            .min(total.saturating_sub(capacity));
        start..(start + capacity).min(total)
    }

    fn focus_row(&self, visible: &Range<usize>, aim: usize) -> usize {
        let visible_tiles = visible.end.div_ceil(TILE_ROWS) - visible.start / TILE_ROWS;
        if visible_tiles > MAX_VISIBLE_TILES {
            return aim.clamp(
                visible.start,
                visible.end.saturating_sub(1).max(visible.start),
            );
        }
        (visible.start + visible.end) / 2
    }

    fn pose(&self, y: f32) -> Transform {
        Transform::from_translation(self.top.with_y(y))
            .with_rotation(Quat::from_rotation_y(self.face.angle()))
    }
}

impl Face {
    fn angle(self) -> f32 {
        match self {
            Self::Source => 0.,
            Self::Diff => std::f32::consts::FRAC_PI_2,
            Self::Blame => -std::f32::consts::FRAC_PI_2,
        }
    }

    pub(super) fn normal(self) -> Vec3 {
        match self {
            Self::Source => Vec3::Z,
            Self::Diff => Vec3::X,
            Self::Blame => Vec3::NEG_X,
        }
    }

    pub(super) fn at_camera(
        file: &repo_world::File,
        size: Vec3,
        camera: &Transform,
        range: f32,
    ) -> Option<Self> {
        [Self::Source, Self::Diff, Self::Blame]
            .into_iter()
            .filter_map(|face| {
                WallSide::new(file, 1, face, size)
                    .ray_hit(camera, 0.12, range)
                    .map(|(_, distance)| (face, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(face, _)| face)
    }
}

struct WallRender<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    images: &'a mut Assets<Image>,
    covered: &'a mut Assets<cover::Material>,
    remaining: usize,
    dt: f32,
}

struct TileDetails<'a> {
    mask: &'a dyn Fn(Range<usize>) -> u32,
    metadata: &'a dyn Fn(Range<usize>, u32) -> cover::CoverRows,
    paint: &'a dyn Fn(Range<usize>) -> [Vec4; TILE_ROWS],
}

impl TileDetails<'_> {
    fn build(&self, rows: Range<usize>) -> cover::CoverRows {
        let mut data = (self.metadata)(rows.clone(), (self.mask)(rows.clone()));
        data.set_paint((self.paint)(rows));
        data
    }
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
        } else if self == Self::Heading {
            // Keep the title in front of the roof rim's vertical fascia.
            pose.translation += side.normal() * 0.003;
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

    fn plane(
        &mut self,
        side: &WallSide,
        image: Image,
        y: f32,
        height: f32,
        layer: Layer,
        cover: Option<cover::CoverRows>,
        alpha: f32,
    ) -> Tile {
        let image = self.images.add(image);
        let material = StandardMaterial {
            base_color: Color::WHITE.with_alpha(alpha),
            base_color_texture: Some(image.clone()),
            alpha_mode: if alpha < 1. {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            unlit: true,
            ..default()
        };
        let mut entity = self.commands.spawn((
            ListingSurface(side.face),
            Mesh3d(self.meshes.add(Rectangle::new(side.width, height))),
            layer.pose(side, y),
        ));
        let material = if let Some(data) = cover {
            let handle = self.covered.add(cover::Material {
                base: material,
                extension: cover::Cover { data },
            });
            entity.insert(MeshMaterial3d(handle.clone()));
            TileMaterial::Covered(handle)
        } else {
            let handle = self.materials.add(material);
            entity.insert(MeshMaterial3d(handle.clone()));
            TileMaterial::Plain(handle)
        };
        let entity = entity.id();
        Tile {
            entity,
            image,
            material,
            alpha,
        }
    }

    fn header(&mut self, side: &WallSide, title: &str, tint: [f32; 3]) -> Tile {
        let image = code_texture::heading(
            title,
            tint,
            side.heading_height / side.width,
            side.glyph_scale().x,
        );
        let height = side.heading_height;
        self.plane(
            side,
            image,
            side.top.y + height / 2.,
            height,
            Layer::Heading,
            None,
            1.,
        )
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
                None,
                1.,
            );
            state.preview = Some(tile.image);
            state.preview_dirty = false;
        }
    }

    fn tiles(
        &mut self,
        side: &WallSide,
        state: &mut Tiles,
        visible: Range<usize>,
        aim: usize,
        count: usize,
        texture: impl Fn(Range<usize>) -> Image,
        details: Option<TileDetails<'_>>,
    ) {
        let focus = side.focus_row(&visible, aim);
        let visible_tiles = visible.start / TILE_ROWS..visible.end.div_ceil(TILE_ROWS);
        let window = side.window(visible, focus, count);
        let reading = !state.resident.is_empty();
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
                    let mode = if alpha >= 1. {
                        AlphaMode::Opaque
                    } else {
                        AlphaMode::Blend
                    };
                    match &tile.material {
                        TileMaterial::Plain(handle) => {
                            if let Some(mut material) = self.materials.get_mut(handle) {
                                material.base_color = Color::WHITE.with_alpha(alpha);
                                material.alpha_mode = mode;
                            }
                        }
                        TileMaterial::Covered(handle) => {
                            if let Some(mut material) = self.covered.get_mut(handle) {
                                material.base.base_color = Color::WHITE.with_alpha(alpha);
                                material.base.alpha_mode = mode;
                            }
                        }
                    }
                }
                if let (Some(details), TileMaterial::Covered(handle)) = (&details, &tile.material) {
                    let rows = tile_rows(*index * TILE_ROWS, count);
                    if let Some(material) = self.covered.get(handle) {
                        let data = material.extension.data;
                        let mut updated = data;
                        updated.set_mask((details.mask)(rows.clone()));
                        updated.set_paint((details.paint)(rows));
                        if updated != data
                            && let Some(mut material) = self.covered.get_mut(handle)
                        {
                            material.extension.data = updated;
                        }
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
        priority.sort_unstable_by_key(|index| {
            (
                !visible_tiles.contains(index),
                index.abs_diff(focus / TILE_ROWS),
                *index,
            )
        });
        for index in priority {
            if self.remaining == 0 {
                break;
            }
            let rows = tile_rows(index * TILE_ROWS, count);
            if let Some(tile) = state.resident.get(&index) {
                if !state.dirty.remove(&index) {
                    continue;
                }
                if let (Some(details), TileMaterial::Covered(handle)) = (&details, &tile.material)
                    && let Some(mut material) = self.covered.get_mut(handle)
                {
                    material.extension.data = details.build(rows.clone());
                }
                if let Some(mut image) = self.images.get_mut(&tile.image) {
                    *image = texture(rows);
                }
            } else {
                let height = rows.len() as f32 * side.wall.row_height;
                let center = side.wall.y((rows.start + rows.end) as f32 / 2.);
                let metadata = details.as_ref().map(|details| details.build(rows.clone()));
                let alpha = f32::from(reading && visible_tiles.contains(&index));
                let tile = self.plane(
                    side,
                    texture(rows),
                    center,
                    height,
                    Layer::Tile,
                    metadata,
                    alpha,
                );
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
    windows: Query<&Window, With<PrimaryWindow>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    time: Option<Res<Time>>,
    mut covered: ResMut<Assets<cover::Material>>,
    (review, focus): (
        Option<Res<review_notes::Review>>,
        Option<Res<file_facade::ChangesOnly>>,
    ),
    #[cfg(not(target_arch = "wasm32"))] live: Option<Res<live_worktree::Live>>,
) {
    let visible = Listing::visible(&explorer, &navigation)
        && repo
            .0
            .files
            .get(explorer.selected)
            .is_some_and(|file| focus.as_ref().is_none_or(|focus| focus.includes(file)));
    #[cfg(not(target_arch = "wasm32"))]
    let visible = visible
        && !live
            .as_ref()
            .zip(time.as_ref())
            .is_some_and(|(live, time)| {
                time.elapsed_secs_f64() < live.growing_until
                    && live.growth.contains_key(&explorer.selected)
            });
    let count = explorer.highlighted.len();
    let key = visible.then_some((explorer.selected, explorer.before, count));
    let changed = key != listing.rendered;
    let diff_changed = std::mem::take(&mut listing.invalidated) || changed;
    let recolored = std::mem::take(&mut listing.recolored);
    let mut render = WallRender {
        commands: &mut commands,
        meshes: &mut meshes,
        materials: &mut materials,
        images: &mut images,
        covered: &mut covered,
        remaining: TEXTURES_PER_FRAME,
        dt: time.as_ref().map_or(1. / 60., |time| time.delta_secs()),
    };
    if changed {
        listing.rendered = key;
        render.clear(&mut listing.source_tiles);
        render.clear(&mut listing.blame_tiles);
        if let Some(header) = listing.blame_header.take() {
            render.images.remove(header.image.id());
        }
    }
    if diff_changed {
        render.clear(&mut listing.diff_tiles);
        for (entity, surface) in &old {
            if changed || surface.0 == Face::Diff {
                render.commands.entity(entity).despawn();
            }
        }
    }
    if diff_changed || recolored {
        let mut source_marks = vec![Mark::Context; count];
        listing.diff_lines.clear();
        if visible
            && let Some(diff) = &explorer.comparison
            && !diff.hunks.is_empty()
        {
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
        listing.blame.clear();
        return;
    }

    let file = &repo.0.files[explorer.selected];
    let size = block_size(file);
    let source = WallSide::new(file, count, Face::Source, size);
    let diff_count = listing.diff_lines.len();
    let diff = WallSide::new(file, diff_count, Face::Diff, size);
    let blame = WallSide::new(file, count, Face::Blame, size);
    if changed {
        let title = if count == 0 {
            format!("{} / {}", file.path, explorer.notice)
        } else {
            format!("{} / {count} rows", file.path)
        };
        render.header(&source, &title, set.ink);
    }
    if diff_changed {
        let title = if diff_count > 0 {
            format!("DIFF / BASE → SNAPSHOT / {diff_count} rows")
        } else if explorer
            .comparison
            .as_ref()
            .is_some_and(|diff| diff.final_newline_changed)
        {
            "DIFF / Only the final newline changed".into()
        } else if explorer.comparison.is_some() {
            "DIFF / No changed lines between these revisions".into()
        } else {
            "DIFF / Comparison unavailable".into()
        };
        render.header(&diff, &title, set.added);
    }

    let fallback = view(file, count);
    let camera = camera.single().unwrap_or(&fallback);
    let aspect = windows
        .single()
        .map_or(16. / 9., |window| window.width() / window.height().max(1.));
    let near = |face, retained| {
        WallSide::new(file, 1, face, size).detail_distance(camera, aspect, retained)
    };
    let viewed = Face::at_camera(file, size, camera, DETAIL_RAY_REACH)
        .filter(|&face| near(face, false).is_some())
        .or_else(|| listing.viewing.filter(|&face| near(face, true).is_some()))
        .or_else(|| {
            [Face::Source, Face::Diff, Face::Blame]
                .into_iter()
                .filter_map(|face| near(face, false).map(|distance| (face, distance)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(face, _)| face)
        });
    if listing.turn.is_none() {
        listing.viewing = viewed;
    }
    let blame_row = (viewed == Some(Face::Blame) && count > 0 && listing.turn.is_none())
        .then(|| blame.wall.row(blame.look_height(camera), count));
    let attribution_updated = listing.blame.update(&repo.0, &explorer, blame_row);
    if listing.blame_header.is_none() || attribution_updated {
        let caption = if count > 0 {
            listing.blame.caption()
        } else {
            "BLAME / No readable source"
        };
        if let Some(header) = &listing.blame_header {
            if let Some(mut image) = render.images.get_mut(&header.image) {
                *image = code_texture::render(caption, &[], set.ink, 1280, 64);
            }
        } else {
            listing.blame_header = Some(render.header(&blame, caption, set.ink));
        }
        let tiles = &mut listing.blame_tiles;
        tiles.dirty.extend(tiles.resident.keys().copied());
    }
    let Listing {
        source_tiles,
        diff_tiles,
        blame_tiles,
        diff_lines,
        source_marks,
        revealed,
        reveal_progress,
        wash_preview,
        preview_marks,
        blame: attribution,
        ..
    } = &mut *listing;
    let fully_revealed = reveal_progress
        .get(&(explorer.selected, explorer.before))
        .is_some_and(|coverage| coverage.rows == count && coverage.opened == count);
    let is_revealed =
        |line| fully_revealed || revealed.contains(&(explorer.selected, explorer.before, line));
    let source_row = |index| (Some(&explorer.highlighted[index]), source_marks[index]);
    let diff_row = |index: usize| (diff_lines[index].0.as_ref(), diff_lines[index].1);
    let retain = |face, tiles: &Tiles| {
        viewed == Some(face) || (!tiles.resident.is_empty() && near(face, true).is_some())
    };
    let show_source = retain(Face::Source, source_tiles);
    let show_diff = retain(Face::Diff, diff_tiles);
    let show_blame = retain(Face::Blame, blame_tiles);
    if !show_source {
        render.hide_side(source_tiles);
    }
    if !show_diff {
        render.hide_side(diff_tiles);
    }
    if !show_blame {
        render.hide_side(blame_tiles);
    }

    let source_rows = |rows: Range<usize>| {
        rows.map(|index| {
            (
                Some(explorer.highlighted[index].clone()),
                source_marks[index],
            )
        })
        .collect::<Vec<_>>()
    };
    let active_wash = wash_preview
        .as_ref()
        .filter(|preview| preview.file == explorer.selected && preview.before == explorer.before);
    let mask = |rows| {
        cover::mask(rows, &|line| {
            is_revealed(line)
                || active_wash.is_some_and(|preview| preview.rows.contains(&(line - 1)))
        })
    };
    let metadata =
        |rows, bits| cover::CoverRows::new(&source_rows(rows), bits, source.glyph_scale());
    let tone = |[r, g, b]: [u8; 3], alpha| {
        Vec4::new(r as f32 / 255., g as f32 / 255., b as f32 / 255., alpha)
    };
    let mut marks = Vec::new();
    if let Some(review) = review.as_ref() {
        marks.extend(
            review
                .notes
                .iter()
                .filter(|note| note.anchor.path == file.path && review.shows_paint(note, &repo.0))
                .map(|note| {
                    (
                        &note.anchor,
                        tone(
                            note.ink(),
                            if note.resolved == Some(true) {
                                0.38
                            } else {
                                1.
                            },
                        ),
                    )
                }),
        );
    }
    if preview_marks.len() != marks.len()
        || preview_marks
            .iter()
            .zip(&marks)
            .any(|((old, color), (anchor, next))| old != *anchor || color != next)
    {
        *preview_marks = marks
            .iter()
            .map(|(anchor, color)| ((*anchor).clone(), *color))
            .collect();
        source_tiles.preview_dirty = true;
        diff_tiles.preview_dirty = true;
    }
    let line_paint = |before, line| {
        preview_marks
            .iter()
            .rev()
            .find(|(anchor, _)| {
                anchor.before == before && (anchor.start..=anchor.end).contains(&line)
            })
            .map(|(_, paint)| *paint)
    };
    render.preview(&source, source_tiles, count, || {
        code_texture::listing_preview(
            count,
            source_row,
            is_revealed,
            Face::Source,
            source.glyph_scale(),
            |index| line_paint(explorer.before, index + 1),
        )
    });
    render.preview(&diff, diff_tiles, diff_count, || {
        code_texture::listing_preview(
            diff_count,
            diff_row,
            |_| true,
            Face::Diff,
            diff.glyph_scale(),
            |index| {
                let (line, mark) = &diff_lines[index];
                line.as_ref()
                    .and_then(|line| line_paint(*mark == Mark::Removed, line.number))
            },
        )
    });
    if let Some(review) = review.as_ref() {
        if let (Some((_, anchor)), Some(preset)) = (&review.drag, review.paint())
            && anchor.path == file.path
        {
            marks.push((anchor, tone(preset.color, 0.58)));
        }
    }
    let source_paint =
        |rows| paint::pigments(rows, |index| Some((explorer.before, index + 1)), &marks);
    let diff_paint = |rows| {
        paint::pigments(
            rows,
            |index| {
                let (line, mark) = &diff_lines[index];
                line.as_ref()
                    .map(|line| (*mark == Mark::Removed, line.number))
            },
            &marks,
        )
    };
    for face in viewed.into_iter().chain(
        [Face::Source, Face::Diff, Face::Blame]
            .into_iter()
            .filter(|face| Some(*face) != viewed),
    ) {
        match face {
            Face::Source if show_source => render.tiles(
                &source,
                source_tiles,
                source.visible_rows(camera, count, aspect),
                source.wall.row(source.look_height(camera), count),
                count,
                |rows| {
                    code_texture::listing(&source_rows(rows), Face::Source, source.glyph_scale())
                },
                Some(TileDetails {
                    mask: &mask,
                    metadata: &metadata,
                    paint: &source_paint,
                }),
            ),
            Face::Diff if show_diff => render.tiles(
                &diff,
                diff_tiles,
                diff.visible_rows(camera, diff_count, aspect),
                diff.wall.row(diff.look_height(camera), diff_count),
                diff_count,
                |rows| code_texture::listing(&diff_lines[rows], Face::Diff, diff.glyph_scale()),
                Some(TileDetails {
                    mask: &|_| u32::MAX,
                    metadata: &|rows, bits| {
                        cover::CoverRows::new(&diff_lines[rows], bits, diff.glyph_scale())
                    },
                    paint: &diff_paint,
                }),
            ),
            Face::Blame if show_blame => render.tiles(
                &blame,
                blame_tiles,
                blame.visible_rows(camera, count, aspect),
                blame.wall.row(blame.look_height(camera), count),
                count,
                |rows| {
                    code_texture::listing_with_notes(
                        &source_rows(rows),
                        Face::Blame,
                        blame.glyph_scale(),
                        attribution.notes(),
                    )
                },
                None,
            ),
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn smoke_side(explorer: &Explorer, file: &repo_world::File, blame: bool) -> Transform {
    let face = if blame { Face::Blame } else { Face::Diff };
    let count = if blame {
        explorer.highlighted.len()
    } else {
        diff_rows(explorer)
    };
    let side = WallSide::new(file, count, face, block_size(file));
    let focus = if count == 0 {
        side.wall.top * 0.5
    } else {
        let row = if blame {
            18
        } else {
            explorer
                .comparison
                .as_ref()
                .and_then(|diff| diff.hunks.first())
                .map_or(0, |hunk| projected_row(explorer, hunk.after.start, true))
        };
        side.wall.y(row.clamp(0, count - 1) as f32 + 0.5)
    };
    side.view(focus)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn smoke_source_ready(listing: &Listing) -> bool {
    !listing.source_tiles.resident.is_empty()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn smoke_blame_ready(listing: &Listing, count: usize) -> bool {
    listing
        .blame
        .notes()
        .is_some_and(|notes| notes.len() == count)
        && !listing.blame_tiles.resident.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_line_file() -> (repo_world::File, Explorer) {
        let source = "// Math helpers\nexport function add(a, b) {\n  return a + b;\n}\n";
        let mut file = fixture_file();
        file.path = "math.mjs".into();
        file.after = Some(repo_world::Blob {
            id: String::new(),
            size: source.len(),
            mode: "100644".into(),
        });
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight(&file.path, source);
        (file, explorer)
    }

    #[test]
    fn four_line_source_reserves_a_single_row_for_its_heading() {
        let (file, explorer) = four_line_file();
        let side = WallSide::new(
            &file,
            explorer.highlighted.len(),
            Face::Source,
            block_size(&file),
        );
        assert!(
            side.heading_height <= side.wall.row_height * 1.01,
            "the heading must not consume more height than a source row"
        );
    }

    #[test]
    fn source_heading_sits_in_front_of_the_roof_rim() {
        let (file, explorer) = four_line_file();
        let size = block_size(&file);
        let rim = file_facade::transform(&file, size, file_facade::Part::Roof);
        for (face, extent) in [
            (Face::Source, rim.scale.z),
            (Face::Diff, rim.scale.x),
            (Face::Blame, rim.scale.x),
        ] {
            let side = WallSide::new(&file, explorer.highlighted.len(), face, size);
            let heading = Layer::Heading.pose(&side, side.top.y + side.heading_height / 2.);
            let heading_depth = (heading.translation - rim.translation).dot(side.normal());
            assert!(
                heading_depth > extent / 2. + 0.001,
                "the roof fascia must not cover a readable heading on {face:?}"
            );
        }
    }

    #[test]
    fn four_line_approach_keeps_body_glyphs_readable_when_metadata_underestimates_rows() {
        let (file, explorer) = four_line_file();
        let size = block_size(&file);
        let side = WallSide::new(&file, explorer.highlighted.len(), Face::Source, size);
        let camera =
            Listing::default().arrival_view(&explorer, &file, size, None, Vec3::new(0., 5., 30.));
        let nominal_glyph =
            side.width * side.glyph_scale().x * 20. / code_texture::LISTING_WIDTH as f32;
        let point = side.top.with_y(side.wall.y(0.5));
        let screen_y = |point: Vec3| {
            let local = camera.compute_affine().inverse().transform_point3(point);
            local.y / -local.z / std::f32::consts::FRAC_PI_8.tan() * 1080. / 2.
        };
        let pixels = (screen_y(point + Vec3::Y * nominal_glyph / 2.)
            - screen_y(point - Vec3::Y * nominal_glyph / 2.))
        .abs();
        assert!(
            pixels >= 18.,
            "approached source glyph is only {pixels:.1} screen pixels tall"
        );
        let across = Vec3::Y.cross(side.normal());
        let heading_start = side.top
            + Vec3::Y * side.heading_height / 2.
            + across
                * (-side.width / 2.
                    + side.width * code_texture::HEADING_LEFT / code_texture::LISTING_WIDTH as f32);
        let local = camera
            .compute_affine()
            .inverse()
            .transform_point3(heading_start);
        let heading_x = 864. + local.x / -local.z / std::f32::consts::FRAC_PI_8.tan() * 1080. / 2.;
        assert!(
            heading_x >= 8.,
            "approach clips the header at {heading_x:.1}px"
        );
    }

    #[test]
    fn visible_source_context_excludes_hidden_faces_and_unwashed_rows() {
        let file = fixture_with_rows(40);
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(40));
        let camera = view(&file, 40);
        let mut listing = Listing::default();
        assert_eq!(
            listing.visible_source_rows(&file, &camera, &explorer, 1.6),
            None
        );
        listing.rendered = Some((0, false, 40));
        listing.viewing = Some(Face::Source);
        let rows = listing
            .visible_source_rows(&file, &camera, &explorer, 1.6)
            .unwrap();
        assert!(!rows.is_empty() && rows.end <= 40);
        assert!(!listing.source_row_is_readable(&explorer, rows.start));
        listing.revealed.insert((0, false, rows.start + 1));
        assert!(listing.source_row_is_readable(&explorer, rows.start));
        listing.viewing = Some(Face::Diff);
        assert_eq!(
            listing.visible_source_rows(&file, &camera, &explorer, 1.6),
            None
        );
        listing.viewing = Some(Face::Source);
        listing.rendered = None;
        assert_eq!(
            listing.visible_source_rows(&file, &camera, &explorer, 1.6),
            None
        );
    }

    #[test]
    fn filtering_out_the_last_file_removes_its_detailed_walls_and_textures() {
        let mut file = fixture_with_rows(40);
        file.change = Change::Unchanged;
        let mut repo = repository_picker::empty();
        repo.files.push(file);
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(40));
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<cover::Material>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<Listing>()
            .add_systems(Update, draw);
        app.update();
        assert!(app.world().resource::<Listing>().rendered.is_some());
        assert!(
            app.world_mut()
                .query::<&ListingSurface>()
                .iter(app.world())
                .next()
                .is_some()
        );
        app.world_mut()
            .resource_mut::<file_facade::ChangesOnly>()
            .enabled = true;
        app.update();
        assert!(app.world().resource::<Listing>().rendered.is_none());
        assert_eq!(
            app.world_mut()
                .query::<&ListingSurface>()
                .iter(app.world())
                .count(),
            0
        );
        assert!(
            app.world()
                .resource::<Listing>()
                .source_tiles
                .resident
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<Listing>()
                .source_tiles
                .preview
                .is_none()
        );
    }

    #[test]
    fn source_arrival_keeps_the_original_building_dimensions_when_rows_load() {
        let mut file = fixture_file();
        file.after = Some(repo_world::Blob {
            id: String::new(),
            size: 68_000,
            mode: "100644".into(),
        });
        let unloaded = view(&file, 0);
        let loaded = view(&file, 180);
        assert_eq!(unloaded.translation.xz(), loaded.translation.xz());
    }

    fn fixture_file() -> repo_world::File {
        repo_world::File {
            path: "test.rs".into(),
            before: None,
            after: None,
            change: Change::Added,
            position: [0., 0.],
        }
    }

    fn fixture_with_rows(rows: usize) -> repo_world::File {
        let mut file = fixture_file();
        file.before = Some(repo_world::Blob {
            id: String::new(),
            size: rows * 40,
            mode: "100644".into(),
        });
        file
    }

    #[test]
    fn glyphs_keep_equal_world_scale_with_mismatched_row_counts() {
        let file = fixture_with_rows(80);
        let size = block_size(&file);
        for face in [Face::Source, Face::Diff, Face::Blame] {
            for count in [32, 80, 190] {
                let side = WallSide::new(&file, count, face, size);
                let scale = side.glyph_scale();
                let horizontal = side.width * scale.x / code_texture::LISTING_WIDTH as f32;
                let vertical =
                    side.wall.row_height * scale.y / code_texture::LISTING_ROW_PIXELS as f32;
                assert!((horizontal - vertical).abs() < 0.00001);
            }
        }
    }

    #[test]
    fn sparse_source_rows_fit_at_the_top_without_resizing_the_building() {
        let file = fixture_with_rows(300);
        let size = block_size(&file);
        let count = 25;
        let side = WallSide::new(&file, count, Face::Source, size);
        assert!((side.wall.top + side.heading_height - size.y).abs() < 0.00001);
        assert_eq!(side.wall.row_height, texture_row_height(side.width) * 1.5);
        let below_code = side.wall.y(count as f32) - 1.;
        let target = side.top.with_y(below_code);
        let camera = Transform::from_translation(target + Vec3::Z * 2.).looking_at(target, Vec3::Y);
        assert!(side.wash_hit(&camera, count).is_none());
    }

    #[test]
    fn wider_reading_window_covers_visible_rows_and_preloads_around_them() {
        let file = fixture_with_rows(724);
        let side = WallSide::new(&file, 724, Face::Source, block_size(&file));
        let before = side.window(120..125, 122, 724);
        let after = side.window(221..352, 286, 724);
        assert_eq!(before.len(), TILES_PER_SIDE);
        assert_eq!(after.len(), MAX_VISIBLE_TILES);
        assert!(after.contains(&(221 / TILE_ROWS)));
        assert!(after.contains(&(351 / TILE_ROWS)));
        let visible = 0..724;
        let glancing = side.window(visible.clone(), side.focus_row(&visible, 60), 724);
        assert!(glancing.contains(&(60 / TILE_ROWS)));
    }

    #[test]
    fn review_line_centers_github_line_numbers_on_the_correct_side() {
        let file = fixture_with_rows(32);
        let mut explorer = Explorer::new(0);
        let prefix = "let start = 1;\n";
        let suffix = "let end = 2;\n".repeat(28);
        let before = format!("{prefix}let old = 1;\n{suffix}");
        let after = format!("{prefix}let new = 1;\n{suffix}");
        explorer.comparison = Some(Comparison::new(&file.path, &before, &after).unwrap());
        let count = diff_rows(&explorer);
        let side = WallSide::new(&file, count, Face::Diff, block_size(&file));
        for (before, expected) in [(false, 2), (true, 1)] {
            explorer.before = before;
            let camera = Listing::review_line_view(&explorer, &file, 2).unwrap();
            let hit = side.ray_hit(&camera, 0.12, side.detail_range()).unwrap().0;
            assert_eq!(side.wall.row(hit.y, count), expected);
        }
    }

    #[test]
    fn readable_tiles_and_previews_keep_the_same_rows_on_distinct_depths() {
        let file = fixture_file();
        for face in [Face::Source, Face::Diff, Face::Blame] {
            let side = WallSide::new(&file, 724, face, size_from_rows(724));
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
    fn reading_delay_tracks_content_and_accepts_an_explicit_override() {
        let short = ["fn f() {}", ""];
        let dense = ["fn f() { call(argument, other_argument); }", ""];
        let estimate = reading::Settings {
            min_seconds: None,
            max_seconds: None,
            ..default()
        };
        assert!(estimate.seconds(short) > 0.);
        assert!(estimate.seconds(short) < estimate.seconds(dense));
        assert_eq!(
            estimate.seconds(short),
            estimate.seconds(["fn f() {}", "  ", "}"])
        );
        let instant = reading::Settings {
            mode: reading::Mode::Immediate,
            ..default()
        };
        assert_eq!(instant.seconds(dense), 0.);
    }

    #[test]
    fn washing_only_saves_a_tile_after_continuous_focus() {
        let mut preview = None;
        assert!(!WashPreview::advance(
            &mut preview,
            0,
            false,
            0..16,
            4.,
            || 6.
        ));
        assert!(!WashPreview::advance(
            &mut preview,
            0,
            false,
            16..17,
            2.,
            || 3.
        ));
        assert!(!WashPreview::advance(
            &mut preview,
            0,
            false,
            0..16,
            3.,
            || 6.
        ));
        assert!(WashPreview::advance(
            &mut preview,
            0,
            false,
            0..16,
            3.,
            || panic!("recounted tile")
        ));
        let mut listing = Listing {
            water_on: true,
            wash_preview: preview,
            ..default()
        };
        listing.stop_water();
        assert!(listing.wash_preview.is_none());
        assert!(!listing.water_on());
    }

    #[test]
    fn wash_can_only_reach_readable_source_under_the_reticle() {
        let file = fixture_file();
        for count in [1, 23, 329, 1394] {
            let size = size_from_rows(count);
            let source = WallSide::new(&file, count, Face::Source, size);
            let row = 12.min(count - 1);
            let y = source.wall.y(row as f32 + 0.5);
            assert!(y > CAMERA_FLOOR);
            let close = source.view(y);
            let hit = source.wash_hit(&close, count).expect("arrival is in range");
            assert_eq!(source.wall.row(hit.y, count), row);
            let tilted = Transform::from_translation(close.translation + Vec3::Y * source.width)
                .looking_at(hit, Vec3::Y);
            let tilted_hit = source
                .wash_hit(&tilted, count)
                .expect("near wall at an angle");
            assert_eq!(source.wall.row(tilted_hit.y, count), row);
            let far = close.with_translation(close.translation + Vec3::Z * source.width);
            assert!(source.wash_hit(&far, count).is_none());
            let from_side = WallSide::new(&file, count, Face::Diff, size).view(y);
            assert!(source.wash_hit(&from_side, count).is_none());
            let at_roof = Transform::from_translation(close.translation.with_y(size.y - 0.01));
            assert!(source.wash_hit(&at_roof, count).is_none());
            assert!(source.wash_hit(&close, 0).is_none());
        }
    }

    #[test]
    fn a_close_glancing_shot_cannot_reach_a_distant_row() {
        let file = fixture_file();
        let count = 700;
        let size = size_from_rows(count);
        let source = WallSide::new(&file, count, Face::Source, size);
        let hit = source.top.with_y(4.);
        let eye = hit + Vec3::Y * 32. + Vec3::Z * (size.x * 0.84);
        let camera = Transform::from_translation(eye).looking_at(hit, Vec3::Y);
        assert!(source.ray_hit(&camera, 0.12, f32::INFINITY).is_some());
        assert!(Face::at_camera(&file, size, &camera, DETAIL_RAY_REACH).is_none());
        assert!(source.wash_hit(&camera, count).is_none());
    }

    #[test]
    fn visible_rows_keep_detail_when_the_reticle_slips_past_the_wall() {
        let file = fixture_file();
        let count = 300;
        let size = size_from_rows(count);
        let source = WallSide::new(&file, count, Face::Source, size);
        let bottom = source.top.with_y(0.);
        let eye = bottom + Vec3::Z * (size.x * 0.8) + Vec3::Y * 6.;
        let camera = Transform::from_translation(eye).looking_at(bottom - Vec3::Y, Vec3::Y);
        assert!(
            source
                .ray_hit(&camera, 0.12, source.detail_range())
                .is_none()
        );
        assert!(source.detail_distance(&camera, 16. / 9., false).is_some());
        assert!(!source.visible_rows(&camera, count, 16. / 9.).is_empty());
        assert!(source.wash_hit(&camera, count).is_none());
        assert!(
            source
                .detail_distance(
                    &camera.with_translation(eye + Vec3::Z * 32.),
                    16. / 9.,
                    true
                )
                .is_none()
        );
    }

    #[test]
    fn overview_marks_coverage_only_at_a_distance_and_keeps_progress() {
        use code_texture::Coverage;
        let count = 80;
        let file = fixture_with_rows(count);
        let wall = WallSide::new(&file, count, Face::Source, size_from_rows(count)).wall;
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let mut explorer = Explorer::new(0);
        explorer.before = true;
        explorer.highlighted = syntax::highlight(&file.path, &"let x = 1;\n".repeat(count));
        let mut images = Assets::default();
        let mut materials = Assets::default();
        let neutral = materials.add(StandardMaterial::default());
        let rims = Coverage::ALL.map(|_| materials.add(StandardMaterial::default()));
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
                facade_rims: rims.clone(),
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
        let rim = app
            .world_mut()
            .spawn((FacadeRim(0), MeshMaterial3d(rims[0].clone())))
            .id();
        let rim_material = |app: &App| {
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(rim)
                .unwrap()
                .0
                .clone()
        };
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
        assert_eq!(rim_material(&app), rims[1]);
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
        {
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            let images = app.world().resource::<Assets<Image>>();
            let rail = |material: &Handle<StandardMaterial>, band: usize| {
                let texture = materials
                    .get(material)
                    .unwrap()
                    .base_color_texture
                    .as_ref()
                    .unwrap();
                let image = images.get(texture).unwrap();
                let width = image.width() as usize;
                let y = (2 * band + 1) * image.height() as usize / (2 * bands.len());
                let start = (y * width + width * 91 / code_texture::LISTING_WIDTH as usize) * 4;
                let pixels = image.data.as_ref().unwrap();
                [pixels[start], pixels[start + 1], pixels[start + 2]]
            };
            let [r, g, b] = rail(&closed, 13);
            assert!(b > 120 && b > r && b > g, "unread rail should read as blue");
            let [r, g, b] = rail(&open, 0);
            assert!(
                g > 150 && g > r + 35 && b > r + 35,
                "read rail should read as cyan"
            );
            assert_eq!(rail(&partial, 0), rail(&open, 0));
            assert_eq!(rail(&partial, 13), rail(&closed, 13));
            let [r, g, b] = rail(&partial, 12);
            assert!(r > g && b > g, "a mixed band should read as violet");
        }
        wash(&mut app, 17.);
        app.update();
        assert_eq!(current(&app), partial);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 3);
        for row in (TILE_ROWS * 2..count).step_by(TILE_ROWS) {
            wash(&mut app, row as f32 + 0.5);
        }
        app.update();
        assert_eq!(current(&app), open);
        assert_eq!(rim_material(&app), rims[2]);
        app.world_mut().resource_mut::<Listing>().rendered = Some((0, true, count));
        app.update();
        assert_eq!(current(&app), neutral);
        app.world_mut().resource_mut::<Listing>().rendered = None;
        app.update();
        assert_eq!(current(&app), open);
    }

    #[test]
    fn repeated_hits_keep_the_source_texture_and_update_its_mask() {
        let count = TILE_ROWS * 12;
        let file = fixture_with_rows(count);
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
            .init_resource::<Assets<cover::Material>>()
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
        let wall = WallSide::new(&file, count, Face::Source, size_from_rows(count)).wall;
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
        assert_eq!(modified, [preview.id()]);
        let TileMaterial::Covered(material) = &handles[&1].material else {
            panic!("source tile must use the shader mask");
        };
        assert_eq!(
            app.world()
                .resource::<Assets<cover::Material>>()
                .get(material)
                .unwrap()
                .extension
                .data
                .mask(),
            u16::MAX as u32
        );
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
        let count = 1614;
        let file = fixture_with_rows(count);
        let mut repo = repository_picker::empty();
        repo.files.push(file.clone());
        let source = "let x = 1;\n".repeat(count);
        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight(&file.path, &source);
        let before = source.replacen("let x = 1;", "let x = 0;", 1);
        explorer.comparison = Some(Comparison::new(&file.path, &before, &source).unwrap());
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<cover::Material>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<Listing>()
            .add_systems(Update, draw);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), view(&file, count)))
            .id();
        app.update();
        assert!(app.world().resource::<Assets<Image>>().len() <= 7);
        let limit = 5 + TILES_PER_SIDE + RETIRING_TILES;
        let source = WallSide::new(&file, count, Face::Source, size_from_rows(count));
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
        let diff = WallSide::new(&file, count + 1, Face::Diff, size_from_rows(count));
        // Walk around to the diff face without pressing G.
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = diff.view(diff.wall.y(42.));
        app.update();
        let listing = app.world().resource::<Listing>();
        assert!(listing.source_tiles.resident.is_empty());
        assert!(!listing.diff_tiles.resident.is_empty());
        assert_eq!(listing.source_tiles.preview, preview);
    }

    #[test]
    fn source_and_block_share_both_edges_and_the_last_row() {
        let file = fixture_file();
        let short = size_from_rows(23);
        let medium = size_from_rows(329);
        let tall = size_from_rows(724);
        assert!(short.y < 8. && tall.x < medium.x && tall.y <= MAX_HEIGHT);
        for rows in [1, 23, 329, 724, 1614] {
            let size = size_from_rows(rows);
            let side = WallSide::new(&file, rows, Face::Source, size);
            assert!((side.width - size.x).abs() < 0.001);
            assert!(side.wall.y(rows as f32).abs() < 0.001);
        }
    }

    // Given a long comparison, walking around the right corner shows sharp diff rows.
    // Both listing edges and the title stay on the physical side of the building.
    #[test]
    fn long_diff_fills_its_wall_even_when_it_has_more_rows_than_source() {
        let file = fixture_file();
        let size = size_from_rows(1394);
        let side = WallSide::new(&file, 1620, Face::Diff, size);
        assert!((side.width - size.z).abs() < 0.001);
        assert!(side.wall.y(1620.).abs() < 0.001);
        assert!((side.wall.y(0.) + side.heading_height - size.y).abs() < 0.001);
    }

    #[test]
    fn arrival_prefers_the_nearest_closed_row_and_falls_back_to_the_top() {
        assert_eq!(nearest_unopened(15, 40, |row| row < 32), 32);
        assert_eq!(nearest_unopened(20, 40, |row| (16..32).contains(&row)), 15);
        assert_eq!(nearest_unopened(20, 40, |_| true), 0);
    }

    #[test]
    fn scroll_stops_at_first_and_last_strip() {
        let count = 47;
        let file = fixture_with_rows(count);
        let wall = WallSide::new(&file, count, Face::Source, size_from_rows(count)).wall;
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
            .init_resource::<review_notes::Review>()
            .add_systems(Update, controls);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let start = Transform::from_xyz(3., 13., 30.)
            .looking_at(Vec3::new(1., 10., WIDTH / 2. + PANEL_OFFSET), Vec3::Y);
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
