//! Refresh the native city from coalesced filesystem events.
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

use notify::{EventKind, RecursiveMode, Watcher};

use super::*;

const BATCH: Duration = Duration::from_millis(160);
const GROWTH: f64 = 0.42;
const HIGHLIGHT: f64 = 30.;

#[derive(Resource, Default)]
pub(super) struct Live {
    session: Option<Session>,
    pub(super) growth: HashMap<usize, Vec3>,
    pub(super) growing_until: f64,
    pub(super) keep_camera: bool,
    pub(super) refreshed_files: Option<HashSet<usize>>,
    touched: HashMap<String, f64>,
    last_touched: Option<(String, f64)>,
    focus_requested: bool,
    status: String,
}

impl Live {
    pub(super) fn touched_at(&self, path: &str) -> Option<f64> {
        self.touched.get(path).copied()
    }

    pub(super) fn last_touched(&self, now: f64) -> Option<(&str, u64)> {
        let (path, time) = self.last_touched.as_ref()?;
        Some((path, (now - time).max(0.) as u64))
    }
}

pub(super) fn focus_latest(
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    review: Res<review_notes::Review>,
    menu: Res<city_menu::Menu>,
    search: Res<listing::find::Search>,
    visibility: Res<file_facade::ChangesOnly>,
    mut picker: ResMut<file_picker::FilePicker>,
    mut live: ResMut<Live>,
) {
    let requested = std::mem::take(&mut live.focus_requested) || keys.just_pressed(KeyCode::KeyL);
    if requested
        && repo.0.is_worktree()
        && !picker.is_open()
        && !review.blocked
        && !menu.is_open()
        && !search.blocks_controls()
        && let Some((path, _)) = live.last_touched.as_ref()
        && let Some(index) = repo
            .0
            .files
            .iter()
            .position(|file| &file.path == path && visibility.includes(file))
    {
        picker.destination = Some(index);
    }
}

pub(super) fn request_focus(_: On<bevy::ui_widgets::Activate>, mut live: ResMut<Live>) {
    live.focus_requested = true;
}

struct Session {
    root: PathBuf,
    updates: Mutex<Receiver<Result<Repository, String>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Session {
    fn start(root: PathBuf) -> Self {
        let (send, updates) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let folder = root.clone();
        thread::spawn(move || {
            let (event, events) = mpsc::sync_channel(1);
            let watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    if result.is_err()
                        || result.is_ok_and(|event| !matches!(event.kind, EventKind::Access(_)))
                    {
                        let _ = event.try_send(());
                    }
                })
                .and_then(|mut watcher| {
                    watcher.watch(&folder, RecursiveMode::Recursive)?;
                    Ok(watcher)
                });
            let _watcher = match watcher {
                Ok(watcher) => watcher,
                Err(error) => {
                    let _ = send.send(Err(format!("Could not watch the worktree: {error}")));
                    return;
                }
            };
            loop {
                if send.send(Repository::open_worktree(&folder)).is_err() {
                    return;
                }
                loop {
                    if worker_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if events.recv_timeout(BATCH).is_ok() {
                        break;
                    }
                }
                thread::sleep(BATCH);
                let _ = events.try_recv();
            }
        });
        Self {
            root,
            updates: Mutex::new(updates),
            stop,
        }
    }
}

fn changed(before: &repo_world::File, after: &repo_world::File) -> bool {
    before.before != after.before || before.after != after.after
}

fn refreshed_files(previous: &Repository, next: &Repository) -> Option<HashSet<usize>> {
    if previous.revision != next.revision || previous.files.len() != next.files.len() {
        return None;
    }
    let mut files = HashSet::new();
    for (index, (old, new)) in previous.files.iter().zip(&next.files).enumerate() {
        if old.path != new.path {
            return None;
        }
        if changed(old, new) {
            files.insert(index);
        }
    }
    Some(files)
}

fn keep_layout(previous: &Repository, next: &mut Repository) {
    let positions = previous
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.position))
        .collect::<HashMap<_, _>>();
    let previous_districts = previous.districts.iter().cloned();
    next.districts = previous_districts.collect();
    // A growing district could displace a building that is already under the camera.
    const WIDTH: f32 = 27.;
    const DEPTH: f32 = 30.;
    let edge = next
        .districts
        .iter()
        .map(|district| district.position[0] + district.size[0])
        .fold(0., f32::max);
    let first = edge + 17.;
    let mut column = 0;
    for file in &mut next.files {
        if let Some(position) = positions.get(file.path.as_str()) {
            file.position = *position;
        } else {
            file.position = [first + (column as f32 + 0.5) * WIDTH, DEPTH / 2.];
            column += 1;
        }
    }
    if column > 0 {
        next.districts.push(repo_world::District {
            name: "New files".into(),
            position: [first, 0.],
            size: [column as f32 * WIDTH, DEPTH],
        });
    }
}

pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    listing: Res<listing::Listing>,
    old_city: repository_picker::CityEntities,
    mut live: ResMut<Live>,
) {
    if !repo.0.is_worktree() {
        if live.session.is_some() {
            *live = Live::default();
        }
        return;
    }
    if live
        .session
        .as_ref()
        .is_none_or(|watch| watch.root != repo.0.root)
    {
        *live = Live {
            session: Some(Session::start(repo.0.root.clone())),
            ..default()
        };
    }
    let Some(result) = live
        .session
        .as_ref()
        .and_then(|watch| watch.updates.lock().ok()?.try_iter().last())
    else {
        return;
    };
    let mut next = match result {
        Ok(next) => next,
        Err(error) => {
            live.status = error;
            return;
        }
    };
    live.status.clear();
    let refreshed_files = refreshed_files(&repo.0, &next);
    if refreshed_files.as_ref().is_some_and(HashSet::is_empty) {
        return;
    }
    let previous = repo
        .0
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<HashMap<_, _>>();
    keep_layout(&repo.0, &mut next);
    let now = time.elapsed_secs_f64();
    if !live.touched.is_empty() {
        let current: HashSet<_> = next.files.iter().map(|file| file.path.as_str()).collect();
        live.touched
            .retain(|path, _| current.contains(path.as_str()));
    }
    for file in &next.files {
        if previous
            .get(file.path.as_str())
            .is_none_or(|old| changed(old, file))
        {
            live.touched.insert(file.path.clone(), now);
            live.last_touched = Some((file.path.clone(), now));
        }
    }
    if live
        .last_touched
        .as_ref()
        .is_some_and(|(path, _)| !next.files.iter().any(|file| file.path == *path))
    {
        live.last_touched = None;
    }
    live.growth = next
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            let height = listing::block_size(file);
            match previous.get(file.path.as_str()) {
                Some(old) if changed(old, file) => {
                    let previous = listing::block_size(old);
                    (previous != height).then_some((index, previous))
                }
                None => Some((index, height.with_y(0.12))),
                _ => None,
            }
        })
        .collect();
    live.growing_until = time.elapsed_secs_f64() + GROWTH;
    let selected = repo.0.files.get(explorer.selected).and_then(|file| {
        next.files
            .iter()
            .position(|candidate| candidate.path == file.path)
    });
    let index = selected.unwrap_or_else(|| next.initial_file());
    let mut next_explorer = Explorer::new(index);
    if refreshed_files
        .as_ref()
        .is_some_and(|files| !files.contains(&index))
    {
        std::mem::swap(&mut *explorer, &mut next_explorer);
        next_explorer.anchored = false;
        next_explorer.comparison_mode = 0;
        next_explorer.reviewed.clear();
        next_explorer.rebuild = true;
    } else if !next.files.is_empty() {
        next_explorer.open(&next, index);
    }
    repository_picker::replace_selection(
        &mut commands,
        &old_city,
        next,
        next_explorer,
        refreshed_files.as_ref(),
    );
    commands.insert_resource(listing.live_reset());
    live.refreshed_files = refreshed_files;
    live.keep_camera = true;
}

pub(super) fn highlight(
    mut gizmos: Gizmos,
    live: Res<Live>,
    time: Res<Time>,
    repo: Res<Repo>,
    focus: Res<file_facade::ChangesOnly>,
    buildings: Query<(&Facade, &Transform)>,
) {
    if live.touched.is_empty() || !repo.0.is_worktree() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if live
        .last_touched
        .as_ref()
        .is_none_or(|(_, time)| now - time >= HIGHLIGHT)
    {
        return;
    }
    for (facade, pose) in &buildings {
        let file = &repo.0.files[facade.index];
        if !focus.includes(file) {
            continue;
        }
        let Some(touched) = live.touched.get(&file.path) else {
            continue;
        };
        let age = (now - touched) / HIGHLIGHT;
        if age >= 1. {
            continue;
        }
        let ink = Color::srgb_u8(89, 235, 220).with_alpha((1. - age) as f32);
        gizmos.cube(
            Transform::from_translation(pose.translation)
                .with_scale(pose.scale + Vec3::splat(0.12)),
            ink,
        );
        let roof = pose.translation + Vec3::Y * (pose.scale.y / 2. + 0.3);
        gizmos.line(roof, roof + Vec3::Y * 8., ink);
    }
}

