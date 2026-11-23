use super::*;

const POSITION_STEP: f32 = 0.35;
const SAVE_INTERVAL: f64 = 0.5;

#[derive(Serialize, Deserialize)]
struct View {
    path: String,
    offset: [f32; 3],
    rotation: [f32; 4],
}

impl View {
    fn origin(file: &repo_world::File) -> Vec3 {
        Vec3::new(file.position[0], 0., file.position[1])
    }

    fn capture(file: &repo_world::File, camera: &Transform) -> Self {
        Self {
            path: file.path.clone(),
            offset: (camera.translation - Self::origin(file)).to_array(),
            rotation: camera.rotation.to_array(),
        }
    }

    fn position(&self, file: &repo_world::File) -> Vec3 {
        Self::origin(file) + Vec3::from_array(self.offset)
    }

    fn pose(&self, file: &repo_world::File, frame: listing::ReadingFrame) -> Option<Transform> {
        let translation = self.position(file);
        let rotation = Quat::from_array(self.rotation);
        if !translation.is_finite()
            || translation.y < city_navigation::STREET_EYE_HEIGHT
            || !rotation.is_normalized()
        {
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
    pub(crate) fn resume_camera(
        &mut self,
        repo: &Repository,
        explorer: &mut Explorer,
        frame: listing::ReadingFrame,
    ) -> Option<Transform> {
        if !explorer.resume_on_open {
            return None;
        }
        let scope = comparison_scope(repo);
        let json = self.read_view(&scope).ok().flatten()?;
        let saved: View = serde_json::from_str(&json).ok()?;
        let index = repo.files.iter().position(|file| file.path == saved.path)?;
        if index != explorer.selected {
            explorer.open(repo, index);
        }
        saved.pose(&repo.files[index], frame)
    }

    pub(super) fn record_view(&mut self, file: &repo_world::File, camera: &Transform, now: f64) {
        if self.scope.is_empty() {
            return;
        }
        if let Some(previous) = &self.camera.saved
            && previous.path == file.path
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
        let view = View::capture(file, camera);
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
        let scope = world_scope(repo, world);
        serde_json::from_str(&self.read_view(&scope).ok().flatten()?).ok()
    }

    pub(crate) fn save_world_place(
        &mut self,
        repo: &Repository,
        world: &str,
        place: &impl Serialize,
    ) -> Result<(), String> {
        let scope = world_scope(repo, world);
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
        self.write_view_at(&self.scope.clone(), json)
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

fn world_scope(repo: &Repository, world: &str) -> String {
    format!("{}|world={world}", comparison_scope(repo))
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
        progress.record_view(&repo.files[0], &camera(0., 0.), 0.);
        progress.record_view(&repo.files[1], &camera(32., 8.), 0.1);
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
