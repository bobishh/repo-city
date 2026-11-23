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
use sha2::{Digest, Sha256};

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
    change_generation: u64,
}

impl Live {
    pub(super) fn change_generation(&self) -> u64 {
        self.change_generation
    }

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
    updates: Mutex<Receiver<Result<(Repository, u64), String>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Session {
    fn start(previous: &Repository) -> Self {
        let root = previous.root.clone();
        let mut previous = previous.clone();
        let mut change_generation = 0_u64;
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
                let refreshed = Repository::open_worktree(&folder).map(|next| {
                    // Compare only candidate content on the watcher thread. Git and
                    // worktree IDs use different hashes for the same saved bytes.
                    if source_changed(&previous, &next) {
                        change_generation = change_generation.wrapping_add(1);
                    }
                    previous = next.clone();
                    (next, change_generation)
                });
                if send.send(refreshed).is_err() {
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

fn source_changed(previous: &Repository, next: &Repository) -> bool {
    let before = previous
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            file.after
                .as_ref()
                .map(|blob| (file.path.as_str(), (index, blob)))
        })
        .collect::<HashMap<_, _>>();
    let after = next
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            file.after
                .as_ref()
                .map(|blob| (file.path.as_str(), (index, blob)))
        })
        .collect::<HashMap<_, _>>();
    before.len() != after.len()
        || before.iter().any(|(path, (old_index, old))| {
            let Some((new_index, new)) = after.get(path) else {
                return true;
            };
            old.size != new.size
                || (old.id != new.id
                    && match (
                        content_identity(previous, *old_index, old),
                        content_identity(next, *new_index, new),
                    ) {
                        (Some(old), Some(new)) => old != new,
                        _ => true,
                    })
        })
}

fn content_identity(repo: &Repository, index: usize, blob: &repo_world::Blob) -> Option<String> {
    if let Some(hash) = blob.id.strip_prefix("worktree:")
        && hash.len() == 64
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Some(hash.to_owned());
    }
    if blob.id.starts_with("worktree:") {
        // Oversized worktree assets have metadata identities, not recoverable
        // old bytes. Keep such changes audible without reading large files.
        return None;
    }
    let bytes = repo.asset_bytes(index, false).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
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
    // Saves stay at their addresses. A changed file inventory uses the repository's
    // freshly packed districts so additions and removals follow their real folders.
    if previous.files.len() != next.files.len()
        || previous
            .files
            .iter()
            .zip(&next.files)
            .any(|(old, new)| old.path != new.path)
    {
        return;
    }
    next.districts.clone_from(&previous.districts);
    for (old, new) in previous.files.iter().zip(&mut next.files) {
        new.position = old.position;
    }
}

