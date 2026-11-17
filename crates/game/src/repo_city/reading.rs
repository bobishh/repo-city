//! Reading preferences belong to the player, independently of saved progress.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Immediate,
    #[default]
    Delayed,
}

#[derive(Resource, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct Settings {
    pub mode: Mode,
}

impl Settings {
    pub(super) fn seconds<'a>(self, lines: impl IntoIterator<Item = &'a str>) -> f32 {
        if self.mode == Mode::Immediate {
            return 0.;
        }
        // Brysbaert (2019), doi:10.1016/j.jml.2019.104047. Use the
        // length-adjusted prose estimate as a baseline for an initial scan.
        const CHARACTERS_PER_MINUTE: f32 = 238. * 4.6;
        let characters: usize = lines
            .into_iter()
            .filter(|line| line.chars().any(char::is_alphanumeric))
            .map(|line| line.chars().filter(|c| !c.is_whitespace()).count())
            .sum();
        characters as f32 * 60. / CHARACTERS_PER_MINUTE
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn path() -> Option<PathBuf> {
        Some(repository_picker::settings_file()?.with_file_name("reading.json"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
                warn!("Could not read {}: {error}", path.display());
                Self::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let settings = Self::default();
                if let Err(error) = settings.save() {
                    warn!("Could not create reading preferences: {error}");
                }
                settings
            }
            Err(error) => {
                warn!("Could not read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn save(self) -> Result<(), String> {
        let path = Self::path().ok_or("Could not locate the preferences folder.")?;
        let data = serde_json::to_vec_pretty(&self).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(path.parent().expect("settings directory"))
            .and_then(|_| std::fs::write(path, data))
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeReadingSettings)]
    fn browser_settings() -> bool;
}

#[cfg(target_arch = "wasm32")]
pub(super) fn apply(mut settings: ResMut<Settings>) {
    settings.mode = if browser_settings() {
        Mode::Immediate
    } else {
        Mode::Delayed
    };
}
