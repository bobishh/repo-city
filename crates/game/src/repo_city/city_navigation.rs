//! Camera travel and code stops for the city world.

use bevy::input::mouse::MouseScrollUnit;

use super::*;

const PAGE: usize = 23;
const PLATFORM_SIZE: Vec3 = Vec3::new(16., 0.5, 7.);
pub(super) const STREET_EYE_HEIGHT: f32 = 1.65;

pub(super) struct Walk {
    start_height: f32,
    elapsed: f32,
    duration: f32,
}

impl Walk {
    pub(super) fn new(height: f32) -> Self {
        Self {
            start_height: height,
            elapsed: 0.,
            duration: ((height - STREET_EYE_HEIGHT).abs() / 70. + 0.4).clamp(0.4, 2.4),
        }
    }

    pub(super) fn step(
        &mut self,
        camera: &mut Transform,
        direction: Vec3,
        dt: f32,
        clear: impl Fn(Vec2) -> bool,
    ) {
        self.elapsed += dt;
        let t = (self.elapsed / self.duration).min(1.);
        camera.translation.y = self
            .start_height
            .lerp(STREET_EYE_HEIGHT, t * t * (3. - 2. * t));
        let movement = direction.xz().clamp_length_max(1.) * 4.5 * dt;
        let target = camera.translation.xz() + movement;
        if clear(target) {
            camera.translation.x = target.x;
            camera.translation.z = target.y;
        }
    }
}

#[cfg(test)]
#[test]
fn walking_lands_smoothly_then_stays_at_eye_height_and_blocks_walls() {
    let mut eye = Transform::from_xyz(0., 40., 0.);
    let mut walk = Walk::new(eye.translation.y);
    walk.step(&mut eye, Vec3::ZERO, 0.1, |_| true);
    assert!((STREET_EYE_HEIGHT..40.).contains(&eye.translation.y));
    for _ in 0..20 {
        walk.step(&mut eye, Vec3::ZERO, 0.1, |_| true);
    }
    assert_eq!(eye.translation.y, STREET_EYE_HEIGHT);
    walk.step(&mut eye, Vec3::X, 0.1, |_| false);
    assert_eq!(eye.translation.x, 0.);
    walk.step(&mut eye, Vec3::X + Vec3::Y, 0.1, |_| true);
    assert!(eye.translation.x > 0.);
    assert_eq!(eye.translation.y, STREET_EYE_HEIGHT);
}

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
    scroll: f32,
    labels: Vec<String>,
}

impl Route {
    fn scroll_step(&mut self, wheel: &AccumulatedMouseScroll) -> i32 {
        if self.kind != RouteKind::Review {
            return 0;
        }
        let delta = match wheel.unit {
            MouseScrollUnit::Pixel => wheel.delta.y / 40.,
            MouseScrollUnit::Line => wheel.delta.y,
        };
        if delta != 0. && self.scroll.signum() != delta.signum() {
            self.scroll = 0.;
        }
        self.scroll += delta;
        if self.scroll.abs() < 1. {
            return 0;
        }
        let step = self.scroll.signum() as i32;
        self.scroll = 0.;
        step
    }
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
    Facade,
    Stop,
    ReviewWall,
    ReviewLine,
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

    fn facade(start: Transform, end: Transform, repo: &Repository) -> Self {
        let mut hop = Self::new(start, end, Arrival::Facade);
        hop.roof = hop
            .roof
            .max(flight_roof(start.translation, end.translation, repo));
        let climb = hop.roof - start.translation.y.min(end.translation.y);
        hop.duration = hop.duration.max((climb / 24. + 0.65).min(3.8));
        hop
    }

