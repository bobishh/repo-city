//! Persists content-bound reveal ranges and comparison review marks.
use serde::{Deserialize, Serialize};

use super::*;

mod content;
mod view;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mark {
    Reveal {
        path: String,
        before: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blob: Option<String>,
        start: usize,
        end: usize,
        rows: usize,
    },
    Reviewed(String),
    Visited {
        path: String,
        diff: bool,
    },
    ReadDocument(markdown_preview::ReadDocument),
}

#[derive(Resource, Default)]
pub(super) struct Progress {
    scope: String,
    changes: Vec<(String, bool)>,
    reset: bool,
    visited: HashSet<usize>,
    visited_diffs: HashSet<usize>,
    total_diffs: usize,
    camera: view::Checkpoint,
    #[cfg(not(target_arch = "wasm32"))]
    store: Option<std::sync::Mutex<sqlite::Store>>,
    #[cfg(target_arch = "wasm32")]
    reset_version: u32,
    #[cfg(target_arch = "wasm32")]
    reported: Option<(usize, usize)>,
}

impl Progress {
    pub(super) fn visited_counts(&self) -> (usize, usize, usize) {
        (
            self.visited.len(),
            self.visited_diffs.len(),
            self.total_diffs,
        )
    }

    pub(super) fn has_visited(&self, index: usize) -> bool {
        self.visited.contains(&index)
    }

    pub(super) fn visit(&mut self, index: usize, file: &repo_world::File, diff: bool) {
        let new_file = self.visited.insert(index);
        let new_diff = diff && file.change != Change::Unchanged && self.visited_diffs.insert(index);
        if new_diff || new_file {
            self.mark(
                Mark::Visited {
                    path: file.path.clone(),
                    diff: new_diff,
                },
                true,
            );
        }
    }

    pub(super) fn exploration_label(&self, files: usize) -> String {
        let percentage = |count: usize, total: usize| count * 100 / total.max(1);
        let city = percentage(self.visited.len(), files);
        if self.total_diffs == 0 {
            format!("City explored {city}%")
        } else {
            let diff = percentage(self.visited_diffs.len(), self.total_diffs);
            format!("City explored {city}% · Diff explored {diff}%")
        }
    }
    pub(super) fn mark(&mut self, mark: Mark, checked: bool) {
        if let Ok(mark) = serde_json::to_string(&mark) {
            self.changes.push((mark, checked));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn request_reset(&mut self) {
        self.reset = true;
    }

    fn load(&mut self, repo: &Repository) -> Vec<String> {
        let scope = comparison_scope(repo);
        if self.scope != scope {
            self.scope = scope;
            self.camera = view::Checkpoint::default();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.read_native(&format!("{:?}", repo.root))
                .unwrap_or_else(|error| {
                    warn!("Could not load review progress: {error}");
                    vec![]
                })
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.reset_version = browser_reset_version();
            self.reported = None;
            serde_json::from_str(&browser_marks()).unwrap_or_default()
        }
    }

    fn write(&mut self, changes: Option<&[(String, bool)]>) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let scope = self.scope.clone();
            self.native_store()?.write(&scope, changes)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let json = serde_json::to_string(&changes).map_err(|error| error.to_string())?;
            browser_write(&self.scope, &json);
            Ok(())
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_native(&mut self, checkout: &str) -> Result<Vec<String>, String> {
        let scope = self.scope.clone();
        self.native_store()?.load(&scope, checkout)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_store(&mut self) -> Result<&mut sqlite::Store, String> {
        if self.store.is_none() {
            let path = repository_picker::settings_file()
                .ok_or("Could not locate the settings folder.")?
                .with_file_name("progress.sqlite3");
            std::fs::create_dir_all(path.parent().expect("settings directory"))
                .map_err(|error| error.to_string())?;
            self.store = Some(std::sync::Mutex::new(sqlite::Store::open(&path)?));
        }
        self.store
            .as_mut()
            .expect("progress store opened")
            .get_mut()
            .map_err(|error| error.to_string())
    }
}

pub(super) fn comparison_scope(repo: &Repository) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    return serde_json::to_string(&(format!("{:?}", repo.root), &repo.revision, &repo.baseline))
        .expect("comparison key contains only strings");
    #[cfg(target_arch = "wasm32")]
    {
        let _ = repo;
        browser_scope()
    }
}

pub(super) fn restore(
    repo: Res<Repo>,
    mut progress: ResMut<Progress>,
    mut explorer: ResMut<Explorer>,
    mut listing: ResMut<listing::Listing>,
) {
    if !repo.is_changed() || repo.0.files.is_empty() {
        return;
    }
    progress.changes.clear();
    progress.reset = false;
    progress.visited.clear();
    progress.visited_diffs.clear();
    progress.total_diffs = repo
        .0
        .files
        .iter()
        .filter(|file| file.change != Change::Unchanged)
        .count();
    let paths: std::collections::HashMap<_, _> = repo
        .0
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.as_str(), index))
        .collect();
    let mut sources = content::Sources::default();
    for entry in progress.load(&repo.0) {
        let Ok(entry) = serde_json::from_str::<content::StoredMark>(&entry) else {
            continue;
        };
        match &entry.mark {
            Mark::Reveal { path, before, .. } => {
                if let Some(&index) = paths.get(path.as_str()) {
                    let file = &repo.0.files[index];
                    for (range, rows) in entry.reveals(&repo.0, index, &mut sources) {
                        listing.reveal(index, *before, file, range, rows);
                        progress.visited.insert(index);
                    }
                }
            }
            Mark::Reviewed(path) => {
                if let Some(&index) = paths.get(path.as_str()) {
                    explorer.reviewed.insert(index);
                    progress.visited.insert(index);
                }
            }
            Mark::ReadDocument(document) => {
                if let Some(&index) = paths.get(document.path.as_str()) {
                    let file = &repo.0.files[index];
                    if document.matches(file) {
                        listing.reveal_document(index, document.before, document.rows);
                        progress.visited.insert(index);
                    } else {
                        for (range, rows) in entry.reveals(&repo.0, index, &mut sources) {
                            listing.reveal(index, document.before, file, range, rows);
                            progress.visited.insert(index);
                        }
                    }
                }
            }
            Mark::Visited { path, diff } => {
                if let Some(&index) = paths.get(path.as_str()) {
                    progress.visited.insert(index);
                    if *diff && repo.0.files[index].change != Change::Unchanged {
                        progress.visited_diffs.insert(index);
                    }
                }
            }
        }
    }
}

