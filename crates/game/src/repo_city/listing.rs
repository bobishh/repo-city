//! In-place source reveal for the city facade. Progress is keyed by snapshot and file.
use avian3d::prelude::{AngularVelocity, CollisionStart, LinearVelocity, Position};
use repo_world::comparison::Mark;

use super::*;

const TILE_ROWS: usize = 16;
const ROW_HEIGHT: f32 = 0.28;
pub(super) const WIDTH: f32 = 14.;
const BASE: f32 = 2.;
const FRONT: f32 = 4.18;
const CRACK_SECONDS: f32 = 0.45;

pub(super) fn top(rows: usize) -> f32 {
    BASE + rows as f32 * ROW_HEIGHT
}

pub(super) fn view(file: &repo_world::File, rows: usize) -> Transform {
    let [x, z] = file.position;
    let y = (top(rows) - 2.2).max(BASE + 0.4);
    Transform::from_xyz(x, y, z + 17.).looking_at(Vec3::new(x, y, z + FRONT), Vec3::Y)
}

fn reading_height(y: f32, count: usize, delta: f32) -> f32 {
    let first = top(count) - (count.min(TILE_ROWS) as f32 * ROW_HEIGHT / 2.);
    let last = BASE + (count.min(TILE_ROWS) as f32 * ROW_HEIGHT / 2.);
    (y + delta).clamp(last, first.max(last))
}

#[derive(Resource, Default)]
pub(super) struct Listing {
    revealed: HashSet<(usize, bool, usize)>,
    diff_side: bool,
    turn: Option<(f32, f32, f32)>,
    rendered: Option<(usize, bool, usize, usize)>,
    revision: usize,
}

#[derive(Component)]
pub(super) struct ListingSurface;

#[derive(Component)]
pub(super) struct CoverShot {
    pub file: usize,
    pub before: bool,
}

#[derive(Component)]
pub(super) struct CoverCollider;

#[derive(Component)]
pub(super) struct CoverStrike {
    file: usize,
    before: bool,
    rows: std::ops::Range<usize>,
    point: Vec3,
    top: Vec3,
    elapsed: f32,
}

impl CoverStrike {
    fn reveal_when_ready(&self, listing: &mut Listing) -> usize {
        if self.elapsed < CRACK_SECONDS {
            return 0;
        }
        listing.reveal(self.file, self.before, self.rows.clone().map(|row| row + 1))
    }
}

fn tile_rows(row: usize, total: usize) -> std::ops::Range<usize> {
    let first = row / TILE_ROWS * TILE_ROWS;
    first..(first + TILE_ROWS).min(total)
}

fn hit_row(origin: Vec3, direction: Vec3, center: Vec3, rows: usize) -> Option<usize> {
    if direction.z >= -0.0001 {
        return None;
    }
    let distance = (center.z - origin.z) / direction.z;
    let impact = origin + direction * distance;
    let row = (center.y - impact.y) / ROW_HEIGHT;
    (distance > 0. && (impact.x - center.x).abs() < WIDTH / 2. && row >= 0. && row < rows as f32)
        .then_some(row as usize)
}

fn rows(explorer: &Explorer) -> Vec<(Option<syntax::Line>, Mark)> {
    explorer
        .highlighted
        .iter()
        .cloned()
        .map(|line| (Some(line), Mark::Context))
        .collect()
}

const SIDE_DEPTH: f32 = 14.;
const SIDE_CENTER: f32 = FRONT - SIDE_DEPTH / 2. - 0.05;

fn wall_pose(file: &repo_world::File, y: f32, angle: f32, distance: f32) -> Transform {
    let [x, z] = file.position;
    let center = Vec3::new(x, y, z + SIDE_CENTER);
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
        self.rendered = None;
    }

    pub(super) fn turning(&self) -> bool {
        self.turn.is_some()
    }
    pub(super) fn on_diff(&self) -> bool {
        self.diff_side
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
        let visible = (1..=total)
            .filter(|line| {
                self.revealed
                    .contains(&(explorer.selected, explorer.before, *line))
            })
            .count();
        (visible, total)
    }

    fn reveal(&mut self, file: usize, before: bool, lines: impl Iterator<Item = usize>) -> usize {
        let added = lines
            .filter(|line| self.revealed.insert((file, before, *line)))
            .count();
        if added > 0 {
            self.revision += 1;
        }
        added
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
            let row = ((top(old_count) - camera.translation.y) / ROW_HEIGHT)
                .clamp(0., old_count.saturating_sub(1) as f32) as usize;
            listing.diff_side = !listing.diff_side;
            let count = if listing.diff_side {
                diff_rows(&explorer)
            } else {
                explorer.highlighted.len()
            };
            let target_row = projected_row(&explorer, row, listing.diff_side);
            let y = top(count) - (target_row as f32 + 0.5) * ROW_HEIGHT;
            listing.turn = Some((0., camera.translation.y, y));
        }
    }
    if let Some((elapsed, start, end)) = listing.turn {
        let t = (elapsed + time.delta_secs() / 0.55).min(1.);
        let eased = t * t * (3. - 2. * t);
        let angle =
            if listing.diff_side { eased } else { 1. - eased } * std::f32::consts::FRAC_PI_2;
        *camera = wall_pose(file, start.lerp(end, eased), angle, 20.);
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
    if (scroll != 0. || step != 0.) && count > 0 {
        let y = reading_height(
            camera.translation.y,
            count,
            (scroll * ROW_HEIGHT * 4. + step * ROW_HEIGHT * TILE_ROWS as f32).clamp(-20., 20.),
        );
        *camera = wall_pose(file, y, angle, 20.);
    }
    if keys.just_pressed(KeyCode::Home) {
        let y = (top(count) + BASE) / 2.;
        let distance = (top(count) - BASE + 2.).max(WIDTH * 0.85) * 1.45 + 7.;
        *camera = wall_pose(file, y, angle, distance);
    }
}

