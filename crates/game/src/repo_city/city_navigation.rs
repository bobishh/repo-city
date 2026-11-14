//! Camera travel and code stops for the city world.
use avian3d::prelude::{AngularVelocity, Collider, CollisionStart, LinearVelocity, RigidBody};

use super::*;

const PAGE: usize = 23;
const PLATFORM_HEIGHT: f32 = 52.;

#[derive(Clone, Copy)]
enum Stop {
    Review {
        hunk: usize,
        row: usize,
        count: usize,
    },
    Trace {
        file: usize,
        line: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteKind {
    Review,
    Trace,
}

struct Route {
    kind: RouteKind,
    stops: Vec<Stop>,
    current: usize,
}

struct Hop {
    start: Transform,
    end: Transform,
    elapsed: f32,
    duration: f32,
    arrival: Arrival,
}

#[derive(Clone, Copy)]
enum Arrival {
    Facade(usize),
    Stop,
    Comparison,
}

impl Hop {
    fn new(start: Transform, end: Transform, arrival: Arrival) -> Self {
        let duration = (start.translation.distance(end.translation) / 90. + 0.65).clamp(0.8, 3.0);
        Self {
            start,
            end,
            elapsed: 0.,
            duration,
            arrival,
        }
    }

    fn pose(&self, progress: f32) -> Transform {
        let t = progress.clamp(0., 1.);
        let eased = t * t * (3. - 2. * t);
        let lift = (self.start.translation.distance(self.end.translation) * 0.18).clamp(6., 34.);
        let mut translation = self.start.translation.lerp(self.end.translation, eased);
        translation.y += lift * (std::f32::consts::PI * eased).sin().max(0.);
        let mut rotation = self.start.rotation.slerp(self.end.rotation, eased);
        if matches!(self.arrival, Arrival::Facade(_)) {
            let smooth = |v: f32| {
                let v = v.clamp(0., 1.);
                v * v * (3. - 2. * v)
            };
            let horizontal = smooth((t - 0.2) / 0.6);
            let roof = self
                .start
                .translation
                .y
                .max(self.end.translation.y)
                .max(22.);
            translation = self
                .start
                .translation
                .lerp(self.end.translation, horizontal);
            translation.y = if t < 0.2 {
                self.start.translation.y.lerp(roof, smooth(t / 0.2))
            } else if t > 0.8 {
                roof.lerp(self.end.translation.y, smooth((t - 0.8) / 0.2))
            } else {
                roof + lift * (std::f32::consts::PI * horizontal).sin().max(0.) * 0.4
            };
            let target = self.end.translation + self.end.forward().as_vec3() * 11.;
            let aimed = Transform::from_translation(translation).looking_at(target, Vec3::Y);
            rotation = self.start.rotation.slerp(aimed.rotation, smooth(t / 0.28));
        }
        Transform::from_translation(translation).with_rotation(rotation)
    }
}

#[derive(Component)]
pub(super) struct RouteVisual;

#[derive(Component)]
pub(super) struct ReviewedPlatform(usize);

#[derive(Resource, Default)]
pub(super) struct Navigation {
    trace: Vec<Stop>,
    route: Option<Route>,
    hop: Option<Hop>,
    redraw: bool,
    broken: HashSet<usize>,
}

impl Navigation {
    pub(super) fn new(repo: &Repository, trace_path: Option<&str>) -> Result<Self, String> {
        let trace = if let Some(path) = trace_path {
            let content = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            parse_trace(repo, &content)?
        } else {
            Vec::new()
        };
        Ok(Self { trace, ..default() })
    }

    pub(super) fn guides_camera(&self) -> bool {
        self.hop.is_some() || self.route.is_some()
    }

    pub(super) fn is_flying(&self) -> bool {
        self.hop.is_some()
    }

    pub(super) fn route_status(&self) -> Option<String> {
        self.route.as_ref().map(|route| {
            let title = match route.stops[route.current] {
                Stop::Review { hunk, .. } => format!("Review · hunk {}", hunk + 1),
                Stop::Trace { line, .. } => format!("Stack trace · line {line}"),
            };
            format!(
                "{title} · stop {} / {}",
                route.current + 1,
                route.stops.len()
            )
        })
    }

    pub(super) fn prompt(
        &self,
        explorer: &Explorer,
        nearest: &Nearest,
        repo: &Repository,
    ) -> String {
        if self.hop.is_some() {
            return "Flying to destination · Esc to cancel".into();
        }
        if let Some(route) = &self.route {
            let last = route.current + 1 == route.stops.len();
            let hint = match (route.kind, last) {
                (RouteKind::Review, true) => {
                    "W flies to the full diff · S goes back · Esc returns to the city"
                }
                (RouteKind::Review, false) => {
                    "W flies to the next change · S goes back · Esc returns to the city"
                }
                (RouteKind::Trace, true) => "End of stack trace · S to go back · Esc to leave",
                (RouteKind::Trace, false) => "W to follow the stack · S to go back · Esc to leave",
            };
            return if route.kind == RouteKind::Review && route.current > 0 {
                format!("{hint}\nAim at a reviewed platform, then hold F and release to break it")
            } else {
                hint.into()
            };
        }
        if explorer.comparison_mode > 0 {
            return "FULL DIFF · Scroll to read · G switches view · Esc to fly".into();
        }
        if explorer.anchored {
            return "SOURCE WALL · Scroll to read · E or Esc to fly".into();
        }
        let destination = nearest
            .file
            .filter(|&index| index != explorer.selected)
            .map(|index| format!("Click to fly to {}", repo.files[index].path))
            .unwrap_or_else(|| "Point at a building to see where it leads".into());
        let trace = if self.trace.is_empty() {
            ""
        } else {
            " · T follows stack trace"
        };
        format!(
            "{destination}\nScroll follows the text · Files / Cmd+P searches paths · WASD flies\nHold F and release to reveal code · Home frames the file · G turns to the diff · C reviews changes{trace}"
        )
    }
}

fn parse_trace(repo: &Repository, source: &str) -> Result<Vec<Stop>, String> {
    let stops = source.lines().enumerate().filter(|(_, text)| !text.trim().is_empty()).map(|(index, text)| {
        let error = || format!("Frame {} needs a repository-relative path and positive line (path:line): {text}", index + 1);
        let (path, line) = text.trim().rsplit_once(':').ok_or_else(error)?;
        let line = line.parse::<usize>().ok().filter(|&line| line > 0).ok_or_else(error)?;
        let file = repo.files.iter().position(|file| file.path == path && file.after.is_some())
            .ok_or_else(|| format!("Frame {} is absent from the selected revision: {path}", index + 1))?;
        Ok(Stop::Trace { file, line })
    }).collect::<Result<Vec<_>, String>>()?;
    if stops.is_empty() {
        return Err("No stack frames supplied.".into());
    }
    Ok(stops)
}

fn review_stops(diff: &Comparison) -> Vec<Stop> {
    diff.hunks
        .iter()
        .enumerate()
        .flat_map(|(hunk, change)| {
            let len = change.before.len().max(change.after.len());
            (0..len).step_by(PAGE).map(move |offset| Stop::Review {
                hunk,
                row: change.row + offset,
                count: (len - offset).min(PAGE),
            })
        })
        .collect()
}

fn stop_position(route: &Route, index: usize, repo: &Repository, selected: usize) -> Vec3 {
    match route.stops[index] {
        Stop::Review { .. } => {
            let [x, z] = repo.files[selected].position;
            Vec3::new(
                x + index as f32 * 18.,
                PLATFORM_HEIGHT,
                z - index as f32 * 15.,
            )
        }
        Stop::Trace { file, .. } => {
            let [x, z] = repo.files[file].position;
            Vec3::new(x + (index % 5) as f32 * 12., PLATFORM_HEIGHT, z)
        }
    }
}

fn fly_to_stop(navigation: &mut Navigation, camera: Transform, repo: &Repository, selected: usize) {
    let route = navigation.route.as_ref().unwrap();
    let point = stop_position(route, route.current, repo, selected);
    let end = Transform::from_translation(point + Vec3::new(0., 5., 15.))
        .looking_at(point + Vec3::Y * 3., Vec3::Y);
    navigation.hop = Some(Hop::new(camera, end, Arrival::Stop));
    navigation.redraw = true;
}

pub(super) fn navigate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    repo: Res<Repo>,
    nearest: Res<Nearest>,
    mut explorer: ResMut<Explorer>,
    mut navigation: ResMut<Navigation>,
    mut picker: ResMut<file_picker::FilePicker>,
    mut listing: ResMut<listing::Listing>,
    file_buttons: Query<&bevy::picking::hover::Hovered, With<file_picker::FileButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    if picker.is_open() || listing.turning() {
        return;
    }
    let (Ok(mut camera), Ok(window)) = (camera.single_mut(), windows.single()) else {
        return;
    };
    if !window.focused {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        listing.reset_view();
        let leaving_view = navigation.guides_camera() || explorer.comparison_mode > 0;
        navigation.hop = None;
        navigation.route = None;
        navigation.redraw = true;
        if leaving_view {
            explorer.comparison_mode = 0;
            explorer.rebuild = true;
        }
        return;
    }
    if let Some(hop) = &mut navigation.hop {
        hop.elapsed += time.delta_secs();
        let progress = hop.elapsed / hop.duration;
        *camera = hop.pose(progress);
        if progress >= 1. {
            let arrival = hop.arrival;
            *camera = hop.end;
            navigation.hop = None;
            match arrival {
                Arrival::Comparison => {
                    explorer.comparison_mode = 1;
                    explorer.anchored = true;
                    explorer.page = explorer
                        .comparison
                        .as_ref()
                        .and_then(|diff| diff.hunks.first())
                        .map_or(0, |hunk| hunk.row / PAGE);
                    explorer.hunk = 0;
                    explorer.rebuild = true;
                    let selected = explorer.selected;
                    explorer.reviewed.insert(selected);
                }
                Arrival::Stop => {
                    if let Some(route) = &navigation.route
                        && let Stop::Trace { file, line } = route.stops[route.current]
                    {
                        if file != explorer.selected {
                            explorer.open(&repo.0, file);
                        }
                        explorer.page = (line - 1) / PAGE;
                        explorer.rebuild = true;
                    }
                    navigation.redraw = true;
                }
                Arrival::Facade(index) => {
                    if explorer.selected != index {
                        explorer.open(&repo.0, index);
                        #[cfg(target_arch = "wasm32")]
                        {
                            *camera =
                                listing::view(&repo.0.files[index], explorer.highlighted.len());
                        }
                    }
                }
            }
        }
        return;
    }

    if let Some(route) = &mut navigation.route {
        if keys.just_pressed(KeyCode::KeyW) && route.current + 1 == route.stops.len() {
            if route.kind == RouteKind::Review {
                navigation.route = None;
                navigation.redraw = true;
                let mut end = *camera;
                comparison_scene::focus(&mut end, &repo.0, &explorer);
                navigation.hop = Some(Hop::new(*camera, end, Arrival::Comparison));
            }
        } else if keys.just_pressed(KeyCode::KeyW) || keys.just_pressed(KeyCode::KeyS) {
            let old = route.current;
            route.current = if keys.just_pressed(KeyCode::KeyW) {
                old + 1
            } else {
                old.saturating_sub(1)
            };
            if old != route.current {
                fly_to_stop(&mut navigation, *camera, &repo.0, explorer.selected);
            }
        }
        return;
    }

    if keys.just_pressed(KeyCode::KeyC) || keys.just_pressed(KeyCode::KeyT) {
        let trace = keys.just_pressed(KeyCode::KeyT);
        let (kind, stops) = if trace {
            (RouteKind::Trace, navigation.trace.clone())
        } else {
            (
                RouteKind::Review,
                explorer
                    .comparison
                    .as_ref()
                    .map_or_else(Vec::new, review_stops),
            )
        };
        if stops.is_empty() {
            explorer.notice = if trace {
                "Launch with --trace PATH to follow a stack trace."
            } else {
                "This file has no readable text changes. N visits the next changed file."
            }
            .into();
            return;
        }
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        explorer.rebuild = true;
        navigation.broken.clear();
        navigation.route = Some(Route {
            kind,
            stops,
            current: 0,
        });
        fly_to_stop(&mut navigation, *camera, &repo.0, explorer.selected);
        return;
    }
    let next_changed = keys.just_pressed(KeyCode::KeyN);
    let target = if let Some(file) = picker.destination.take() {
        Some(file)
    } else if next_changed {
        (1..repo.0.files.len())
            .map(|offset| (explorer.selected + offset) % repo.0.files.len())
            .find(|&index| repo.0.files[index].change != Change::Unchanged)
    } else if !explorer.anchored
        && explorer.comparison_mode == 0
        && mouse.just_pressed(MouseButton::Left)
        && !file_buttons.iter().any(|hovered| hovered.get())
    {
        nearest.file.filter(|&index| index != explorer.selected)
    } else {
        None
    };
    if let Some(index) = target {
        listing.reset_view();
        let count = if index == explorer.selected {
            explorer.highlighted.len()
        } else {
            let file = &repo.0.files[index];
            repo.0
                .source(index, file.after.is_none())
                .map_or(0, |source| source.lines().count())
        };
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        explorer.rebuild = true;
        explorer.notice = format!("Flying to {}", repo.0.files[index].path);
        navigation.hop = Some(Hop::new(
            *camera,
            listing::view(&repo.0.files[index], count),
            Arrival::Facade(index),
        ));
        return;
    }
}

pub(super) fn draw(
    mut commands: Commands,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut navigation: ResMut<Navigation>,
    set: Res<WorldSet>,
    old: Query<Entity, With<RouteVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut gizmos: Gizmos,
    nearest: Res<Nearest>,
) {
    if !navigation.guides_camera()
        && !explorer.anchored
        && explorer.comparison_mode == 0
        && let Some(index) = nearest.file.filter(|&index| index != explorer.selected)
    {
        let file = &repo.0.files[index];
        let [x, z] = file.position;
        let height = (4.
            + (file
                .after
                .as_ref()
                .or(file.before.as_ref())
                .map_or(1, |blob| blob.size)
                .max(1) as f32)
                .log2()
                * 0.65)
            .min(17.);
        let corners = [
            Vec3::new(x - 6.1, 0.1, z + 3.23),
            Vec3::new(x + 6.1, 0.1, z + 3.23),
            Vec3::new(x + 6.1, height, z + 3.23),
            Vec3::new(x - 6.1, height, z + 3.23),
        ];
        for edge in 0..4 {
            gizmos.line(corners[edge], corners[(edge + 1) % 4], color(set.accent));
        }
    }
    if let Some(route) = &navigation.route
        && route.current + 1 < route.stops.len()
    {
        let start = stop_position(route, route.current, &repo.0, explorer.selected);
        let end = stop_position(route, route.current + 1, &repo.0, explorer.selected);
        for segment in 0..12 {
            let t = segment as f32 / 12.;
            gizmos.line(
                start.lerp(end, t),
                start.lerp(end, t + 0.04),
                color(set.accent),
            );
        }
    }
    if !navigation.redraw {
        return;
    }
    navigation.redraw = false;
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let Some(route) = &navigation.route else {
        return;
    };
    let first = route.current.saturating_sub(1);
    for index in first..(route.current + 2).min(route.stops.len()) {
        let point = stop_position(route, index, &repo.0, explorer.selected);
        let current = index == route.current;
        if navigation.broken.contains(&index) && !current {
            continue;
        }
        let body = commands
            .spawn((
                RouteVisual,
                Mesh3d(meshes.add(Cuboid::new(16., 0.5, 7.))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color(if current { set.accent } else { set.shell }),
                    ..default()
                })),
                Transform::from_translation(point),
            ))
            .id();
        if route.kind == RouteKind::Review && index < route.current {
            commands.entity(body).insert((
                ReviewedPlatform(index),
                RigidBody::Static,
                Collider::cuboid(16., 0.5, 7.),
            ));
        }
        match route.stops[index] {
            Stop::Review { hunk, row, count } => {
                let Some(diff) = &explorer.comparison else {
                    continue;
                };
                for before in [true, false] {
                    let title = format!(
                        "Hunk {} / {}",
                        hunk + 1,
                        if before { "BEFORE" } else { "AFTER" }
                    );
                    let side = if before { -4. } else { 4. };
                    panel(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        &title,
                        &diff.page(before, row, count),
                        point + Vec3::new(side, 3., 0.),
                        7.8,
                        &set,
                    );
                }
            }
            Stop::Trace { file, line } => {
                let path = &repo.0.files[file].path;
                let number = index + 1;
                let mut title = format!("Frame {number} / {path}:{line}");
                let rows = match repo.0.source(file, false) {
                    Ok(source) if line <= source.lines().count() => {
                        syntax::highlight(path, &source)
                            .into_iter()
                            .skip(line.saturating_sub(12))
                            .take(PAGE)
                            .map(|row| (Some(row), repo_world::comparison::Mark::Context))
                            .collect()
                    }
                    Ok(_) => {
                        title = format!("Frame {number} / {path}:{line} · line outside snapshot");
                        Vec::new()
                    }
                    Err(error) => {
                        title = format!("Frame {number} / {error}");
                        Vec::new()
                    }
                };
                panel(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    &title,
                    &rows,
                    point + Vec3::Y * 3.,
                    10.,
                    &set,
                );
            }
        }
    }
}