pub(super) fn sync(
    time: Res<Time>,
    mut progress: ResMut<Progress>,
    mut explorer: ResMut<Explorer>,
    mut listing: ResMut<listing::Listing>,
    repo: Res<Repo>,
    navigation: Res<city_navigation::Navigation>,
    review: Res<review_notes::Review>,
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    #[cfg(not(target_arch = "wasm32"))] mut document: Option<ResMut<markdown_preview::ReadQueue>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    let read = document.as_deref_mut().and_then(|queue| queue.0.take());
    #[cfg(target_arch = "wasm32")]
    let read = markdown_preview::take_browser_read();
    #[cfg(target_arch = "wasm32")]
    {
        let version = browser_reset_version();
        if version != progress.reset_version {
            progress.reset_version = version;
            progress.reset = true;
        }
    }
    if let Some(document) = read
        && let Some((index, file)) = repo
            .0
            .files
            .iter()
            .enumerate()
            .find(|(_, file)| document.matches(file))
        && listing.reveal_document(index, document.before, document.rows)
    {
        progress.visit(index, file, false);
        progress.mark(Mark::ReadDocument(document), true);
    }
    if std::mem::take(&mut progress.reset) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = progress.write(None) {
            explorer.notice = format!("Could not reset review progress: {error}");
            return;
        }
        progress.changes.clear();
        explorer.reviewed.clear();
        progress.visited.clear();
        progress.visited_diffs.clear();
        explorer.notice = "Review progress reset for this comparison.".into();
        listing.clear_progress();
    } else if !review.blocked
        && listing::Listing::visible(&explorer, &navigation)
        && !navigation.overview()
        && !listing.turning()
        && let Ok((eye, pilot)) = camera.single()
        && pilot.captured
        && let Some(file) = repo.0.files.get(explorer.selected)
    {
        let size = listing::block_size(file);
        if let Some(face) =
            listing::Face::at_camera(file, size, eye, listing::tool_range(size.x) * 1.4)
        {
            progress.visit(explorer.selected, file, face == listing::Face::Diff);
            progress.record_view(file, eye, time.elapsed_secs_f64());
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let counts = (explorer.reviewed.len(), listing.revealed_count());
        if progress.reported != Some(counts) {
            progress.reported = Some(counts);
            browser_report(counts.0, counts.1);
        }
    }
    if !progress.changes.is_empty() {
        let changes = content::bind_changes(&repo.0, std::mem::take(&mut progress.changes));
        if let Err(error) = progress.write(Some(&changes)) {
            // Report disk failure without retrying I/O every frame.
            warn!("Could not save review progress: {error}");
            explorer.notice = "Review progress could not be saved.".into();
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = progressScope)]
    fn browser_scope() -> String;
    #[wasm_bindgen(js_namespace = repoCity, js_name = progressMarks)]
    fn browser_marks() -> String;
    #[wasm_bindgen(js_namespace = repoCity, js_name = progressResetVersion)]
    fn browser_reset_version() -> u32;
    #[wasm_bindgen(js_namespace = repoCity, js_name = writeProgress)]
    fn browser_write(scope: &str, json: &str);
    #[wasm_bindgen(js_namespace = repoCity, js_name = reportProgress)]
    fn browser_report(reviewed: usize, revealed: usize);
}