pub(super) fn cover_impacts(
    mut commands: Commands,
    mut collisions: MessageReader<CollisionStart>,
    mut shots: Query<(&mut physics::Projectile, &Position, &CoverShot)>,
    covers: Query<Entity, With<CoverCollider>>,
    strikes: Query<&CoverStrike>,
    explorer: Res<Explorer>,
    repo: Res<Repo>,
    listing: Res<Listing>,
    navigation: Res<city_navigation::Navigation>,
) {
    let mut new_strikes = HashSet::new();
    for event in collisions.read() {
        let shot = if covers.contains(event.collider1) {
            event.collider2
        } else if covers.contains(event.collider2) {
            event.collider1
        } else {
            continue;
        };
        let Ok((mut projectile, position, target)) = shots.get_mut(shot) else {
            continue;
        };
        if projectile.used {
            continue;
        }
        projectile.used = true;
        commands.entity(shot).despawn();
        if !Listing::visible(&explorer, &navigation)
            || target.file != explorer.selected
            || target.before != explorer.before
        {
            continue;
        }
        let count = explorer.highlighted.len();
        let [x, z] = repo.0.files[explorer.selected].position;
        let top = Vec3::new(x, top(count), z + FRONT + 0.025);
        let Some(row) = hit_row(
            Vec3::new(position.x, position.y, top.z + 1.),
            Vec3::NEG_Z,
            top,
            count,
        ) else {
            continue;
        };
        let rows = tile_rows(row, count);
        if !new_strikes.insert(rows.start)
            || listing
                .revealed
                .contains(&(explorer.selected, explorer.before, rows.start + 1))
            || strikes.iter().any(|strike| {
                strike.file == explorer.selected
                    && strike.before == explorer.before
                    && strike.rows.start == rows.start
            })
        {
            continue;
        }
        commands.spawn((
            CoverStrike {
                file: explorer.selected,
                before: explorer.before,
                rows,
                point: Vec3::new(position.x, position.y, top.z),
                top,
                elapsed: 0.,
            },
            physics::Lifetime(3.),
        ));
    }
}

pub(super) fn cover_breaks(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut listing: ResMut<Listing>,
    mut strikes: Query<(Entity, &mut CoverStrike)>,
    mut gizmos: Gizmos,
    palette: Res<Palette>,
    mut explorer: ResMut<Explorer>,
    navigation: Res<city_navigation::Navigation>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    for (entity, mut strike) in &mut strikes {
        strike.elapsed += time.delta_secs();
        let visible = Listing::visible(&explorer, &navigation)
            && strike.file == explorer.selected
            && strike.before == explorer.before;
        if strike.elapsed < CRACK_SECONDS {
            if visible {
                draw_cracks(&strike, &mut gizmos);
            }
            continue;
        }
        commands.entity(entity).despawn();
        if strike.reveal_when_ready(&mut listing) == 0 || !visible {
            continue;
        }
        let (open, total) = listing.counts(&explorer);
        explorer.notice = if open == total {
            "Home frames the revealed file.".into()
        } else {
            String::new()
        };
        let height = strike.rows.len() as f32 * ROW_HEIGHT;
        for row in 0..3 {
            for column in 0..6 {
                let width = WIDTH / 6.;
                let tile_height = height / 3.;
                let offset = Vec3::new(
                    (column as f32 + 0.5) * width - WIDTH / 2.,
                    -(row as f32 + 0.5) * tile_height - strike.rows.start as f32 * ROW_HEIGHT,
                    0.16,
                );
                let position = strike.top + offset;
                let away = position - strike.point;
                commands.spawn((
                    Mesh3d(palette.unit.clone()),
                    MeshMaterial3d(palette.shell.clone()),
                    Transform::from_translation(position).with_scale(Vec3::new(
                        width * 0.95,
                        tile_height * 0.93,
                        0.18,
                    )),
                    RigidBody::Dynamic,
                    Collider::cuboid(1., 1., 1.),
                    LinearVelocity(Vec3::new(
                        away.x * 0.55,
                        away.y * 0.35 + 0.9,
                        1.2 + (column % 3) as f32 * 0.55,
                    )),
                    AngularVelocity(Vec3::new(1.4 + row as f32, away.x * 0.25, away.x * 0.14)),
                    physics::Lifetime(5.),
                ));
            }
        }
    }
}