    fn pose(&self, progress: f32) -> Transform {
        let t = progress.clamp(0., 1.);
        let eased = t * t * (3. - 2. * t);
        let lift = (self.start.translation.distance(self.end.translation) * 0.18).clamp(6., 34.);
        let mut translation = self.start.translation.lerp(self.end.translation, eased);
        if !matches!(self.arrival, Arrival::Stop | Arrival::ReviewWall)
            || self.start.translation.xz() != self.end.translation.xz()
        {
            translation.y += lift * (std::f32::consts::PI * eased).sin().max(0.);
        }
        let mut rotation = self.start.rotation.slerp(self.end.rotation, eased);
        if matches!(self.arrival, Arrival::Facade | Arrival::ReviewLine) {
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
    route: Option<Route>,
    hop: Option<Hop>,
    redraw: bool,
    broken: HashSet<usize>,
    overview: Option<(Overview, Transform)>,
    district_from_project: bool,
    history: History,
    review_line: Option<(usize, f32)>,
    review_face: Option<listing::Face>,
    pub overview_requested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Overview {
    District,
    Project,
}

#[derive(Default)]
struct History {
    scope: Option<(PathBuf, String, Option<String>)>,
    paths: Vec<String>,
    current: usize,
}

impl History {
    fn record(&mut self, path: &str) {
        if self
            .paths
            .get(self.current)
            .is_some_and(|saved| saved == path)
        {
            return;
        }
        self.paths.truncate(self.current + 1);
        self.paths.push(path.to_owned());
        self.current = self.paths.len() - 1;
    }

    fn step(&mut self, offset: isize) -> Option<&str> {
        let index = self.current.checked_add_signed(offset)?;
        let path = self.paths.get(index)?;
        self.current = index;
        Some(path)
    }
}

pub(super) fn history_direction(keys: &ButtonInput<KeyCode>) -> Option<isize> {
    if !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        || keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight])
    {
        return None;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        Some(-1)
    } else if keys.just_pressed(KeyCode::KeyI) {
        Some(1)
    } else {
        None
    }
}

impl Navigation {
    pub(super) fn overview(&self) -> bool {
        self.overview.is_some()
    }

    pub(super) fn overview_level(&self) -> Option<Overview> {
        self.overview.map(|(level, _)| level)
    }

    fn cycle_overview(&mut self, camera: Transform) -> (Option<Overview>, Transform) {
        let from_project = std::mem::take(&mut self.district_from_project);
        let (level, origin) = match self.overview.take() {
            None => (Some(Overview::District), camera),
            Some((Overview::District, origin)) if from_project => (None, origin),
            Some((Overview::District, origin)) => (Some(Overview::Project), origin),
            Some((Overview::Project, origin)) => (None, origin),
        };
        self.overview = level.map(|level| (level, origin));
        (level, origin)
    }

    fn overview_hop(&mut self, start: Transform, end: Transform) {
        let mut hop = Hop::new(start, end, Arrival::Overview);
        hop.duration =
            (start.translation.distance(end.translation) / 260. + 0.35).clamp(0.50, 1.05);
        self.hop = Some(hop);
    }

    pub(super) fn clear_trace_route(&mut self) {
        if self
            .route
            .as_ref()
            .is_some_and(|route| route.kind == RouteKind::Trace)
        {
            self.route = None;
            self.hop = None;
            self.redraw = true;
        }
    }

    pub(super) fn refresh_trace(&mut self) {
        if self
            .route
            .as_ref()
            .is_some_and(|route| route.kind == RouteKind::Trace)
        {
            self.redraw = true;
        }
    }

    pub(super) fn reading_rows(
        &self,
        repo: &Repository,
        selected: usize,
        camera: &Transform,
    ) -> Option<std::ops::Range<usize>> {
        if self.is_flying() {
            return None;
        }
        let route = self.route.as_ref()?;
        let Stop::Review { row, count, .. } = route.stops[route.current] else {
            return None;
        };
        let center = stop_position(route, route.current, repo, selected) + Vec3::Y * 3.;
        reading::panel_in_focus(camera, center, Vec2::new(15.8, 4.3), 20.)
            .then_some(row..row + count)
    }

    pub(super) fn guides_camera(&self) -> bool {
        self.is_flying() || self.route.is_some()
    }

