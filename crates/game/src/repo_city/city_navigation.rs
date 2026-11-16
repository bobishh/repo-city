//! Camera travel and code stops for the city world.

use super::*;

const PAGE: usize = 23;
const PLATFORM_SIZE: Vec3 = Vec3::new(16., 0.5, 7.);
const PLATFORM_HEIGHT: f32 = listing::MAX_HEIGHT + 28.;

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
    roof: f32,
}

#[derive(Clone, Copy)]
enum Arrival {
    Facade(Option<file_facade::Aim>),
    Stop,
    Comparison,
    Overview,
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
            roof: start.translation.y.max(end.translation.y).max(22.),
        }
    }

    fn facade(
        start: Transform,
        end: Transform,
        aim: Option<file_facade::Aim>,
        repo: &Repository,
        explorer: &Explorer,
    ) -> Self {
        let mut hop = Self::new(start, end, Arrival::Facade(aim));
        hop.roof = hop.roof.max(flight_roof(
            start.translation,
            end.translation,
            repo,
            explorer,
        ));
        let climb = hop.roof - start.translation.y.min(end.translation.y);
        hop.duration = hop.duration.max((climb / 24. + 0.65).min(3.8));
        hop
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
            let roof = self.roof;
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
            rotation = self
                .start
                .rotation
                .slerp(aimed.rotation, smooth(t / 0.28))
                .slerp(self.end.rotation, smooth((t - 0.72) / 0.28));
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
    overview_return: Option<Transform>,
    pub overview_requested: bool,
}

impl Navigation {
    pub(super) fn overview(&self) -> bool {
        self.overview_return.is_some()
    }

    fn overview_hop(&mut self, start: Transform, end: Transform) {
        let mut hop = Hop::new(start, end, Arrival::Overview);
        hop.duration =
            (start.translation.distance(end.translation) / 260. + 0.35).clamp(0.50, 1.05);
        self.hop = Some(hop);
    }

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