fn draw_cracks(strike: &CoverStrike, gizmos: &mut Gizmos) {
    let progress = (strike.elapsed / CRACK_SECONDS).clamp(0., 1.);
    let min = Vec3::new(
        strike.top.x - WIDTH / 2.,
        strike.top.y - strike.rows.end as f32 * ROW_HEIGHT,
        strike.top.z,
    );
    let max = Vec3::new(
        strike.top.x + WIDTH / 2.,
        strike.top.y - strike.rows.start as f32 * ROW_HEIGHT,
        strike.top.z,
    );
    let ink = Color::srgb(1., 0.72, 0.33);
    let root = strike.point + Vec3::Z * 0.035;
    for branch in 0..9 {
        let angle = branch as f32 * std::f32::consts::TAU / 9.;
        let ray = Vec2::from_angle(angle);
        let mut last = root;
        for segment in 1..=5 {
            let reach = (progress * 5. - (segment - 1) as f32).clamp(0., 1.);
            if reach == 0. {
                break;
            }
            let jag = ((segment + branch) as f32 * 2.4).sin() * 0.42;
            let end = strike.point
                + Vec3::new(
                    ray.x * segment as f32 * 1.25,
                    ray.y * segment as f32 * 0.52 + jag,
                    0.,
                );
            let end = end.clamp(min, max) + Vec3::Z * 0.035;
            let current = last.lerp(end, reach);
            gizmos.line(last, current, ink);
            gizmos.line(last + Vec3::Y * 0.018, current + Vec3::Y * 0.018, ink);
            last = current;
        }
    }
    if progress < 0.37 {
        let radius = 0.10 + progress * 1.5;
        for direction in [
            Vec3::X,
            Vec3::Y,
            Vec3::new(0.7, 0.7, 0.),
            Vec3::new(-0.7, 0.7, 0.),
        ] {
            gizmos.line(
                root - direction * radius,
                root + direction * radius,
                Color::WHITE,
            );
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
    let visible = Listing::visible(&explorer, &navigation);
    let key = visible.then_some((
        explorer.selected,
        explorer.before,
        listing.revision,
        explorer.highlighted.len(),
    ));
    if key == listing.rendered {
        return;
    }
    listing.rendered = key;
    for entity in &old {
        commands.entity(entity).despawn();
    }
    if !visible {
        return;
    }

    let [x, z] = repo.0.files[explorer.selected].position;
    let file = &repo.0.files[explorer.selected];
    let current = rows(&explorer);
    let after = Vec3::new(x, top(current.len()), z + FRONT);
    let height = top(current.len().max(diff_rows(&explorer))).max(5.);
    commands.spawn((
        ListingSurface,
        Mesh3d(meshes.add(Cuboid::new(WIDTH, height, SIDE_DEPTH))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color(set.shell),
            perceptual_roughness: 0.85,
            ..default()
        })),
        Transform::from_xyz(x, height / 2., z + SIDE_CENTER),
    ));
    for y in [0.28, height + 0.12] {
        commands.spawn((
            ListingSurface,
            Mesh3d(meshes.add(Cuboid::new(WIDTH + 0.6, 0.24, SIDE_DEPTH + 0.45))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: set.change(&file.change),
                ..default()
            })),
            Transform::from_xyz(x, y, z + SIDE_CENTER),
        ));
    }
    if !current.is_empty() {
        let length = current.len() as f32 * ROW_HEIGHT;
        commands.spawn((
            ListingSurface,
            CoverCollider,
            RigidBody::Static,
            Collider::cuboid(WIDTH, length, 0.12),
            Transform::from_xyz(x, BASE + length / 2., z + FRONT),
        ));
    }
    draw_side(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &current,
        after,
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
        let top = Vec3::new(x + WIDTH / 2. + 0.06, top(changed.len()), z + SIDE_CENTER);
        draw_side(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &changed,
            top,
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
    title: &str,
    tint: [f32; 3],
    revealed: impl Fn(usize) -> bool,
    angle: f32,
) {
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
        angle,
    );
    for (index, tile) in lines.chunks(TILE_ROWS).enumerate() {
        let offset = index * TILE_ROWS;
        let height = tile.len() as f32 * ROW_HEIGHT;
        let image = code_texture::listing(tile, &revealed);
        plane(
            commands,
            meshes,
            materials,
            images,
            image,
            top - Vec3::Y * (offset as f32 * ROW_HEIGHT + height / 2.),
            height,
            angle,
        );
    }
}