#[cfg(not(target_arch = "wasm32"))]
mod sqlite;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn committing_new_code_keeps_read_unchanged_lines_without_thawing_new_code() {
        struct Checkout(std::path::PathBuf);
        impl Drop for Checkout {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let checkout = Checkout(std::env::temp_dir().join(format!(
            "repo-city-progress-commit-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
        std::fs::create_dir_all(&checkout.0).unwrap();
        let git = |args: &[&str]| {
            let result = std::process::Command::new("git")
                .current_dir(&checkout.0)
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
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        git(&["init", "--quiet"]);
        let old = "fn already_read() {}\nfn old_body() {}\nfn never_read() {}\n";
        std::fs::write(checkout.0.join("code.rs"), old).unwrap();
        git(&["add", "code.rs"]);
        git(&["commit", "--quiet", "-m", "original"]);
        let previous = Repository::open_worktree(&checkout.0).unwrap();
        let mut progress = Progress {
            store: Some(std::sync::Mutex::new(
                sqlite::Store::open(std::path::Path::new(":memory:")).unwrap(),
            )),
            ..default()
        };
        assert!(progress.load(&previous).is_empty());
        let saved = content::bind_changes(
            &previous,
            vec![(
                serde_json::to_string(&Mark::Reveal {
                    path: "code.rs".into(),
                    before: false,
                    blob: previous.files[0].after.as_ref().map(|blob| blob.id.clone()),
                    start: 0,
                    end: 2,
                    rows: 3,
                })
                .unwrap(),
                true,
            )],
        );
        progress.write(Some(&saved)).unwrap();
        let new = "// new code\nfn already_read() {}\nfn changed_body() {}\nfn never_read() {}\n";
        std::fs::write(checkout.0.join("code.rs"), new).unwrap();
        git(&["add", "code.rs"]);
        git(&["commit", "--quiet", "-m", "new code"]);
        let current = Repository::open_worktree(&checkout.0).unwrap();
        assert_ne!(current.revision, previous.revision);
        let mut explorer = Explorer::new(0);
        explorer.set_highlighted(syntax::highlight("code.rs", new));
        let mut app = App::new();
        app.insert_resource(Repo(current))
            .insert_resource(progress)
            .insert_resource(explorer)
            .init_resource::<listing::Listing>()
            .add_systems(Update, restore);
        app.update();
        let listing = app.world().resource::<listing::Listing>();
        let explorer = app.world().resource::<Explorer>();
        assert_eq!(listing.counts(explorer), (1, 4));
        assert!(listing.source_row_is_readable(explorer, 1));
        for unread in [0, 2, 3] {
            assert!(!listing.source_row_is_readable(explorer, unread));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_worktree_does_not_restore_legacy_reveals_without_content_identity() {
        let file = repo_world::File {
            path: "changed.rs".into(),
            before: None,
            after: Some(repo_world::Blob {
                id: "worktree:new-content".into(),
                size: 8,
                mode: "100644".into(),
            }),
            change: Change::Modified,
            position: [0., 0.],
        };
        let repo = Repository {
            root: "/example".into(),
            revision: "worktree:head".into(),
            baseline: Some("head".into()),
            files: vec![file],
            districts: Vec::new(),
        };
        let mut progress = Progress::default();
        let scope =
            serde_json::to_string(&(format!("{:?}", repo.root), &repo.revision, &repo.baseline))
                .unwrap();
        let mut store = sqlite::Store::open(std::path::Path::new(":memory:")).unwrap();
        let mark = serde_json::to_string(&Mark::Reveal {
            path: "changed.rs".into(),
            before: false,
            blob: None,
            start: 0,
            end: 1,
            rows: 1,
        })
        .unwrap();
        store.write(&scope, Some(&[(mark, true)])).unwrap();
        progress.store = Some(std::sync::Mutex::new(store));
        let mut explorer = Explorer::new(0);
        explorer.set_highlighted(syntax::highlight("changed.rs", "new code"));
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(progress)
            .insert_resource(explorer)
            .init_resource::<listing::Listing>()
            .add_systems(Update, restore);
        app.update();
        assert_eq!(
            app.world()
                .resource::<listing::Listing>()
                .counts(app.world().resource::<Explorer>()),
            (0, 1),
            "an old reveal cannot certify rewritten worktree bytes"
        );
    }

    #[test]
    fn exploration_counts_unique_files_and_only_changed_diffs() {
        let file = |path: &str, change| repo_world::File {
            path: path.into(),
            before: None,
            after: None,
            change,
            position: [0., 0.],
        };
        let mut progress = Progress {
            total_diffs: 2,
            ..default()
        };
        let changed = file("changed.rs", Change::Modified);
        let unchanged = file("same.rs", Change::Unchanged);
        progress.visit(0, &changed, false);
        progress.visit(0, &changed, true);
        progress.visit(0, &changed, true);
        progress.visit(1, &unchanged, true);
        assert_eq!(
            progress.exploration_label(4),
            "City explored 50% · Diff explored 50%"
        );
        assert_eq!(progress.changes.len(), 3);
    }
}