#[derive(Component)]
pub(super) struct Growing {
    start: Transform,
    end: Transform,
    until: f64,
}

pub(super) fn grow(
    commands: &mut Commands,
    entity: Entity,
    file: &repo_world::File,
    part: file_facade::Part,
    size: Vec3,
    end: Vec3,
    until: f64,
) {
    let start = file_facade::transform(file, size, part);
    commands.entity(entity).insert((
        start,
        Growing {
            start,
            end: file_facade::transform(file, end, part),
            until,
        },
    ));
}

pub(super) fn animate(
    mut commands: Commands,
    time: Res<Time>,
    mut objects: Query<(Entity, &Growing, &mut Transform)>,
) {
    for (entity, growth, mut pose) in &mut objects {
        let fraction =
            (1. - (growth.until - time.elapsed_secs_f64()) / GROWTH).clamp(0., 1.) as f32;
        let eased = fraction * fraction * (3. - 2. * fraction);
        pose.translation = growth.start.translation.lerp(growth.end.translation, eased);
        pose.scale = growth.start.scale.lerp(growth.end.scale, eased);
        if fraction == 1. {
            commands.entity(entity).remove::<Growing>();
        }
    }
}

impl Live {
    pub(super) fn status(&self) -> &str {
        &self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_one_file_in_a_120_file_city_does_not_rebuild_its_neighbors() {
        fn objects(world: &mut World) -> (HashSet<Entity>, HashSet<Entity>) {
            let mut query =
                world.query_filtered::<(Entity, Option<&FileBuilding>), With<CityObject>>();
            let (changed, stable): (Vec<_>, Vec<_>) = query
                .iter(world)
                .partition(|(_, building)| building.is_some_and(|building| building.0 == 57));
            (
                stable.into_iter().map(|(entity, _)| entity).collect(),
                changed.into_iter().map(|(entity, _)| entity).collect(),
            )
        }

        let mut repo = repository_picker::empty();
        repo.revision = "worktree:fixture".into();
        repo.districts.push(repo_world::District {
            name: "src".into(),
            position: [0., 0.],
            size: [300., 300.],
        });
        let blob = repo_world::Blob {
            id: "old".into(),
            size: 120,
            mode: "100644".into(),
        };
        repo.files = (0..120)
            .map(|index| repo_world::File {
                path: format!("src/file_{index}.rs"),
                before: Some(blob.clone()),
                after: Some(blob.clone()),
                position: [(index % 12) as f32 * 22., (index / 12) as f32 * 22.],
                change: Change::Unchanged,
            })
            .collect();
        let mut next = repo.clone();
        next.files[57].after.as_mut().unwrap().size = 800;
        next.files[57].change = Change::Modified;
        let (send, updates) = mpsc::sync_channel(1);
        let mut app = App::new();
        app.insert_resource(Live {
            session: Some(Session {
                root: repo.root.clone(),
                updates: Mutex::new(updates),
                stop: Arc::default(),
            }),
            ..default()
        })
        .insert_resource(Repo(repo))
        .insert_resource(WorldSet::load(None).unwrap())
        .insert_resource(Palette {
            shell: Handle::default(),
            facade_closed: Handle::default(),
            facade_open: Handle::default(),
            facade_neutral: Handle::default(),
            facade_rims: default(),
            edge: Handle::default(),
            removed: Handle::default(),
            plate: Handle::default(),
            unit: Handle::default(),
        })
        .insert_resource(Explorer::new(0))
        .init_resource::<listing::Listing>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(Time::<()>::default())
        .add_systems(Update, (update, spawn_city).chain());
        app.update();
        let (stable_before, changed_before) = objects(app.world_mut());
        assert_eq!(changed_before.len(), 7);
        assert!(stable_before.len() > 119 * 7);

        send.send(Ok(next)).unwrap();
        app.update();
        let (stable_after, changed_after) = objects(app.world_mut());
        assert_eq!(stable_after, stable_before);
        assert_eq!(changed_after.len(), changed_before.len());
        assert!(changed_after.is_disjoint(&changed_before));
        let expected = &app.world().resource::<Repo>().0.files[57].path;
        let live = app.world().resource::<Live>();
        assert_eq!(live.last_touched(0.), Some((expected.as_str(), 0)));
        assert_eq!(live.touched.len(), 1);
        assert_eq!(
            app.world().resource::<Repo>().0.files[57]
                .after
                .as_ref()
                .unwrap()
                .size,
            800
        );
    }
}