fn panel(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    title: &str,
    rows: &[(Option<syntax::Line>, repo_world::comparison::Mark)],
    position: Vec3,
    width: f32,
    set: &WorldSet,
) {
    let texture = images.add(code_texture::source(title, rows, set.accent, 1024, 704, 0));
    commands.spawn((
        RouteVisual,
        Mesh3d(meshes.add(Rectangle::new(width, 5.))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(position),
    ));
}

pub(super) fn break_platforms(
    mut commands: Commands,
    mut events: MessageReader<CollisionStart>,
    mut shots: Query<&mut physics::Projectile>,
    platforms: Query<(&Transform, &ReviewedPlatform)>,
    mut navigation: ResMut<Navigation>,
    palette: Res<Palette>,
) {
    for event in events.read() {
        let (shot, platform) = if platforms.contains(event.collider1) {
            (event.collider2, event.collider1)
        } else {
            (event.collider1, event.collider2)
        };
        let (Ok(mut shot), Ok((transform, stop))) = (shots.get_mut(shot), platforms.get(platform))
        else {
            continue;
        };
        if shot.used || !navigation.broken.insert(stop.0) {
            continue;
        }
        shot.used = true;
        navigation.redraw = true;
        for tile in 0..8 {
            let offset = Vec3::new(
                (tile % 4) as f32 * 4. - 6.,
                0.,
                (tile / 4) as f32 * 3.5 - 1.75,
            );
            commands.spawn((
                Mesh3d(palette.unit.clone()),
                MeshMaterial3d(palette.edge.clone()),
                Transform::from_translation(transform.translation + offset)
                    .with_scale(Vec3::new(3.8, 0.45, 3.2)),
                RigidBody::Dynamic,
                Collider::cuboid(1., 1., 1.),
                LinearVelocity(Vec3::new(offset.x * 0.45, 1.5, offset.z)),
                AngularVelocity(Vec3::new(0.4, tile as f32 * 0.13, 0.6)),
                physics::Lifetime(12.),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approach_clears_roofs_and_ends_at_the_reading_view() {
        let start = Transform::from_xyz(0., 5., 10.);
        let end = Transform::from_xyz(100., 8., 15.);
        let hop = Hop::new(start, end, Arrival::Facade(0));
        assert_eq!(hop.pose(0.).translation, start.translation);
        assert!(hop.pose(0.5).translation.y > 18.);
        assert_eq!(hop.pose(1.).translation, end.translation);
    }

    #[test]
    fn review_route_covers_large_hunks_and_keeps_git_row_order() {
        let old = (0..80)
            .map(|index| format!("old {index}\n"))
            .collect::<String>();
        let new = old.replace("old", "new");
        let diff = Comparison::new("source.txt", &old, &new).unwrap();
        let stops = review_stops(&diff);
        let rows = stops
            .into_iter()
            .flat_map(|stop| match stop {
                Stop::Review { row, count, .. } => row..row + count,
                Stop::Trace { .. } => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, (0..80).collect::<Vec<_>>());
    }

    #[test]
    fn projectile_breaks_a_reviewed_platform_once_and_creates_falling_tiles() {
        let mut app = App::new();
        app.add_message::<CollisionStart>()
            .init_resource::<Navigation>()
            .insert_resource(Palette {
                unit: default(),
                edge: default(),
                shell: default(),
                removed: default(),
                plate: default(),
            })
            .add_systems(Update, break_platforms);
        let shot = app
            .world_mut()
            .spawn(physics::Projectile { used: false })
            .id();
        let platform = app
            .world_mut()
            .spawn((ReviewedPlatform(0), Transform::default()))
            .id();
        for _ in 0..2 {
            app.world_mut().write_message(CollisionStart {
                collider1: shot,
                collider2: platform,
                body1: Some(shot),
                body2: Some(platform),
            });
            app.update();
        }
        assert!(app.world().resource::<Navigation>().broken.contains(&0));
        let mut tiles = app.world_mut().query::<(&RigidBody, &physics::Lifetime)>();
        assert_eq!(
            tiles
                .iter(app.world())
                .filter(|(body, _)| **body == RigidBody::Dynamic)
                .count(),
            8
        );
    }
}