pub(super) fn update(
    mut commands: Commands,
    time: Res<Time>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    listing: Res<listing::Listing>,
    mut camera: Query<&mut Transform, PlayerCamera>,
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
            session: Some(Session::start(&repo.0)),
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
    let (mut next, change_generation) = match result {
        Ok(next) => next,
        Err(error) => {
            live.status = error;
            return;
        }
    };
    live.status.clear();
    live.change_generation = change_generation;
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
    if selected.is_some()
        && let Ok(mut eye) = camera.single_mut()
    {
        let file = &repo.0.files[explorer.selected];
        let [x, z] = file.position;
        let [next_x, next_z] = next.files[index].position;
        let size = listing::block_size(file);
        let reach = listing.frame.detail_reach(size.x).max(size.x * 1.85);
        if (x, z) != (next_x, next_z)
            && (explorer.anchored || listing::Face::at_camera(file, size, &eye, reach).is_some())
        {
            eye.translation += Vec3::new(next_x - x, 0., next_z - z);
        }
    }
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
    fn saving_then_committing_identical_bytes_does_not_repeat_the_save_cue() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let path = std::env::temp_dir().join(format!(
            "repo-city-save-cue-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        let fixture = Fixture(path);
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&fixture.0)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        std::fs::write(fixture.0.join("main.rs"), "one\n").unwrap();
        git(&["add", "main.rs"]);
        git(&["commit", "--quiet", "-m", "initial"]);
        let initial = Repository::open_worktree(&fixture.0).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                fixture.0.join("main.rs"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            let mode_only = Repository::open_worktree(&fixture.0).unwrap();
            assert!(!source_changed(&initial, &mode_only));
        }
        std::fs::write(fixture.0.join("main.rs"), "two\n").unwrap();
        let saved = Repository::open_worktree(&fixture.0).unwrap();
        assert!(source_changed(&initial, &saved));
        git(&["add", "main.rs"]);
        git(&["commit", "--quiet", "-m", "save"]);
        let committed = Repository::open_worktree(&fixture.0).unwrap();
        assert!(!source_changed(&saved, &committed));
        std::fs::remove_file(fixture.0.join("main.rs")).unwrap();
        let deleted = Repository::open_worktree(&fixture.0).unwrap();
        assert!(source_changed(&committed, &deleted));
    }

    #[test]
    fn save_cues_track_worktree_content_including_deletion_not_baseline_or_mode_refreshes() {
        let mut previous = repository_picker::empty();
        previous.files.push(repo_world::File {
            path: "src/main.rs".into(),
            before: None,
            after: Some(repo_world::Blob {
                id: "content-a".into(),
                size: 120,
                mode: "100644".into(),
            }),
            position: [0., 0.],
            change: Change::Added,
        });
        let mut next = previous.clone();
        assert!(!source_changed(&previous, &next));
        next.revision = "worktree:new-head".into();
        next.baseline = Some("new-baseline".into());
        next.files[0].before = next.files[0].after.clone();
        next.files[0].after.as_mut().unwrap().mode = "100755".into();
        assert!(!source_changed(&previous, &next));
        next.files[0].after.as_mut().unwrap().id = "content-b".into();
        assert!(source_changed(&previous, &next));
        next.files[0].after = None;
        assert!(source_changed(&previous, &next));
        next.files.clear();
        assert!(source_changed(&previous, &next));
        assert!(source_changed(&next, &previous));
    }

    #[test]
    fn worktree_files_join_their_real_districts_on_each_structural_refresh() {
        let mut previous = repository_picker::empty();
        let file = |path: &str, position| repo_world::File {
            path: path.into(),
            before: None,
            after: None,
            position,
            change: Change::Added,
        };
        previous.files = vec![file("src/main.rs", [12., 12.])];
        previous.districts = vec![repo_world::District {
            name: "src".into(),
            position: [0., 0.],
            size: [35., 38.],
        }];
        for path in ["src/new.rs", "src/next.rs"] {
            let mut next = previous.clone();
            next.districts[0].size[0] += 27.;
            let width = next.districts[0].size[0];
            next.files.push(file(path, [width - 15., 12.]));
            keep_layout(&previous, &mut next);
            assert_eq!(next.districts.len(), 1);
            assert_eq!(next.districts[0].name, "src");
            assert!(next.files.last().unwrap().position[0] < next.districts[0].size[0]);
            previous = next;
        }
    }

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
            facades: Default::default(),
            facade_neutral: Handle::default(),
            facade_rims: default(),
            edge: Handle::default(),
            removed: Handle::default(),
            plate: Handle::default(),
            unit: Handle::default(),
        })
        .insert_resource(Explorer::new(0))
        .init_resource::<listing::Listing>()
        .init_resource::<progress::Progress>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .insert_resource(Time::<()>::default())
        .add_systems(Update, (update, spawn_city).chain());
        app.update();
        assert_eq!(app.world().resource::<Live>().change_generation(), 0);
        let (stable_before, changed_before) = objects(app.world_mut());
        assert_eq!(changed_before.len(), 7);
        assert!(stable_before.len() > 119 * 7);

        send.send(Ok((next, 1))).unwrap();
        app.update();
        let (stable_after, changed_after) = objects(app.world_mut());
        assert_eq!(stable_after, stable_before);
        assert_eq!(changed_after.len(), changed_before.len());
        assert!(changed_after.is_disjoint(&changed_before));
        let expected = &app.world().resource::<Repo>().0.files[57].path;
        let live = app.world().resource::<Live>();
        assert_eq!(live.change_generation(), 1);
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