    pub(super) fn prompt(&self, explorer: &Explorer) -> String {
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
                format!("{hint}\nLeft-click an aimed reviewed platform to clear it")
            } else {
                hint.into()
            };
        }
        if explorer.comparison_mode > 0 {
            return "FULL DIFF · Scroll to read · G switches view · Esc to fly".into();
        }
        if explorer.anchored {
            return "SOURCE WALL · Scroll to read · E flies to the facade · Enter closes the wall"
                .into();
        }
        let trace = if self.trace.is_empty() {
            ""
        } else {
            " · T follows stack trace"
        };
        let shortcut = file_picker::SHORTCUT;
        format!(
            "Click to capture · Esc releases · Right-click opens actions · E flies to the aimed face\nScroll follows the text · Files / {shortcut} searches paths · WASD flies\nHold left mouse close to wash source · Home frames the file · G diff · B blame · C reviews changes{trace}"
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

fn district_view(district: &repo_world::District, aspect: f32) -> Transform {
    let [x, z] = district.position;
    let [width, depth] = district.size;
    let center = Vec3::new(x + width / 2., 0., z + depth / 2.);
    let diameter = (width / aspect.max(0.5)).max(depth) + 28.;
    let half_fov = std::f32::consts::FRAC_PI_8;
    let height = listing::MAX_HEIGHT + diameter * 0.5 / half_fov.tan();
    Transform::from_translation(center + Vec3::Y * height).looking_to(Vec3::NEG_Y, Vec3::NEG_Z)
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
    mut menu: ResMut<city_menu::Menu>,
    mut saved_progress: ResMut<progress::Progress>,
    search: Res<listing::find::Search>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &Pilot), PlayerCamera>,
) {
    if picker.is_open() || listing.turning() {
        return;
    }
    let (Ok((mut camera, pilot)), Ok(window)) = (camera.single_mut(), windows.single()) else {
        return;
    };
    if !window.focused {
        return;
    }

    let overview =
        keys.just_pressed(KeyCode::KeyM) || std::mem::take(&mut navigation.overview_requested);
    if overview || (navigation.overview() && keys.just_pressed(KeyCode::Escape)) {
        listing.reset_view();
        if let Some(origin) = navigation.overview_return.take() {
            navigation.overview_hop(*camera, origin);
        } else {
            let file = &repo.0.files[explorer.selected];
            let name = file
                .path
                .rsplit_once('/')
                .map_or("/", |(directory, _)| directory);
            let target = nearest
                .district
                .and_then(|index| repo.0.districts.get(index))
                .or_else(|| {
                    repo.0
                        .districts
                        .iter()
                        .find(|district| district.name == name)
                });
            if let Some(district) = target {
                navigation.route = None;
                navigation.redraw = true;
                explorer.anchored = false;
                explorer.comparison_mode = 0;
                explorer.rebuild = true;
                navigation.overview_return = Some(*camera);
                let end = district_view(district, window.width() / window.height().max(1.));
                navigation.overview_hop(*camera, end);
            }
        }
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
        if let Arrival::Facade(aim) = hop.arrival {
            let file = &repo.0.files[explorer.selected];
            let end = listing.arrival_view(
                &explorer,
                file,
                explorer.facade_size(file, explorer.selected),
                aim,
                hop.start.translation,
            );
            if hop.end != end {
                // Source loading can resize the wall. Retarget from the current pose.
                *hop = Hop::facade(*camera, end, aim, &repo.0, &explorer);
            }
        }
        hop.elapsed += time.delta_secs();
        let progress = hop.elapsed / hop.duration;
        *camera = hop.pose(progress);
        if progress >= 1. {
            let arrival = hop.arrival;
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
                    if explorer.reviewed.insert(selected) {
                        saved_progress.mark(
                            progress::Mark::Reviewed(repo.0.files[selected].path.clone()),
                            true,
                        );
                    }
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
                Arrival::Facade(_) | Arrival::Overview => {}
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
    let next_changed = keys.just_pressed(KeyCode::KeyN) && !search.owns_next();
    let target = if let Some(aim) = menu.destination.take() {
        Some((aim.index, Some(aim)))
    } else if let Some(file) = picker.destination.take() {
        Some((file, None))
    } else if navigation.overview()
        && (mouse.just_pressed(MouseButton::Left) || keys.just_pressed(KeyCode::KeyE))
    {
        nearest.file.map(|aim| (aim.index, None))
    } else if next_changed {
        (1..repo.0.files.len())
            .map(|offset| (explorer.selected + offset) % repo.0.files.len())
            .find(|&index| repo.0.files[index].change != Change::Unchanged)
            .map(|index| (index, None))
    } else if let Some(aim) = pilot.shot.then_some(nearest.file).flatten().filter(|aim| {
        let file = &repo.0.files[aim.index];
        let size = explorer.facade_size(file, aim.index);
        nearest.near_source != Some(aim.index)
            && !aim.within_tool_reach(file, size, camera.translation, nearest.file_distance)
    }) {
        Some((aim.index, Some(aim.on_side(listing::Face::Source))))
    } else if keys.just_pressed(KeyCode::KeyE) {
        nearest.file.map(|aim| (aim.index, Some(aim))).or_else(|| {
            nearest
                .near_source
                .or_else(|| {
                    (explorer.anchored || explorer.comparison_mode > 0).then_some(explorer.selected)
                })
                .map(|index| (index, None))
        })
    } else {
        None
    };
    if let Some((index, aimed)) = target {
        let from_overview = navigation.overview_return.take().is_some();
        listing.reset_view();
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        if index != explorer.selected {
            explorer.open(&repo.0, index);
        }
        explorer.rebuild = true;
        let file = &repo.0.files[index];
        let size = explorer.facade_size(file, index);
        explorer.frame_loaded_source = false;
        let end = listing.arrival_view(&explorer, file, size, aimed, camera.translation);
        if camera.translation.distance_squared(end.translation) < 0.04
            && camera.rotation.dot(end.rotation).abs() > 0.9999
        {
            return;
        }
        let mut hop = Hop::facade(*camera, end, aimed, &repo.0, &explorer);
        if from_overview {
            hop.duration = hop.duration.min(1.65);
        }
        navigation.hop = Some(hop);
    } else if !navigation.overview()
        && !explorer.anchored
        && explorer.comparison_mode == 0
        && let Some(index) = nearest
            .near_source
            .filter(|&index| index != explorer.selected)
    {
        listing.reset_view();
        explorer.open(&repo.0, index);
        explorer.frame_loaded_source = false;
    }
}

fn flight_roof(start: Vec3, end: Vec3, repo: &Repository, explorer: &Explorer) -> f32 {
    let origin = Vec3::new(start.x, 0., start.z);
    let offset = Vec3::new(end.x, 0., end.z) - origin;
    let distance = offset.length();
    let direction = offset.try_normalize().unwrap_or(Vec3::ZERO);

    repo.files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            let [x, z] = file.position;
            let size = explorer.facade_size(file, index);
            let width = size.x / 2. + 1.;
            let depth = size.z / 2. + 1.;
            let hit = ray_box(
                origin,
                direction,
                Vec3::new(x - width, 0., z - depth),
                Vec3::new(x + width, 1., z + depth),
            )?;
            (hit <= distance).then(|| size.y + 4.)
        })
        .fold(0., f32::max)
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
        && let Some(aim) = nearest.file
    {
        let index = aim.index;
        let file = &repo.0.files[index];
        let [x, z] = file.position;
        let size = explorer.facade_size(file, index);
        let height = size.y;
        let w = size.x / 2. + 0.08;
        let d = size.z / 2. + 0.08;
        let footprint = [
            Vec3::new(x - w, 0.1, z - d),
            Vec3::new(x + w, 0.1, z - d),
            Vec3::new(x + w, 0.1, z + d),
            Vec3::new(x - w, 0.1, z + d),
        ];
        for edge in 0..4 {
            let bottom = footprint[edge];
            let next = footprint[(edge + 1) % 4];
            let rise = Vec3::Y * height;
            gizmos.line(bottom, bottom + rise, color(set.accent));
            gizmos.line(bottom + rise, next + rise, color(set.accent));
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
                Mesh3d(meshes.add(Cuboid::from_size(PLATFORM_SIZE))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color(if current { set.accent } else { set.shell }),
                    ..default()
                })),
                Transform::from_translation(point),
            ))
            .id();
        if route.kind == RouteKind::Review && index < route.current {
            commands.entity(body).insert(ReviewedPlatform(index));
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
                    let panel = panel(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        &title,
                        &diff.page(before, row, count),
                        Vec3::new(side, 3., 0.),
                        7.8,
                        &set,
                    );
                    commands.entity(body).add_child(panel);
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
                let panel = panel(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &mut images,
                    &title,
                    &rows,
                    Vec3::Y * 3.,
                    10.,
                    &set,
                );
                commands.entity(body).add_child(panel);
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
) -> Entity {
    let texture = images.add(code_texture::source(title, rows, set.accent, 1024, 704, 0));
    commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(width, 5.))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(texture),
                unlit: true,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(position),
        ))
        .id()
}

