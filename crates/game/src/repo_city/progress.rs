//! Persists reveal ranges and review marks for one immutable comparison.
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mark {
    Reveal {
        path: String,
        before: bool,
        start: usize,
        end: usize,
        rows: usize,
    },
    Reviewed(String),
}

#[derive(Resource, Default)]
pub(super) struct Progress {
    scope: String,
    changes: Vec<(String, bool)>,
    reset: bool,
    #[cfg(not(target_arch = "wasm32"))]
    store: Option<std::sync::Mutex<sqlite::Store>>,
    #[cfg(target_arch = "wasm32")]
    reset_version: u32,
    #[cfg(target_arch = "wasm32")]
    reported: Option<(usize, usize)>,
}

impl Progress {
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.scope = serde_json::to_string(&(
                format!("{:?}", repo.root),
                &repo.revision,
                &repo.baseline,
            ))
            .expect("comparison key contains only strings");
            self.read_native().unwrap_or_else(|error| {
                warn!("Could not load review progress: {error}");
                vec![]
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = repo;
            self.scope = browser_scope();
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
    fn read_native(&mut self) -> Result<Vec<String>, String> {
        let scope = self.scope.clone();
        self.native_store()?.read(&scope)
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
    let paths: std::collections::HashMap<_, _> = repo
        .0
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.as_str(), index))
        .collect();
    for entry in progress.load(&repo.0) {
        match serde_json::from_str(&entry) {
            Ok(Mark::Reveal {
                path,
                before,
                start,
                end,
                rows,
            }) => {
                if let Some(&index) = paths.get(path.as_str()) {
                    let file = &repo.0.files[index];
                    if if before {
                        file.before.is_some()
                    } else {
                        file.after.is_some()
                    } {
                        listing.reveal(index, before, file, start..end, rows);
                    }
                }
            }
            Ok(Mark::Reviewed(path)) => {
                if let Some(&index) = paths.get(path.as_str()) {
                    explorer.reviewed.insert(index);
                }
            }
            Err(_) => {}
        }
    }
}

pub(super) fn sync(
    mut progress: ResMut<Progress>,
    mut explorer: ResMut<Explorer>,
    mut listing: ResMut<listing::Listing>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let version = browser_reset_version();
        if version != progress.reset_version {
            progress.reset_version = version;
            progress.reset = true;
        }
    }
    if std::mem::take(&mut progress.reset) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = progress.write(None) {
            explorer.notice = format!("Could not reset review progress: {error}");
            return;
        }
        progress.changes.clear();
        explorer.reviewed.clear();
        explorer.notice = "Review progress reset for this comparison.".into();
        listing.clear_progress();
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
        let changes = std::mem::take(&mut progress.changes);
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