fn plane(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    image: Image,
    center: Vec3,
    height: f32,
    angle: f32,
) {
    commands.spawn((
        ListingSurface,
        Mesh3d(meshes.add(Rectangle::new(WIDTH, height))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(center).with_rotation(Quat::from_rotation_y(angle)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_stays_closed_during_cracking_and_reveals_once_afterward() {
        let mut listing = Listing::default();
        let strike = CoverStrike {
            file: 0,
            before: false,
            rows: 0..16,
            point: Vec3::ZERO,
            top: Vec3::Y,
            elapsed: 0.,
        };
        assert_eq!(strike.reveal_when_ready(&mut listing), 0);
        assert!(listing.revealed.is_empty());
        let strike = CoverStrike {
            elapsed: CRACK_SECONDS,
            ..strike
        };
        assert_eq!(strike.reveal_when_ready(&mut listing), 16);
        assert_eq!(strike.reveal_when_ready(&mut listing), 0);
    }

    #[test]
    fn moving_projectile_hits_cover_and_starts_the_crack_stage() {
        use avian3d::prelude::{CollisionEventsEnabled, GravityScale, SweptCcd};
        use bevy::time::TimeUpdateStrategy;

        let mut explorer = Explorer::new(0);
        explorer.highlighted = syntax::highlight("test.rs", &"let x = 1;\n".repeat(32));
        let mut repo = repository_picker::empty();
        repo.files.push(repo_world::File {
            path: "test.rs".into(),
            before: None,
            after: None,
            change: Change::Added,
            position: [0., 0.],
        });
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
        let height = 32. * ROW_HEIGHT;
        app.world_mut().spawn((
            CoverCollider,
            RigidBody::Static,
            Collider::cuboid(WIDTH, height, 0.12),
            Transform::from_xyz(0., BASE + height / 2., FRONT),
        ));
        let shot = app
            .world_mut()
            .spawn((
                CoverShot {
                    file: 0,
                    before: false,
                },
                physics::Projectile { used: false },
                RigidBody::Dynamic,
                Collider::sphere(0.32),
                GravityScale(0.),
                SweptCcd::default(),
                CollisionEventsEnabled,
                LinearVelocity(Vec3::NEG_Z * 40.),
                Transform::from_xyz(0., top(32) - 2., FRONT + 12.),
            ))
            .id();
        for _ in 0..40 {
            app.update();
        }
        assert!(app.world().get_entity(shot).is_err());
        let mut strikes = app.world_mut().query::<&CoverStrike>();
        let strike = strikes.single(app.world()).unwrap();
        assert_eq!(strike.rows, 0..16);
        assert!(app.world().resource::<Listing>().revealed.is_empty());
    }

    #[test]
    fn scroll_stops_at_first_and_last_strip() {
        let count = 47;
        let first = view(
            &repo_world::File {
                path: "test.rs".into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [0., 0.],
            },
            count,
        )
        .translation
        .y;
        let previous = reading_height(first, count, -4.);
        assert!(previous < first);
        let last = reading_height(first, count, -100.);
        assert!(last < previous);
        assert_eq!(reading_height(last, count, -100.), last);
        let first = reading_height(last, count, 100.);
        assert_eq!(reading_height(first, count, 100.), first);
    }

    #[test]
    fn aimed_shots_reveal_original_rows_in_place_and_keep_progress_per_file() {
        let top = Vec3::new(2., 11., 3.);
        let mut listing = Listing::default();
        let total = 35;
        for (height, expected) in [(32.5, 32..35), (0.5, 0..16), (16.5, 16..32)] {
            let origin = top + Vec3::new(0., -height * ROW_HEIGHT, 8.);
            let row = hit_row(origin, Vec3::NEG_Z, top, total).unwrap();
            let tile = tile_rows(row, total);
            assert_eq!(tile, expected);
            assert_eq!(
                listing.reveal(0, false, tile.clone().map(|row| row + 1)),
                tile.len()
            );
            assert_eq!(listing.reveal(0, false, tile.map(|row| row + 1)), 0);
        }
        assert!((1..=total).all(|line| listing.revealed.contains(&(0, false, line))));
        assert!(!listing.revealed.contains(&(1, false, 1)));
        assert!(!listing.revealed.contains(&(0, true, 1)));
    }
}