    pub(super) fn is_flying(&self) -> bool {
        self.hop.is_some() || self.review_line.is_some()
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
        if let Some((level, _)) = self.overview {
            return match level {
                Overview::Project => {
                    "Click a district to approach its files · M returns to your previous view"
                }
                Overview::District if self.district_from_project => {
                    "Click a building to read its code · M returns to your previous view"
                }
                Overview::District => {
                    "Click a building to read its code · M shows the whole project"
                }
            }
            .into();
        }
        if self.hop.is_some() && self.route.is_none() {
            return "Flying to destination · Esc to cancel".into();
        }
        if let Some(route) = &self.route {
            let last = route.current + 1 == route.stops.len();
            let hint = match (route.kind, last) {
                (RouteKind::Review, true) => {
                    "Scroll down / S to go back · W finishes review · Q returns to the diff wall"
                }
                (RouteKind::Review, false) => {
                    "Scroll up / W for next · Scroll down / S for previous · Q returns to the diff wall"
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
        let trace = " · T follows imported stack trace";
        let shortcut = file_picker::SHORTCUT;
        format!(
            "Click to capture · Esc releases · Right-click opens actions · E flies to the aimed face\nScroll follows the text · Files / {shortcut} searches paths · WASD flies\nClick close to start or stop washing source · Home frames the file · G diff · B blame · C reviews changes{trace}"
        )
    }
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
            let roof = listing::block_size(&repo.files[selected]).y;
            Vec3::new(x, roof + 28. + index as f32 * 13., z)
        }
        Stop::Trace { file, .. } => {
            let [x, z] = repo.files[file].position;
            let roof = listing::block_size(&repo.files[file]).y;
            Vec3::new(x + (index % 5) as f32 * 12., roof + 28., z)
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

pub(super) fn district_view(
    district: &repo_world::District,
    repo: &Repository,
    aspect: f32,
) -> Transform {
    let min = Vec2::from_array(district.position);
    let max = min + Vec2::from_array(district.size);
    let roof = repo
        .files
        .iter()
        .filter(|file| {
            let position = Vec2::from_array(file.position);
            position.cmpge(min).all() && position.cmple(max).all()
        })
        .map(|file| listing::block_size(file).y)
        .fold(0., f32::max);
    overview_view(min, max, roof, aspect)
}

fn project_view(repo: &Repository, aspect: f32) -> Transform {
    let (min, max) = repo.districts.iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(min, max), district| {
            let position = Vec2::from_array(district.position);
            (
                min.min(position),
                max.max(position + Vec2::from_array(district.size)),
            )
        },
    );
    let roof = repo
        .files
        .iter()
        .map(|file| listing::block_size(file).y)
        .fold(0., f32::max);
    overview_view(min, max, roof, aspect)
}

fn overview_view(min: Vec2, max: Vec2, roof: f32, aspect: f32) -> Transform {
    let [x, z] = ((min + max) * 0.5).to_array();
    let [width, depth] = (max - min).to_array();
    let center = Vec3::new(x, roof, z);
    let diameter = (width / aspect.max(0.1)).max(depth) + 28.;
    let half_fov = std::f32::consts::FRAC_PI_8;
    let height = diameter * 0.5 / half_fov.tan();
    let offset = Vec3::new(0., height, height / MAX_LOOK_PITCH.tan());
    Transform::from_translation(center + offset)
        .with_rotation(Quat::from_rotation_x(-MAX_LOOK_PITCH))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeFileDestination)]
    fn browser_file_destination() -> String;
}

pub(super) fn navigate(
    (time, keys, mut trace): (
        Res<Time>,
        Res<ButtonInput<KeyCode>>,
        Option<ResMut<trace_import::Trace>>,
    ),
    wheel: Res<AccumulatedMouseScroll>,
    repo: Res<Repo>,
    mut focus: ResMut<file_facade::ChangesOnly>,
    nearest: Res<Nearest>,
    mut explorer: ResMut<Explorer>,
    mut navigation: ResMut<Navigation>,
    mut picker: ResMut<file_picker::FilePicker>,
    mut listing: ResMut<listing::Listing>,
    mut menu: ResMut<city_menu::Menu>,
    mut saved_progress: ResMut<progress::Progress>,
    search: Res<listing::find::Search>,
    mut review: Option<ResMut<review_notes::Review>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &Pilot), PlayerCamera>,
    #[cfg(not(target_arch = "wasm32"))] (mut links, mut agent_navigation): (
        Option<ResMut<file_links::Links>>,
        Option<ResMut<agent_navigation::Navigation>>,
    ),
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
    let trace_start = trace
        .as_deref_mut()
        .is_some_and(|trace| std::mem::take(&mut trace.start));
    if trace_start {
        navigation.route = None;
        navigation.hop = None;
        navigation.overview = None;
        listing.reset_view();
        focus.enabled = false;
    }

