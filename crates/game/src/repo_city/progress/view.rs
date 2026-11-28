use super::*;

const POSITION_STEP: f32 = 0.35;
const SAVE_INTERVAL: f64 = 0.5;

#[derive(Serialize, Deserialize)]
struct View {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<bool>,
    offset: [f32; 3],
    rotation: [f32; 4],
}

impl View {
    fn origin(file: &repo_world::File) -> Vec3 {
        Vec3::new(file.position[0], 0., file.position[1])
    }

    fn capture(file: &repo_world::File, before: bool, camera: &Transform) -> Self {
        Self {
            path: file.path.clone(),
            before: Some(before),
            offset: (camera.translation - Self::origin(file)).to_array(),
            rotation: camera.rotation.to_array(),
        }
    }

    fn position(&self, file: &repo_world::File) -> Vec3 {
        Self::origin(file) + Vec3::from_array(self.offset)
    }

    fn pose(
        &self,
        file: &repo_world::File,
        frame: listing::ReadingFrame,
        floor: f32,
    ) -> Option<Transform> {
        let translation = self.position(file);
        let rotation = Quat::from_array(self.rotation);
        if !translation.is_finite() || translation.y < floor || !rotation.is_normalized() {
            return None;
        }
        let pose = Transform::from_translation(translation).with_rotation(rotation);
        let size = listing::block_size(file);
        listing::Face::at_camera(file, size, &pose, frame.tool_range(size.x) * 1.4)?;
        Some(pose)
    }
}

#[derive(Default)]
pub(super) struct Checkpoint {
    saved: Option<View>,
    saved_at: f64,
}

impl Progress {
    pub(crate) fn separate_camera(&mut self, world: String) {
        self.camera_world = Some(world);
        self.camera = Checkpoint::default();
    }

    fn camera_scope(&self, comparison: &str) -> String {
        match self.camera_world.as_deref() {
            Some(world) => world_scope(comparison, world),
            // Existing city checkpoints use the comparison itself as their key.
            None => comparison.to_owned(),
        }
    }

    pub(super) fn restore_file_cursor(&mut self, scope: &str) {
        let key = format!("{scope}|city-change-route");
        self.file_cursor = self
            .read_view(&key)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok());
    }

    pub(crate) fn set_file_cursor(&mut self, path: Option<&str>) {
        self.file_cursor = path.map(str::to_owned);
        if !self.scope.is_empty() {
            let key = format!("{}|city-change-route", self.scope);
            let result = serde_json::to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|json| self.write_view_at(&key, &json));
            if let Err(error) = result {
                warn!("Could not save the change review position: {error}");
            }
        }
    }

    pub(crate) fn resume_camera(
        &mut self,
        repo: &Repository,
        explorer: &mut Explorer,
        frame: listing::ReadingFrame,
    ) -> Option<Transform> {
        if !explorer.resume_on_open {
            return None;
        }
        let scope = self.camera_scope(&comparison_scope(repo));
        let json = self.read_view(&scope).ok().flatten()?;
        let saved: View = serde_json::from_str(&json).ok()?;
        let index = repo.files.iter().position(|file| file.path == saved.path)?;
        if index != explorer.selected {
            explorer.open(repo, index);
        }
        let file = &repo.files[index];
        if let Some(before) = saved.before
            && before != explorer.before
            && (if before { &file.before } else { &file.after }).is_some()
        {
            explorer.before = before;
            explorer.reload(repo);
        }
        let floor = if self.camera_world.is_some() {
            CAMERA_FLOOR
        } else {
            city_navigation::STREET_EYE_HEIGHT
        };
        saved.pose(file, frame, floor)
    }

    pub(crate) fn record_view(
        &mut self,
        file: &repo_world::File,
        before: bool,
        camera: &Transform,
        now: f64,
    ) {
        if self.scope.is_empty() {
            return;
        }
        if let Some(previous) = &self.camera.saved
            && previous.path == file.path
            && previous.before == Some(before)
            && (now - self.camera.saved_at < SAVE_INTERVAL
                || (previous.position(file).distance_squared(camera.translation)
                    < POSITION_STEP.powi(2)
                    && Quat::from_array(previous.rotation)
                        .dot(camera.rotation)
                        .abs()
                        > 0.9995))
        {
            return;
        }
        let view = View::capture(file, before, camera);
        let result = serde_json::to_string(&view)
            .map_err(|error| error.to_string())
            .and_then(|json| self.write_view(&json));
        self.camera.saved = Some(view);
        self.camera.saved_at = now;
        if let Err(error) = result {
            warn!("Could not save the last viewed file: {error}");
        }
    }

    pub(crate) fn world_place<T: serde::de::DeserializeOwned>(
        &mut self,
        repo: &Repository,
        world: &str,
    ) -> Option<T> {
        let scope = world_scope(&comparison_scope(repo), world);
        serde_json::from_str(&self.read_view(&scope).ok().flatten()?).ok()
    }

    pub(crate) fn save_world_place(
        &mut self,
        repo: &Repository,
        world: &str,
        place: &impl Serialize,
    ) -> Result<(), String> {
        let scope = world_scope(&comparison_scope(repo), world);
        let value = serde_json::to_string(place).map_err(|error| error.to_string())?;
        self.write_view_at(&scope, &value)
    }

    fn read_view(&mut self, scope: &str) -> Result<Option<String>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.native_store()?.read_view(scope);
        #[cfg(target_arch = "wasm32")]
        return Ok(browser_storage()
            .and_then(|storage| storage.get_item(&browser_key(scope)).ok().flatten()));
    }

    fn write_view(&mut self, json: &str) -> Result<(), String> {
        self.write_view_at(&self.camera_scope(&self.scope), json)
    }

    fn write_view_at(&mut self, scope: &str, json: &str) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native_store()?.write_view(scope, json)
        }
        #[cfg(target_arch = "wasm32")]
        {
            browser_storage()
                .ok_or("Browser storage is unavailable.")?
                .set_item(&browser_key(scope), json)
                .map_err(|_| "Browser storage rejected the saved camera position.".into())
        }
    }
}

