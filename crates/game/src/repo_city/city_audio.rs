//! Native city atmosphere, generative radio and local narration of visible code.
mod radio;
mod speech;
mod synthesis;

use super::*;
use bevy::{
    audio::{AddAudioSource, ChannelCount, SampleRate, Source},
    reflect::TypePath,
};
use serde::{Deserialize, Serialize};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use synthesis::{Levels, Synth};

pub(super) struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Settings::load())
            .init_resource::<Runtime>()
            .add_audio_source::<CitySound>()
            .add_systems(Startup, start)
            .add_systems(Update, update.after(listing::draw));
    }
}

#[derive(Resource, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    pub ambience: bool,
    pub speech: bool,
    pub volume: f32,
    pub voice: String,
    pub radio: Station,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Station {
    #[default]
    Off,
    SpaceTrance,
    Techno,
}

impl Station {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Off => Self::SpaceTrance,
            Self::SpaceTrance => Self::Techno,
            Self::Techno => Self::Off,
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::SpaceTrance => "Space trance",
            Self::Techno => "Techno",
        }
    }
    fn code(self) -> f32 {
        match self {
            Self::Off => 0.,
            Self::SpaceTrance => 1.,
            Self::Techno => 2.,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ambience: false,
            speech: false,
            volume: 0.18,
            voice: "Daniel".into(),
            radio: Station::Off,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(repository_picker::settings_file()?.with_file_name("audio.json"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice::<Self>(&data).ok())
            .filter(|settings| settings.volume.is_finite() && (0. ..=1.).contains(&settings.volume))
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        if !self.volume.is_finite() || !(0. ..=1.).contains(&self.volume) {
            return Err("Volume must be between 0 and 1.".into());
        }
        let path = Self::path().ok_or("Could not locate audio preferences.")?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let data = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(path.parent().expect("settings directory"))
            .and_then(|_| std::fs::write(&temporary, data))
            .and_then(|_| std::fs::rename(&temporary, path))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&temporary);
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(Default)]
struct Controls([AtomicU32; 8]);

impl Controls {
    fn store(&self, levels: Levels) {
        for (field, value) in self.0.iter().zip([
            levels.master,
            levels.wind,
            levels.spray,
            levels.works,
            levels.siren,
            levels.pan,
            levels.music,
            levels.radio,
        ]) {
            field.store(value.to_bits(), Ordering::Relaxed);
        }
    }
    fn load(&self) -> Levels {
        let [master, wind, spray, works, siren, pan, music, radio] =
            std::array::from_fn(|index| f32::from_bits(self.0[index].load(Ordering::Relaxed)));
        Levels {
            master,
            wind,
            spray,
            works,
            siren,
            pan,
            music,
            radio,
        }
    }
}

#[derive(Asset, TypePath)]
struct CitySound {
    controls: Arc<Controls>,
}
struct Decoder {
    controls: Arc<Controls>,
    synth: Synth,
    right: Option<f32>,
}
impl Decodable for CitySound {
    type Decoder = Decoder;
    fn decoder(&self) -> Self::Decoder {
        Decoder {
            controls: self.controls.clone(),
            synth: Synth::new(0xc17a),
            right: None,
        }
    }
}
impl Iterator for Decoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let [left, right] = self.synth.frame(self.controls.load());
        self.right = Some(right);
        Some(left)
    }
}
impl Source for Decoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(44_100).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[derive(PartialEq)]
struct Focus {
    root: PathBuf,
    revision: String,
    baseline: Option<String>,
    path: String,
    blob: String,
    before: bool,
    rows: Range<usize>,
    text_hash: u64,
}

#[derive(Resource)]
struct Runtime {
    controls: Arc<Controls>,
    voice: speech::Voice,
    reading: speech::Reading,
    focus: Option<Focus>,
    camera: Option<Vec3>,
    speech_enabled: bool,
    voice_failed: bool,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            controls: Arc::default(),
            voice: speech::Voice::new(),
            reading: speech::Reading::default(),
            focus: None,
            camera: None,
            speech_enabled: false,
            voice_failed: false,
        }
    }
}

fn start(mut commands: Commands, mut sounds: ResMut<Assets<CitySound>>, runtime: Res<Runtime>) {
    commands.spawn(AudioPlayer(sounds.add(CitySound {
        controls: runtime.controls.clone(),
    })));
}