    let destination = review_notes::take_destination(review.as_deref_mut());
    #[allow(unused_mut)]
    let mut guided = destination.is_some();
    if guided {
        focus.enabled = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let destination = destination.or_else(|| {
        let destination = links
            .as_deref_mut()
            .and_then(file_links::Links::take_destination);
        if destination.is_some() {
            focus.enabled = false;
        }
        destination
    });
    #[cfg(not(target_arch = "wasm32"))]
    let destination = destination.or_else(|| {
        let destination = agent_navigation
            .as_deref_mut()
            .and_then(|navigation| navigation.take_destination(&repo.0));
        if destination.is_some() {
            focus.enabled = false;
            guided = true;
        }
        destination
    });
    if let Some(destination) = destination
        && destination.revision == repo.0.revision
        && let Some(index) = repo
            .0
            .files
            .iter()
            .position(|file| file.path == destination.anchor.path && focus.includes(file))
    {
        navigation.route = None;
        navigation.hop = None;
        navigation.overview = None;
        navigation.redraw = true;
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        if explorer.selected != index {
            explorer.open(&repo.0, index);
        }
        if explorer.before != destination.anchor.before {
            explorer.before = destination.anchor.before;
            explorer.reload(&repo.0);
        }
        explorer.rebuild = true;
        listing.reset_view();
        navigation.review_face = if guided {
            Some(listing::Face::Source)
        } else {
            None
        };
        if guided {
            listing.present_range(&explorer, destination.anchor.start, destination.anchor.end);
        }
        navigation.review_line = Some((destination.anchor.start, time.elapsed_secs() + 10.));
    }
    if let Some((line, deadline)) = navigation.review_line {
        let highlighted = explorer.highlighted.clone();
        explorer
            .source_layout
            .unfold(&highlighted, line.saturating_sub(1));
        let file = &repo.0.files[explorer.selected];
        let end = if let Some(face) = navigation.review_face {
            listing::Listing::line_view(&explorer, file, face, line)
        } else {
            listing::Listing::review_line_view(&explorer, file, line)
        };
        if let Some(end) = end {
            navigation.review_line = None;
            navigation.hop = Some(
                if camera
                    .translation
                    .xz()
                    .distance_squared(end.translation.xz())
                    < 0.04
                {
                    Hop::new(*camera, end, Arrival::ReviewWall)
                } else {
                    let mut hop = Hop::facade(*camera, end, &repo.0);
                    hop.arrival = Arrival::ReviewLine;
                    hop
                },
            );
        } else if time.elapsed_secs() >= deadline || keys.just_pressed(KeyCode::Escape) {
            navigation.review_line = None;
        }
        return;
    }

    if repo.is_changed() {
        let scope = (&repo.0.root, &repo.0.revision, &repo.0.baseline);
        if !navigation
            .history
            .scope
            .as_ref()
            .is_some_and(|(root, revision, baseline)| (root, revision, baseline) == scope)
        {
            navigation.history = History {
                scope: Some((scope.0.clone(), scope.1.clone(), scope.2.clone())),
                ..default()
            };
        }
    }
    if navigation.history.paths.is_empty()
        || (listing::Listing::visible(&explorer, &navigation)
            && saved_progress.has_visited(explorer.selected))
    {
        navigation
            .history
            .record(&repo.0.files[explorer.selected].path);
    }

    let overview =
        keys.just_pressed(KeyCode::KeyM) || std::mem::take(&mut navigation.overview_requested);
    if overview && !repo.0.districts.is_empty() {
        listing.reset_view();
        navigation.route = None;
        navigation.redraw = true;
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        explorer.rebuild = true;
        let aspect = window.width() / window.height().max(1.);
        let (level, origin) = navigation.cycle_overview(*camera);
        let end = match level {
            Some(Overview::District) => {
                let path = &repo.0.files[explorer.selected].path;
                let name = path
                    .rsplit_once('/')
                    .map_or("/", |(directory, _)| directory);
                let district = nearest
                    .district
                    .and_then(|index| repo.0.districts.get(index))
                    .or_else(|| {
                        repo.0
                            .districts
                            .iter()
                            .find(|district| district.name == name)
                    })
                    .unwrap_or(&repo.0.districts[0]);
                district_view(district, &repo.0, aspect)
            }
            Some(Overview::Project) => project_view(&repo.0, aspect),
            None => origin,
        };
        navigation.overview_hop(*camera, end);
        return;
    }

    if navigation.overview_level() == Some(Overview::Project)
        && (pilot.map_click || keys.just_pressed(KeyCode::KeyE))
        && let Some(district) = nearest
            .district
            .and_then(|index| repo.0.districts.get(index))
    {
        let origin = navigation.overview.expect("project overview").1;
        navigation.overview = Some((Overview::District, origin));
        navigation.district_from_project = true;
        let aspect = window.width() / window.height().max(1.);
        navigation.overview_hop(*camera, district_view(district, &repo.0, aspect));
        return;
    }

    let history_destination = history_direction(&keys).and_then(|direction| {
        while let Some(path) = navigation.history.step(direction) {
            if let Some(index) = repo
                .0
                .files
                .iter()
                .position(|file| file.path == path && focus.includes(file))
            {
                return Some(index);
            }
        }
        None
    });
    let requested_file = picker.destination.take().or(history_destination);
    #[cfg(target_arch = "wasm32")]
    let requested_file = requested_file.or_else(|| {
        let path = browser_file_destination();
        (!path.is_empty())
            .then(|| repo.0.files.iter().position(|file| file.path == path))
            .flatten()
    });
    if requested_file.is_some() {
        navigation.hop = None;
        navigation.route = None;
        navigation.redraw = true;
    }

    let reviewing = navigation
        .route
        .as_ref()
        .is_some_and(|route| route.kind == RouteKind::Review);
    if reviewing && keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if reviewing && keys.just_pressed(KeyCode::KeyQ) {
        let route = navigation.route.take().expect("review route");
        let Stop::Review { row, .. } = route.stops[route.current] else {
            unreachable!()
        };
        let file = &repo.0.files[explorer.selected];
        let end = listing::Listing::diff_view(&explorer, file, row);
        navigation.hop = Some(Hop::new(*camera, end, Arrival::ReviewWall));
        navigation.redraw = true;
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
                        if explorer.before {
                            explorer.before = false;
                            explorer.reload(&repo.0);
                        }
                        explorer.page = (line - 1) / PAGE;
                        explorer.rebuild = true;
                    }
                    navigation.redraw = true;
                }
                Arrival::Facade | Arrival::Overview | Arrival::ReviewWall | Arrival::ReviewLine => {
                }
            }
        }
        return;
    }

    if let Some(route) = &mut navigation.route {
        let forward = keys.just_pressed(KeyCode::KeyW);
        let backward = keys.just_pressed(KeyCode::KeyS);
        let scroll = route.scroll_step(&wheel);
        let at_end = route.current + 1 == route.stops.len();
        if forward && at_end {
            if route.kind == RouteKind::Review {
                navigation.route = None;
                navigation.redraw = true;
                let mut end = *camera;
                comparison_scene::focus(&mut end, &repo.0, &explorer);
                navigation.hop = Some(Hop::new(*camera, end, Arrival::Comparison));
            }
        } else if forward || backward || scroll != 0 {
            let old = route.current;
            route.current = if forward || (!backward && scroll > 0) {
                (old + 1).min(route.stops.len() - 1)
            } else {
                old.saturating_sub(1)
            };
            if old != route.current {
                fly_to_stop(&mut navigation, *camera, &repo.0, explorer.selected);
            }
        }
        return;
    }

    if keys.just_pressed(KeyCode::KeyC) || keys.just_pressed(KeyCode::KeyT) || trace_start {
        let following_trace = keys.just_pressed(KeyCode::KeyT) || trace_start;
        let (kind, stops, labels) = if following_trace {
            focus.enabled = false;
            let (stops, labels) = trace.as_ref().map_or_else(
                || (Vec::new(), Vec::new()),
                |trace| {
                    trace
                        .frames()
                        .filter_map(|frame| {
                            let file =
                                repo.0.files.iter().position(|file| {
                                    file.path == frame.path && file.after.is_some()
                                })?;
                            let label = frame.span.as_ref().map_or_else(String::new, |span| {
                                let name = span.name.chars().take(32).collect::<String>();
                                format!(
                                    "{name}{}{}",
                                    span.duration_ms
                                        .map_or_else(String::new, |duration| format!(
                                            " · {duration:.3} ms"
                                        )),
                                    if span.error { " · error" } else { "" }
                                )
                            });
                            Some((
                                Stop::Trace {
                                    file,
                                    line: frame.line,
                                },
                                label,
                            ))
                        })
                        .unzip()
                },
            );
            (RouteKind::Trace, stops, labels)
        } else {
            (
                RouteKind::Review,
                explorer
                    .comparison
                    .as_ref()
                    .map_or_else(Vec::new, review_stops),
                Vec::new(),
            )
        };
        if stops.is_empty() {
            explorer.notice = if following_trace {
                "Open VibePad → Trace to paste a stack trace."
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
            scroll: 0.,
            labels,
        });
        fly_to_stop(&mut navigation, *camera, &repo.0, explorer.selected);
        return;
    }
    let next_changed = keys.just_pressed(KeyCode::KeyN) && !search.owns_next();
    let distant_shot = ((pilot.shot || pilot.map_click) && !listing.water_on())
        .then_some(nearest.file)
        .flatten()
        .filter(|aim| {
            let file = &repo.0.files[aim.index];
            let size = listing::block_size(file);
            nearest.near_source != Some(aim.index)
                && !aim.within_tool_reach(file, size, camera.translation, nearest.file_distance)
        });
    let target = if let Some(aim) = menu.destination.take() {
        Some((aim.index, Some(aim)))
    } else if let Some(file) = requested_file {
        Some((file, None))
    } else if navigation.overview() && (distant_shot.is_some() || keys.just_pressed(KeyCode::KeyE))
    {
        nearest.file.map(|aim| (aim.index, None))
    } else if next_changed {
        (1..repo.0.files.len())
            .map(|offset| (explorer.selected + offset) % repo.0.files.len())
            .find(|&index| repo.0.files[index].change != Change::Unchanged)
            .map(|index| (index, None))
    } else if let Some(aim) = distant_shot {
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
    if let Some((index, aimed)) = target.filter(|(index, _)| focus.includes(&repo.0.files[*index]))
    {
        let from_overview = navigation.overview.take().is_some();
        listing.reset_view();
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        if index != explorer.selected {
            explorer.open(&repo.0, index);
        }
        explorer.rebuild = true;
        let file = &repo.0.files[index];
        navigation.history.record(&file.path);
        let size = listing::block_size(file);
        let end = listing.arrival_view(&explorer, file, size, aimed, camera.translation);
        if camera.translation.distance_squared(end.translation) < 0.04
            && camera.rotation.dot(end.rotation).abs() > 0.9999
        {
            return;
        }
        let mut hop = Hop::facade(*camera, end, &repo.0);
        if from_overview {
            hop.duration = hop.duration.min(1.65);
        }
        navigation.hop = Some(hop);
    } else if !explorer.anchored
        && explorer.comparison_mode == 0
        && let Some(index) = nearest
            .near_source
            .filter(|&index| index != explorer.selected && focus.includes(&repo.0.files[index]))
    {
        listing.reset_view();
        explorer.open(&repo.0, index);
    }
}

