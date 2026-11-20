//! City roads project verified source-file imports; extraction stays in repo-world.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use repo_world::links::{FileLinks, Index};

use super::*;

#[derive(Clone, PartialEq, Eq)]
struct Focus {
    root: PathBuf,
    revision: String,
    baseline: Option<String>,
    path: String,
    before: bool,
    generation: u64,
}

#[derive(Resource, Default)]
pub(super) struct Links {
    cache: Arc<Mutex<Index>>,
    snapshot: Option<Arc<Repository>>,
    focus: Option<Focus>,
    pending: Option<(Focus, native_github::Job<FileLinks>)>,
    generation: u64,
    result: Option<Result<FileLinks, String>>,
    discovered: HashMap<String, FileLinks>,
    destination: Option<review_notes::Destination>,
    version: u64,
    roads_version: u64,
    rendered: u64,
}

impl Links {
    pub(super) fn file(&self) -> Option<&FileLinks> {
        self.result.as_ref()?.as_ref().ok()
    }

    /// Changes only when the focused file, load state, or relationship results change.
    pub(super) fn version(&self) -> u64 {
        self.version
    }

    pub(super) fn status(&self) -> &str {
        match &self.result {
            None if self.focus.is_some() => "Loading file connections…",
            None => "Focus a file to see its connections.",
            Some(Err(error)) => error,
            Some(Ok(file)) if !file.supported => {
                "Import extraction is unavailable for this language."
            }
            Some(Ok(file)) if file.partial => {
                "Syntax errors present; showing recognized imports only."
            }
            Some(Ok(file)) if file.links.is_empty() => {
                "No module-level imports found in this file."
            }
            Some(Ok(file)) if file.truncated => {
                "Showing a bounded set of imports; open a linked file to continue."
            }
            Some(Ok(_)) => "",
        }
    }

    /// Queue a normal source-line flight. The tablet closes after a successful request.
    pub(super) fn visit(&mut self, index: usize) -> bool {
        let Some(focus) = &self.focus else {
            return false;
        };
        let Some(destination) = self
            .file()
            .and_then(|file| file.links.get(index))
            .and_then(|link| link.destination.as_ref())
        else {
            return false;
        };
        self.destination = Some(review_notes::Destination {
            revision: focus.revision.clone(),
            anchor: review_notes::Anchor {
                path: destination.path.clone(),
                before: focus.before,
                start: destination.line,
                end: destination.line,
            },
        });
        true
    }

    pub(super) fn take_destination(&mut self) -> Option<review_notes::Destination> {
        self.destination.take()
    }
}

/// Metadata comparison stays on the game thread; parsing and Git reads run in one worker.
pub(super) fn update(repo: Res<Repo>, explorer: Res<Explorer>, mut links: ResMut<Links>) {
    let snapshot_changed = repo.is_changed();
    let side_changed = links
        .focus
        .as_ref()
        .is_some_and(|focus| focus.before != explorer.before);
    if snapshot_changed || side_changed {
        links.discovered.clear();
        links.version = links.version.wrapping_add(1);
        links.roads_version = links.roads_version.wrapping_add(1);
    }
    if snapshot_changed {
        links.generation = links.generation.wrapping_add(1);
        links.snapshot = Some(Arc::new(repo.0.clone()));
    }
    let focus = repo.0.files.get(explorer.selected).map(|file| Focus {
        root: repo.0.root.clone(),
        revision: repo.0.revision.clone(),
        baseline: repo.0.baseline.clone(),
        path: file.path.clone(),
        before: explorer.before,
        generation: links.generation,
    });
    if focus != links.focus {
        links.result = focus
            .as_ref()
            .and_then(|focus| links.discovered.get(&focus.path).cloned())
            .map(Ok);
        links.focus = focus;
        links.destination = None;
        links.version = links.version.wrapping_add(1);
    }
    if let Some(result) = links.pending.as_ref().and_then(|(_, job)| job.take()) {
        let (completed, _) = links.pending.take().unwrap();
        // A finished inspection still belongs to this city's graph after a
        // flight, but never after a snapshot or comparison-side change.
        if let Ok(file) = &result
            && links.focus.as_ref().is_some_and(|focus| {
                focus.generation == completed.generation
                    && focus.root == completed.root
                    && focus.before == completed.before
            })
        {
            links.discovered.insert(file.path.clone(), file.clone());
            links.roads_version = links.roads_version.wrapping_add(1);
        }
        if Some(&completed) == links.focus.as_ref() {
            links.result = Some(result);
            links.version = links.version.wrapping_add(1);
        }
    }
    if links.pending.is_none()
        && links.result.is_none()
        && let Some(focus) = links.focus.clone()
    {
        let snapshot = links
            .snapshot
            .get_or_insert_with(|| Arc::new(repo.0.clone()))
            .clone();
        let index = explorer.selected;
        let before = explorer.before;
        let cache = links.cache.clone();
        links.pending = Some((
            focus,
            native_github::Job::spawn(move || {
                cache
                    .lock()
                    .map_err(|_| "Import cache is unavailable.".to_owned())?
                    .inspect(&snapshot, index, before)
            }),
        ));
    }
}