pub(super) fn shoot_reviewed_platform(
    mut commands: Commands,
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    platforms: Query<(&Transform, &ReviewedPlatform)>,
    mut navigation: ResMut<Navigation>,
) {
    if navigation.is_flying() || navigation.route.is_none() {
        return;
    }
    let Ok((camera, pilot)) = camera.single() else {
        return;
    };
    if !pilot.shot {
        return;
    }
    let origin = camera.translation;
    let direction = camera.forward().as_vec3();
    let half = PLATFORM_SIZE * 0.5;
    let hit = platforms
        .iter()
        .filter(|(_, platform)| !navigation.broken.contains(&platform.0))
        .filter_map(|(pose, platform)| {
            ray_box(
                origin,
                direction,
                pose.translation - half,
                pose.translation + half,
            )
            .filter(|&distance| distance <= 60.)
            .map(|distance| (distance, platform.0))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((distance, index)) = hit {
        navigation.broken.insert(index);
        navigation.redraw = true;
        physics::splash(&mut commands, origin + direction * distance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_frames_all_roofs_in_wide_and_narrow_windows() {
        let district = repo_world::District {
            name: "src".into(),
            position: [100., 100.],
            size: [175., 120.],
        };
        for aspect in [0.6, 1., 1.78] {
            let eye = district_view(&district, aspect);
            assert!(eye.forward().dot(Vec3::NEG_Y) > 0.999);
            assert_eq!(eye.translation.xz(), Vec2::new(187.5, 160.));
            let half =
                (eye.translation.y - listing::MAX_HEIGHT) * std::f32::consts::FRAC_PI_8.tan();
            assert!(half > 60. && half * aspect > 87.5);
        }
    }

    #[test]
    fn approaching_another_front_reads_in_place_without_a_click() {
        let mut repo = repository_picker::empty();
        for path in ["current.rs", "nearby.toml"] {
            repo.files.push(repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [0., 0.],
            });
        }
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Nearest>()
            .init_resource::<Navigation>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<listing::Listing>()
            .init_resource::<listing::find::Search>()
            .init_resource::<progress::Progress>()
            .add_systems(Update, navigate);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let pose = Transform::from_xyz(0., 1.65, 7.);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), pose, Pilot::default()))
            .id();
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 0);
        app.world_mut().resource_mut::<Nearest>().near_source = Some(1);
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 1);
        assert!(!app.world().resource::<Navigation>().guides_camera());
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), pose);
    }

    #[test]
    fn first_click_captures_then_far_click_flies_to_source_while_e_keeps_the_aimed_face() {
        let mut repo = repository_picker::empty();
        for (index, path) in ["current.rs", "target.rs"].into_iter().enumerate() {
            repo.files.push(repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [90. * index as f32, 0.],
            });
        }
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .init_resource::<Nearest>()
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Navigation>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<listing::Listing>()
            .init_resource::<listing::find::Search>()
            .init_resource::<progress::Progress>()
            .add_systems(Update, (targeting, navigate).chain());
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let aim = Vec3::new(90., 0.6, 0.);
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(120., aim.y, 0.).looking_at(aim, Vec3::Y),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        app.world_mut().spawn(Facade { index: 1 });
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 0);
        assert!(!app.world().resource::<Navigation>().is_flying());

        app.world_mut().get_mut::<Pilot>(camera).unwrap().shot = true;
        let close =
            Transform::from_xyz(105., 20., 0.).looking_at(Vec3::new(97., aim.y, 0.), Vec3::Y);
        let far = *app.world().get::<Transform>(camera).unwrap();
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = close;
        app.update();
        assert!(!app.world().resource::<Navigation>().is_flying());

        *app.world_mut().get_mut::<Transform>(camera).unwrap() = far;
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 1);
        let travel = app.world().resource::<Navigation>().hop.as_ref().unwrap();
        assert!(travel.end.forward().dot(Vec3::NEG_Z) > 0.98);
        app.world_mut().resource_mut::<Navigation>().hop = None;
        app.world_mut().get_mut::<Pilot>(camera).unwrap().shot = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 1);
        let hop = app.world().resource::<Navigation>().hop.as_ref().unwrap();
        assert!(hop.end.translation.x > aim.x);
        assert!(hop.end.forward().dot(Vec3::NEG_X) > 0.99);
    }

    #[test]
    fn approach_clears_roofs_and_ends_at_the_reading_view() {
        let start = Transform::from_xyz(0., 5., 10.);
        let end = Transform::from_xyz(100., 8., 15.).looking_to(Vec3::NEG_X, Vec3::Y);
        let hop = Hop::new(start, end, Arrival::Facade(None));
        assert_eq!(hop.pose(0.).translation, start.translation);
        assert!(hop.pose(0.5).translation.y > 18.);
        assert_eq!(hop.pose(1.).translation, end.translation);
        assert!(hop.pose(1.).rotation.dot(end.rotation).abs() > 0.9999);
        let just_before = hop.pose(0.999);
        assert!(just_before.translation.distance(end.translation) < 0.01);
        assert!(just_before.rotation.dot(end.rotation).abs() > 0.9999);
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
    fn a_click_clears_the_aimed_platform_once() {
        let mut app = App::new();
        let navigation = Navigation {
            route: Some(Route {
                kind: RouteKind::Review,
                stops: vec![Stop::Review {
                    hunk: 0,
                    row: 0,
                    count: 1,
                }],
                current: 0,
            }),
            ..default()
        };
        app.insert_resource(navigation)
            .add_systems(Update, shoot_reviewed_platform);
        let pilot = Pilot {
            captured: true,
            shot: true,
            ..default()
        };
        app.world_mut()
            .spawn((Camera3d::default(), Transform::from_xyz(0., 0., 5.), pilot));
        app.world_mut()
            .spawn((ReviewedPlatform(0), Transform::default()));
        app.world_mut()
            .spawn((ReviewedPlatform(1), Transform::from_xyz(15., 0., 0.)));
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Navigation>().broken,
            HashSet::from([0])
        );
        let mut effects = app.world_mut().query::<&physics::Lifetime>();
        assert_eq!(effects.iter(app.world()).count(), 1);
    }
}