fn world_scope(comparison: &str, world: &str) -> String {
    format!("{comparison}|world={world}")
}

#[cfg(target_arch = "wasm32")]
fn browser_key(scope: &str) -> String {
    format!("repo-city-last-view-v1:{scope}")
}

#[cfg(target_arch = "wasm32")]
fn browser_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn the_bathyscaphe_and_city_resume_their_own_views_of_the_same_file() {
        let mut repo = repository_picker::empty();
        repo.files.push(repo_world::File {
            path: "main.rs".into(),
            position: [0., 0.],
            before: Some(repo_world::Blob {
                id: "a".repeat(40),
                size: 4800,
                mode: "100644".into(),
            }),
            after: Some(repo_world::Blob {
                id: "b".repeat(40),
                size: 4800,
                mode: "100644".into(),
            }),
            change: Change::Unchanged,
        });
        let mut progress = Progress {
            store: Some(std::sync::Mutex::new(
                sqlite::Store::open(std::path::Path::new(":memory:")).unwrap(),
            )),
            scope: comparison_scope(&repo),
            ..default()
        };
        let file = &repo.files[0];
        let city = Transform::from_xyz(12., 7., 1.).looking_at(Vec3::Y * 6., Vec3::Y);
        let dive = Transform::from_xyz(12., 0.8, 2.).looking_at(Vec3::Y * 3., Vec3::Y);
        progress.record_view(file, false, &city, 0.);
        progress.separate_camera("abyss".into());
        let mut explorer = Explorer::new(0);
        let frame = listing::ReadingFrame::default();
        assert!(
            progress
                .resume_camera(&repo, &mut explorer, frame)
                .is_none()
        );
        progress.record_view(file, true, &dive, 1.);
        assert_eq!(
            progress.resume_camera(&repo, &mut explorer, frame),
            Some(dive)
        );
        assert!(explorer.before);
        progress.camera_world = None;
        assert_eq!(
            progress.resume_camera(&repo, &mut explorer, frame),
            Some(city)
        );
        assert!(!explorer.before);
    }

    #[test]
    fn resumes_the_latest_view_in_its_comparison_and_respects_an_explicit_file() {
        let file = |path: &str, position| repo_world::File {
            path: path.into(),
            before: None,
            after: None,
            change: Change::Unchanged,
            position,
        };
        let mut repo = repository_picker::empty();
        repo.files = vec![file("a", [0., 0.]), file("inventory", [32., 8.])];
        let camera = |x, z| {
            Transform::from_xyz(x + 12., 7., z + 1.).looking_at(Vec3::new(x, 6., z), Vec3::Y)
        };
        let mut progress = Progress {
            store: Some(std::sync::Mutex::new(
                sqlite::Store::open(std::path::Path::new(":memory:")).unwrap(),
            )),
            scope: comparison_scope(&repo),
            ..default()
        };
        progress.record_view(&repo.files[0], false, &camera(0., 0.), 0.);
        progress.record_view(&repo.files[1], false, &camera(32., 8.), 0.1);
        let mut explorer = Explorer::new(0);
        explorer.resume_on_open = false;
        assert!(
            progress
                .resume_camera(&repo, &mut explorer, listing::ReadingFrame::default())
                .is_none()
        );
        assert_eq!(explorer.selected, 0);

        explorer.resume_on_open = true;
        repo.baseline = Some("different".into());
        assert!(
            progress
                .resume_camera(&repo, &mut explorer, listing::ReadingFrame::default())
                .is_none()
        );
        repo.baseline = None;
        repo.files[1].position = [44., 12.];
        let restored = progress
            .resume_camera(&repo, &mut explorer, listing::ReadingFrame::default())
            .unwrap();
        assert_eq!(explorer.selected, 1);
        assert_eq!(restored.translation, camera(44., 12.).translation);
        assert_eq!(restored.rotation, camera(32., 8.).rotation);

        repo.files.pop();
        explorer.selected = 0;
        assert!(
            progress
                .resume_camera(&repo, &mut explorer, listing::ReadingFrame::default())
                .is_none()
        );
        assert_eq!(explorer.selected, 0);
    }
}