#[derive(Component)]
pub(super) struct Road;

#[derive(Default)]
pub(super) struct Residents {
    entities: HashMap<Entity, usize>,
    files: HashMap<usize, usize>,
    interested: HashSet<usize>,
}

impl Residents {
    fn add(&mut self, entity: Entity, file: usize) -> bool {
        if self.entities.insert(entity, file).is_some() {
            return false;
        }
        let count = self.files.entry(file).or_default();
        *count += 1;
        *count == 1
    }

    fn remove(&mut self, entity: Entity) -> Option<usize> {
        let file = self.entities.remove(&entity)?;
        let count = self.files.get_mut(&file)?;
        *count -= 1;
        if *count != 0 {
            return None;
        }
        self.files.remove(&file);
        Some(file)
    }
}

fn road_points(from: Vec2, to: Vec2, width: f32) -> [Vec3; 4] {
    // Leave buildings through the street-facing edge instead of drawing under their centers.
    let delta = to - from;
    let along_x = delta.x.abs() >= delta.y.abs();
    let axis = if along_x { Vec2::X } else { Vec2::Y };
    let direction = if delta.dot(axis) >= 0. { 1. } else { -1. };
    let start = from + axis * direction * (width / 2. + 0.4);
    let end = to - axis * direction * (width / 2. + 0.4);
    let middle = (start + end) / 2.;
    let corners = if along_x {
        [Vec2::new(middle.x, start.y), Vec2::new(middle.x, end.y)]
    } else {
        [Vec2::new(start.x, middle.y), Vec2::new(end.x, middle.y)]
    };
    [
        Vec3::new(start.x, 0.055, start.y),
        Vec3::new(corners[0].x, 0.055, corners[0].y),
        Vec3::new(corners[1].x, 0.055, corners[1].y),
        Vec3::new(end.x, 0.055, end.y),
    ]
}