fn update(
    settings: Res<Settings>,
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    nearest: Res<Nearest>,
    review: Res<review_notes::Review>,
    listing: Res<listing::Listing>,
    navigation: Res<city_navigation::Navigation>,
    reading_settings: Res<reading::Settings>,
    filter: Res<file_facade::ChangesOnly>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, PlayerCamera>,
    mut runtime: ResMut<Runtime>,
) {
    let (Ok(camera), Ok(window)) = (camera.single(), windows.single()) else {
        return;
    };
    let active = window.focused && !repo.0.files.is_empty();
    let movement = runtime
        .camera
        .replace(camera.translation)
        .map_or(0., |last| {
            last.distance(camera.translation) / time.delta_secs().max(0.001)
        });
    let changed = nearest
        .file
        .and_then(|aim| repo.0.files.get(aim.index))
        .is_some_and(|file| file.change != Change::Unchanged);
    runtime.controls.store(Levels {
        master: if active && settings.ambience {
            settings.volume
                * if runtime.voice.busy() {
                    0.12
                } else if review.blocked {
                    0.35
                } else {
                    1.
                }
        } else {
            0.
        },
        wind: (0.15 + camera.translation.y.max(0.) / 250. + movement / 120.).clamp(0.15, 1.),
        spray: if !review.blocked && !runtime.voice.busy() && review.nozzle > 0.1 {
            review.nozzle * 0.18
        } else {
            0.
        },
        works: if changed { 0.8 } else { 0.12 },
        siren: 0.5,
        pan: camera.forward().x,
        music: if active {
            settings.volume * if runtime.voice.busy() { 0.12 } else { 1. }
        } else {
            0.
        },
        radio: settings.radio.code(),
    });
    let enabled = active && settings.speech && settings.volume > 0. && !review.blocked;
    if enabled != runtime.speech_enabled {
        runtime.voice.stop();
        runtime.focus = None;
        runtime.reading = speech::Reading::default();
        runtime.speech_enabled = enabled;
        runtime.voice_failed = false;
    }
    if let Some(error) = runtime.voice.error() {
        warn!("{error}");
        runtime.voice_failed = true;
    }
    if !enabled || runtime.voice_failed {
        return;
    }
    if navigation.is_flying() {
        runtime.focus = None;
        runtime.reading = speech::Reading::default();
        return;
    }
    let visible = repo
        .0
        .files
        .get(explorer.selected)
        .filter(|file| filter.includes(file))
        .and_then(|file| {
            let fragments = listing.reading_source_fragments(
                file,
                camera,
                &explorer,
                window.width() / window.height().max(1.),
                *reading_settings,
            );
            let start = fragments.first()?.0.saturating_sub(1);
            let end = fragments.last()?.0;
            Some((file, start..end, fragments))
        });
    let Some((file, rows, fragments)) = visible else {
        if runtime.focus.take().is_some() {
            runtime.reading = speech::Reading::default();
        }
        return;
    };
    // Quote only readable fragments on screen, including partial wrapped source lines.
    let text = fragments.iter().take(100).map(|(_, text)| *text);
    if text.clone().next().is_none() {
        if runtime.focus.take().is_some() {
            runtime.reading = speech::Reading::default();
        }
        return;
    }
    let mut hash = DefaultHasher::new();
    for line in text.clone() {
        line.hash(&mut hash);
    }
    let text_hash = hash.finish();
    let blob = if explorer.before {
        &file.before
    } else {
        &file.after
    };
    let blob = blob.as_ref().map_or("", |blob| blob.id.as_str());
    let changed = runtime.focus.as_ref().is_none_or(|focus| {
        focus.root != repo.0.root
            || focus.revision != repo.0.revision
            || focus.baseline != repo.0.baseline
            || focus.path != file.path
            || focus.blob != blob
            || focus.before != explorer.before
            || focus.rows != rows
            || focus.text_hash != text_hash
    });
    if changed {
        let reading = reading::Settings {
            mode: reading::Mode::Delayed,
            min_seconds: None,
            max_seconds: None,
            ..default()
        };
        let seconds = reading.seconds(text.clone());
        let joined = text.collect::<Vec<_>>().join("\n");
        runtime.reading = speech::Reading::start(&joined, seconds, text_hash as u32);
        runtime.focus = Some(Focus {
            root: repo.0.root.clone(),
            revision: repo.0.revision.clone(),
            baseline: repo.0.baseline.clone(),
            path: file.path.clone(),
            blob: blob.to_owned(),
            before: explorer.before,
            rows,
            text_hash,
        });
    }
    if !runtime.voice.busy()
        && let Some(text) = runtime.reading.tick(time.delta_secs())
    {
        runtime
            .voice
            .speak(text, (settings.volume * 2.).min(1.), &settings.voice);
    }
}