fn flight_roof(start: Vec3, end: Vec3, repo: &Repository) -> f32 {
    let origin = Vec3::new(start.x, 0., start.z);
    let offset = Vec3::new(end.x, 0., end.z) - origin;
    let distance = offset.length();
    let direction = offset.try_normalize().unwrap_or(Vec3::ZERO);

    repo.files
        .iter()
        .filter_map(|file| {
            let [x, z] = file.position;
            let size = listing::block_size(file);
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
    if navigation.overview_level() == Some(Overview::Project)
        && let Some(district) = nearest
            .district
            .and_then(|index| repo.0.districts.get(index))
    {
        let [x, z] = district.position;
        let [width, depth] = district.size;
        let corners = [
            Vec3::new(x, 0.3, z),
            Vec3::new(x + width, 0.3, z),
            Vec3::new(x + width, 0.3, z + depth),
            Vec3::new(x, 0.3, z + depth),
        ];
        for edge in 0..4 {
            gizmos.line(corners[edge], corners[(edge + 1) % 4], color(set.accent));
        }
    }
    if !navigation.guides_camera()
        && !explorer.anchored
        && explorer.comparison_mode == 0
        && let Some(aim) = nearest.file
    {
        let index = aim.index;
        let file = &repo.0.files[index];
        let [x, z] = file.position;
        let size = listing::block_size(file);
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
                if let Some(label) = route.labels.get(index).filter(|label| !label.is_empty()) {
                    title = format!("Frame {number} / {label} / {path}:{line}");
                }
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn project_overview_click_reaches_an_unloaded_district_and_keeps_return_view() {
        let mut repo = repository_picker::empty();
        repo.files = ["near/current.rs", "far/target.rs"]
            .into_iter()
            .enumerate()
            .map(|(index, path)| repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [5010. * index as f32, 5010. * index as f32],
            })
            .collect();
        repo.districts = ["near", "far"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| repo_world::District {
                name: name.into(),
                position: [5000. * index as f32, 5000. * index as f32],
                size: [20., 20.],
            })
            .collect();
        let origin = Transform::from_xyz(10., 3., 30.);
        let mut navigation = Navigation::default();
        navigation.cycle_overview(origin);
        navigation.cycle_overview(origin);
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .insert_resource(navigation)
            .init_resource::<Nearest>()
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<listing::Listing>()
            .init_resource::<listing::find::Search>()
            .init_resource::<progress::Progress>()
            .init_resource::<review_notes::Review>()
            .add_systems(Update, (flight, targeting, navigate).chain());
        let mut window = Window::default();
        window.set_cursor_position(Some(Vec2::new(640., 360.)));
        app.world_mut()
            .spawn((window, PrimaryWindow, CursorOptions::default()));
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(5010., 2500., 5010.).looking_to(Vec3::NEG_Y, Vec3::Z),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        app.update();
        assert_eq!(app.world().resource::<Nearest>().district, Some(1));
        assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
        assert_eq!(
            app.world_mut().query::<&Facade>().iter(app.world()).count(),
            0
        );
        let map_pose = *app.world().get::<Transform>(camera).unwrap();
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x = 9000.;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Nearest>().district, None);
        assert_eq!(
            app.world().resource::<Navigation>().overview_level(),
            Some(Overview::Project)
        );
        assert!(!app.world().resource::<Navigation>().is_flying());
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = map_pose;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let navigation = app.world().resource::<Navigation>();
        assert_eq!(navigation.overview_level(), Some(Overview::District));
        assert_eq!(navigation.overview.unwrap().1, origin);
        assert!(navigation.hop.as_ref().unwrap().end.translation.x > 5000.);
        assert_eq!(app.world().resource::<Explorer>().selected, 0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset(MouseButton::Left);
        app.world_mut()
            .resource_mut::<Navigation>()
            .overview_requested = true;
        app.update();
        let navigation = app.world().resource::<Navigation>();
        assert!(!navigation.overview());
        assert_eq!(navigation.hop.as_ref().unwrap().end, origin);
        app.world_mut().resource_mut::<Navigation>().hop = None;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let pilot = app.world().get::<Pilot>(camera).unwrap();
        assert!(pilot.captured);
        assert!(!pilot.map_click);
        assert!(!pilot.shot);
    }

    #[test]
    fn overview_cycles_through_district_and_project_and_preserves_origin() {
        let origin = Transform::from_xyz(10., 20., 30.);
        let moved = Transform::from_xyz(40., 50., 60.);
        let mut navigation = Navigation::default();
        assert_eq!(
            navigation.cycle_overview(origin),
            (Some(Overview::District), origin)
        );
        assert_eq!(
            navigation.cycle_overview(moved),
            (Some(Overview::Project), origin)
        );
        assert_eq!(navigation.cycle_overview(moved), (None, origin));
        assert!(!navigation.overview());
    }

    #[test]
    fn jump_history_goes_back_and_forward_and_replaces_forward_branch() {
        let mut history = History::default();
        for file in ["a.rs", "b.rs", "c.rs"] {
            history.record(file);
        }
        assert_eq!(history.step(-1), Some("b.rs"));
        history.record("b.rs");
        assert_eq!(history.step(-1), Some("a.rs"));
        assert_eq!(history.step(-1), None);
        assert_eq!(history.step(1), Some("b.rs"));
        history.record("d.rs");
        assert_eq!(history.step(1), None);
        assert_eq!(history.step(-1), Some("b.rs"));
        assert_eq!(history.step(1), Some("d.rs"));
    }

    #[test]
    fn overview_frames_all_roofs_in_wide_and_narrow_windows() {
        let district = repo_world::District {
            name: "src".into(),
            position: [100., 100.],
            size: [175., 120.],
        };
        let mut repo = repository_picker::empty();
        repo.districts.push(district.clone());
        repo.files.push(repo_world::File {
            path: "src/large.rs".into(),
            before: None,
            after: Some(repo_world::Blob {
                id: "large".into(),
                size: 200_000,
                mode: "100644".into(),
            }),
            change: Change::Unchanged,
            position: [180., 160.],
        });
        let roof_height = listing::block_size(&repo.files[0]).y;
        assert!(roof_height > 1000.);
        for aspect in [0.6, 1., 1.78] {
            for eye in [
                district_view(&district, &repo, aspect),
                project_view(&repo, aspect),
            ] {
                assert!((eye.rotation.to_euler(EulerRot::YXZ).1 + MAX_LOOK_PITCH).abs() < 0.00001);
                let half_fov = std::f32::consts::FRAC_PI_8.tan();
                for x in [100., 275.] {
                    for z in [100., 220.] {
                        let roof = Vec3::new(x, roof_height, z);
                        let local = eye.rotation.inverse() * (roof - eye.translation);
                        assert!(local.y.abs() < -local.z * half_fov);
                        assert!(local.x.abs() < -local.z * half_fov * aspect);
                    }
                }
            }
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
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Nearest>()
            .init_resource::<Navigation>()
            .init_resource::<file_facade::ChangesOnly>()
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
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Navigation>()
            .init_resource::<file_facade::ChangesOnly>()
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
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(KeyCode::KeyE);
        let origin = *app.world().get::<Transform>(camera).unwrap();
        {
            let mut navigation = app.world_mut().resource_mut::<Navigation>();
            navigation.hop = None;
            navigation.overview = Some((Overview::District, origin));
        }
        {
            let mut pilot = app.world_mut().get_mut::<Pilot>(camera).unwrap();
            pilot.captured = false;
            pilot.shot = false;
            pilot.map_click = true;
        }
        app.update();
        assert_eq!(app.world().resource::<Explorer>().selected, 1);
        assert!(!app.world().resource::<Navigation>().overview());
        assert!(app.world().resource::<Navigation>().is_flying());
        assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
    }

    #[test]
    fn approach_clears_roofs_and_ends_at_the_reading_view() {
        let start = Transform::from_xyz(0., 5., 10.);
        let end = Transform::from_xyz(100., 8., 15.).looking_to(Vec3::NEG_X, Vec3::Y);
        let hop = Hop::new(start, end, Arrival::Facade);
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
    fn review_platforms_stack_and_the_camera_does_not_overshoot() {
        let mut repo = repository_picker::empty();
        repo.files.push(repo_world::File {
            path: "test.rs".into(),
            before: None,
            after: None,
            change: Change::Modified,
            position: [31., -19.],
        });
        let route = Route {
            kind: RouteKind::Review,
            stops: (0..2)
                .map(|row| Stop::Review {
                    hunk: 0,
                    row,
                    count: 1,
                })
                .collect(),
            current: 0,
            scroll: 0.,
            labels: Vec::new(),
        };
        let lower = stop_position(&route, 0, &repo, 0);
        let upper = stop_position(&route, 1, &repo, 0);
        assert_eq!(lower.xz(), upper.xz());
        assert!(lower.y < upper.y);
        let hop = Hop::new(
            Transform::from_translation(upper),
            Transform::from_translation(lower),
            Arrival::Stop,
        );
        assert!((lower.y..upper.y).contains(&hop.pose(0.2).translation.y));
        assert!((lower.y..upper.y).contains(&hop.pose(0.8).translation.y));
    }

    #[test]
    fn scroll_moves_between_review_stops_and_stays_on_the_last_stop() {
        let mut repo = repository_picker::empty();
        repo.files.push(repo_world::File {
            path: "test.rs".into(),
            before: None,
            after: None,
            change: Change::Modified,
            position: [31., -19.],
        });
        let route = Route {
            kind: RouteKind::Review,
            stops: (0..2)
                .map(|hunk| Stop::Review {
                    hunk,
                    row: hunk,
                    count: 1,
                })
                .collect(),
            current: 0,
            scroll: 0.,
            labels: Vec::new(),
        };
        let start = stop_position(&route, 0, &repo, 0) + Vec3::new(0., 5., 15.);
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .insert_resource(Navigation {
                route: Some(route),
                ..default()
            })
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Nearest>()
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<listing::Listing>()
            .init_resource::<listing::find::Search>()
            .init_resource::<progress::Progress>()
            .add_systems(Update, navigate);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.world_mut().spawn((
            Camera3d::default(),
            Transform::from_translation(start),
            Pilot::default(),
        ));
        let mut scroll = |delta| {
            let mut input = app.world_mut().resource_mut::<AccumulatedMouseScroll>();
            input.unit = bevy::input::mouse::MouseScrollUnit::Pixel;
            input.delta.y = delta;
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(1));
            app.update();
            app.world()
                .resource::<Navigation>()
                .route
                .as_ref()
                .unwrap()
                .current
        };
        assert_eq!(scroll(18.), 0);
        assert_eq!(scroll(24.), 1);
        assert_eq!(scroll(120.), 1);
        assert_eq!(scroll(120.), 1);
        assert_eq!(scroll(-120.), 0);
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
                scroll: 0.,
                labels: Vec::new(),
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
