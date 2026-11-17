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

#[derive(Resource, Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct Settings {
    pub mode: Mode,
    pub min_seconds: Option<f32>,
    pub max_seconds: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Delayed,
            min_seconds: Some(3.),
            max_seconds: Some(6.),
        }
    }
}

impl Settings {
    fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str::<Self>(json)
            .map_err(|error| error.to_string())?
            .validated()
    }

    fn validated(self) -> Result<Self, String> {
        if [self.min_seconds, self.max_seconds]
            .into_iter()
            .flatten()
            .any(|seconds| !seconds.is_finite() || seconds < 0.)
        {
            return Err("Enter non-negative seconds or leave the field blank.".into());
        }
        if let (Some(min), Some(max)) = (self.min_seconds, self.max_seconds)
            && min > max
        {
            return Err("Minimum delay must not exceed maximum delay.".into());
        }
        Ok(self)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn with_bounds(mode: Mode, min: &str, max: &str) -> Result<Self, String> {
        let bound = |text: &str| match text.trim() {
            "" => Ok(None),
            text => text
                .parse::<f32>()
                .map(Some)
                .map_err(|_| "Enter non-negative seconds or leave the field blank.".to_owned()),
        };
        Self {
            mode,
            min_seconds: bound(min)?,
            max_seconds: bound(max)?,
        }
        .validated()
    }

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
        let estimate = characters as f32 * 60. / CHARACTERS_PER_MINUTE;
        estimate.clamp(
            self.min_seconds.unwrap_or(0.),
            self.max_seconds.unwrap_or(f32::INFINITY),
        )
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
        match std::fs::read_to_string(&path) {
            Ok(data) => Self::parse(&data).unwrap_or_else(|error| {
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
        self.validated()?;
        let path = Self::path().ok_or("Could not locate the preferences folder.")?;
        let data = serde_json::to_vec_pretty(&self).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::create_dir_all(path.parent().expect("settings directory"))
            .and_then(|_| std::fs::write(&temporary, data))
            .and_then(|_| std::fs::rename(&temporary, path))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&temporary);
            })
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeReadingSettings)]
    fn browser_settings() -> String;
}

#[cfg(target_arch = "wasm32")]
pub(super) fn apply(mut settings: ResMut<Settings>) {
    let value = browser_settings();
    if !value.is_empty()
        && let Ok(next) = Settings::parse(&value)
    {
        *settings = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_bounds_are_independent_and_blank_uses_the_reading_estimate() {
        let short = ["let x = 0;"];
        let long = "content ".repeat(100);
        let defaults = Settings::default();
        assert_eq!(defaults.seconds(short), 3.);
        assert_eq!(defaults.seconds([long.as_str()]), 6.);
        let unbounded = Settings {
            min_seconds: None,
            max_seconds: None,
            ..defaults
        };
        assert!(unbounded.seconds(short) < 3.);
        assert!(unbounded.seconds([long.as_str()]) > 6.);
        assert_eq!(
            Settings {
                max_seconds: None,
                ..defaults
            }
            .seconds(short),
            3.
        );
        assert_eq!(
            Settings {
                min_seconds: None,
                ..defaults
            }
            .seconds([long.as_str()]),
            6.
        );
        assert_eq!(
            Settings {
                mode: Mode::Immediate,
                ..defaults
            }
            .seconds(short),
            0.
        );
        assert!(Settings::parse(r#"{"mode":"delayed","min_seconds":7,"max_seconds":2}"#).is_err());
        assert_eq!(
            Settings::parse(r#"{"mode":"immediate"}"#)
                .unwrap()
                .max_seconds,
            Some(6.)
        );
        assert!(
            Settings::parse(r#"{"mode":"delayed","min_seconds":null}"#)
                .unwrap()
                .min_seconds
                .is_none()
        );
    }
}