/// Static road meshes are rebuilt only when relationship results change.
pub(super) fn draw(
    mut commands: Commands,
    repo: Res<Repo>,
    mut links: ResMut<Links>,
    filter: Res<file_facade::ChangesOnly>,
    old: Query<Entity, With<Road>>,
    mut previous_filter: Local<Option<bool>>,
    added: Query<(Entity, &FileBuilding), Added<FileBuilding>>,
    mut removed: RemovedComponents<FileBuilding>,
    mut residents: Local<Residents>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let filter_changed = previous_filter.replace(filter.enabled) != Some(filter.enabled);
    let mut residency_changed = false;
    for entity in removed.read() {
        if let Some(file) = residents.remove(entity) {
            residency_changed |= residents.interested.contains(&file);
        }
    }
    for (entity, building) in &added {
        if residents.add(entity, building.0) {
            residency_changed |= residents.interested.contains(&building.0);
        }
    }
    if links.rendered != links.roads_version {
        let paths = links
            .discovered
            .values()
            .flat_map(|file| {
                std::iter::once(file.path.as_str()).chain(file.links.iter().filter_map(|link| {
                    link.destination.as_ref().map(|target| target.path.as_str())
                }))
            })
            .collect::<HashSet<_>>();
        residents.interested = repo
            .0
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| paths.contains(file.path.as_str()).then_some(index))
            .collect();
    }
    if links.rendered == links.roads_version && !filter_changed && !residency_changed {
        return;
    }
    links.rendered = links.roads_version;
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let mut roads = HashSet::new();
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.43, 0.45),
        unlit: true,
        ..default()
    });
    let files = repo
        .0
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.as_str(), (index, file)))
        .collect::<HashMap<_, _>>();
    for file in links.discovered.values() {
        let Some(&(from_index, from)) = files.get(file.path.as_str()) else {
            continue;
        };
        if !filter.includes(from) || !residents.files.contains_key(&from_index) {
            continue;
        }
        for target in file
            .links
            .iter()
            .filter_map(|link| link.destination.as_ref())
        {
            let Some(&(to_index, to)) = files.get(target.path.as_str()) else {
                continue;
            };
            if to_index == from_index
                || !filter.includes(to)
                || !residents.files.contains_key(&to_index)
                || !roads.insert((from_index.min(to_index), from_index.max(to_index)))
            {
                continue;
            }
            let points = road_points(
                Vec2::from_array(from.position),
                Vec2::from_array(to.position),
                listing::block_size(from).x.max(listing::block_size(to).x),
            );
            for [start, end] in points.windows(2).map(|points| [points[0], points[1]]) {
                let length = start.distance(end);
                if length < 0.01 {
                    continue;
                }
                commands.spawn((
                    Road,
                    Mesh3d(meshes.add(Cuboid::new(0.9, 0.035, length))),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation((start + end) / 2.).looking_at(end, Vec3::Y),
                    bevy::light::NotShadowCaster,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road_app() -> App {
        let mut repo = repository_picker::empty();
        repo.files = ["entry.txt", "detail.txt", "other.txt"]
            .into_iter()
            .enumerate()
            .map(|(index, path)| repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Added,
                position: [index as f32 * 50., 0.],
            })
            .collect();
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .init_resource::<Links>()
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, (update, draw).chain());
        for index in 0..3 {
            app.world_mut().spawn(FileBuilding(index));
        }
        app.update();
        app
    }

    fn deliver_links(app: &mut App, path: &str, destination: Option<&str>) {
        let file = FileLinks {
            path: path.into(),
            supported: true,
            links: destination
                .map(|path| repo_world::links::Link {
                    line: 1,
                    import: "mod detail".into(),
                    symbol: None,
                    destination: Some(repo_world::links::Destination {
                        path: path.into(),
                        line: 1,
                        exact: false,
                    }),
                    reason: None,
                })
                .into_iter()
                .collect(),
            ..Default::default()
        };
        deliver_result(app, Ok(file));
    }

    fn deliver_result(app: &mut App, result: Result<FileLinks, String>) {
        let mut links = app.world_mut().resource_mut::<Links>();
        let focus = links.focus.clone().unwrap();
        links.result = None;
        links.pending = Some((focus, native_github::Job::spawn(move || result)));
        drop(links);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<Links>().result.is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "link worker timed out"
            );
            app.update();
            std::thread::yield_now();
        }
    }

    fn road_count(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<Entity, With<Road>>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn discovered_roads_remain_when_flying_to_a_file_without_imports() {
        // Given a resolved road, leaving its source must not erase it while the
        // next file loads, finishes without imports, or fails inspection.
        let mut app = road_app();
        deliver_links(&mut app, "entry.txt", Some("detail.txt"));
        let discovered = road_count(&mut app);
        assert!(discovered > 0);
        app.world_mut().resource_mut::<Explorer>().selected = 2;
        app.update();
        assert_eq!(
            road_count(&mut app),
            discovered,
            "road vanished on departure"
        );
        deliver_links(&mut app, "other.txt", None);
        assert_eq!(road_count(&mut app), discovered);
        app.world_mut().resource_mut::<Explorer>().selected = 1;
        app.update();
        deliver_result(&mut app, Err("Source unavailable".into()));
        assert_eq!(road_count(&mut app), discovered);
    }

    #[test]
    fn exploring_more_files_accumulates_roads_without_duplicate_reverse_links() {
        let mut app = road_app();
        deliver_links(&mut app, "entry.txt", Some("detail.txt"));
        let first = road_count(&mut app);
        app.world_mut().resource_mut::<Explorer>().selected = 1;
        app.update();
        deliver_links(&mut app, "detail.txt", Some("other.txt"));
        assert_eq!(road_count(&mut app), first * 2);
        app.world_mut().resource_mut::<Explorer>().selected = 2;
        app.update();
        deliver_links(&mut app, "other.txt", Some("detail.txt"));
        assert_eq!(road_count(&mut app), first * 2);
        app.world_mut().resource_mut::<Explorer>().selected = 0;
        app.update();
        let links = app.world().resource::<Links>();
        assert_eq!(links.file().unwrap().path, "entry.txt");
        assert!(
            links.pending.is_none(),
            "known file should not be parsed again"
        );
        assert_eq!(road_count(&mut app), first * 2);
    }

    #[test]
    fn discovered_roads_do_not_cross_snapshot_or_comparison_side_boundaries() {
        for change_side in [false, true] {
            let mut app = road_app();
            deliver_links(&mut app, "entry.txt", Some("detail.txt"));
            assert!(road_count(&mut app) > 0);
            if change_side {
                app.world_mut().resource_mut::<Explorer>().before = true;
            } else {
                app.world_mut()
                    .resource_mut::<Repo>()
                    .0
                    .revision
                    .push_str("-new");
            }
            app.update();
            assert_eq!(
                road_count(&mut app),
                0,
                "stale road survived a snapshot change"
            );
        }
    }

    #[test]
    fn roads_stay_above_street_and_connect_distinct_edges() {
        let points = road_points(Vec2::ZERO, Vec2::new(50., 40.), 10.);
        assert!(points.iter().all(|point| point.y > 0.));
        assert_eq!(points[0].x, 5.4);
        assert_eq!(points[3].x, 44.6);
        assert_eq!(points[0].z, 0.);
        assert_eq!(points[3].z, 40.);
    }

    #[test]
    fn same_column_roads_leave_the_building_ends_instead_of_crossing_their_footprints() {
        let points = road_points(Vec2::ZERO, Vec2::new(0., 50.), 10.);
        assert!(points.iter().all(|point| point.x == 0.));
        assert_eq!(points[0].z, 5.4);
        assert_eq!(points[3].z, 44.6);
    }

    #[test]
    fn road_endpoint_stays_resident_until_its_last_building_part_is_removed() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let mut residents = Residents::default();
        assert!(residents.add(first, 7));
        assert!(!residents.add(second, 7));
        assert_eq!(residents.remove(first), None);
        assert!(residents.files.contains_key(&7));
        assert_eq!(residents.remove(second), Some(7));
        assert!(!residents.files.contains_key(&7));
    }

    #[test]
    fn focused_file_changes_reuse_metadata_until_the_repository_snapshot_changes() {
        let mut repo = repository_picker::empty();
        repo.files = ["first.txt", "second.txt"]
            .map(|path| repo_world::File {
                path: path.into(),
                before: None,
                after: None,
                change: Change::Added,
                position: [0., 0.],
            })
            .into();
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .init_resource::<Links>()
            .add_systems(Update, update);
        app.update();
        let snapshot = app.world().resource::<Links>().snapshot.clone().unwrap();
        app.world_mut().resource_mut::<Explorer>().selected = 1;
        app.update();
        assert!(Arc::ptr_eq(
            &snapshot,
            app.world().resource::<Links>().snapshot.as_ref().unwrap()
        ));
        app.world_mut()
            .resource_mut::<Repo>()
            .0
            .revision
            .push_str("-new");
        app.update();
        assert!(!Arc::ptr_eq(
            &snapshot,
            app.world().resource::<Links>().snapshot.as_ref().unwrap()
        ));
    }
}
