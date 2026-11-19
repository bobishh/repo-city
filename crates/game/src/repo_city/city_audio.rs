//! Native city atmosphere, generative radio and local narration of visible code.
mod change_sound;
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
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    pub file_changes: bool,
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
            file_changes: false,
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
struct Controls {
    levels: [AtomicU32; 8],
    cue: AtomicU32,
    cue_volume: AtomicU32,
}

impl Controls {
    fn store(&self, levels: Levels) {
        for (field, value) in self.levels.iter().zip([
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
            std::array::from_fn(|index| f32::from_bits(self.levels[index].load(Ordering::Relaxed)));
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
    cue: u32,
    tone: change_sound::Tone,
}
impl Decodable for CitySound {
    type Decoder = Decoder;
    fn decoder(&self) -> Self::Decoder {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0xc17a, |time| time.subsec_nanos() ^ time.as_secs() as u32);
        Decoder {
            controls: self.controls.clone(),
            synth: Synth::new(seed),
            right: None,
            cue: self.controls.cue.load(Ordering::Relaxed),
            tone: change_sound::Tone::default(),
        }
    }
}
impl Iterator for Decoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let cue = self.controls.cue.load(Ordering::Relaxed);
        if cue != self.cue {
            self.cue = cue;
            self.tone.trigger();
        }
        let tone = self.tone.frame(f32::from_bits(
            self.controls.cue_volume.load(Ordering::Relaxed),
        ));
        let [left, right] = self.synth.frame(self.controls.load());
        self.right = Some((right + tone).clamp(-1., 1.));
        Some((left + tone).clamp(-1., 1.))
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
    change_cue: change_sound::Gate,
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
            change_cue: change_sound::Gate::default(),
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
    live: Res<live_worktree::Live>,
    agent: Option<Res<agent_session::Session>>,
) {
    let (Ok(camera), Ok(window)) = (camera.single(), windows.single()) else {
        return;
    };
    let active = window.focused && !repo.0.files.is_empty();
    let cue_enabled = repo.0.is_worktree() && settings.file_changes && settings.volume > 0.;
    let conversation = agent.is_some_and(|agent| agent.voice_active());
    let speaking = runtime.voice.busy() || conversation;
    runtime.controls.cue_volume.store(
        (if cue_enabled && !speaking {
            settings.volume
        } else {
            0.
        })
        .to_bits(),
        Ordering::Relaxed,
    );
    if runtime.change_cue.update(
        &repo.0.root,
        live.change_generation(),
        cue_enabled,
        speaking,
        time.elapsed_secs_f64(),
    ) {
        runtime.controls.cue.fetch_add(1, Ordering::Relaxed);
    }
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
        master: if active && settings.ambience && !conversation {
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
        music: if active && !conversation {
            settings.volume * if runtime.voice.busy() { 0.12 } else { 1. }
        } else {
            0.
        },
        radio: settings.radio.code(),
    });
    let enabled =
        active && settings.speech && settings.volume > 0. && !review.blocked && !conversation;
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

#[cfg(test)]
mod change_tests {
    use super::*;

    #[test]
    fn local_change_audio_stays_off_for_new_and_existing_preferences() {
        let settings = Settings::default();
        assert!(!settings.file_changes && !settings.ambience && !settings.speech);
        let previous: Settings = serde_json::from_str(r#"{"ambience":true,"volume":0.4}"#).unwrap();
        assert!(!previous.file_changes);
        let enabled = Settings {
            file_changes: true,
            ..settings
        };
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&enabled).unwrap()).unwrap();
        assert!(restored.file_changes);
    }

    #[test]
    fn change_cue_reaches_the_stereo_decoder_with_ambience_off_and_obeys_mute() {
        let controls = Arc::new(Controls::default());
        let asset = CitySound {
            controls: controls.clone(),
        };
        let mut decoder = asset.decoder();
        controls
            .cue_volume
            .store(0.4_f32.to_bits(), Ordering::Relaxed);
        controls.cue.fetch_add(1, Ordering::Relaxed);
        controls.store(Levels::default());
        let samples = decoder.by_ref().take(44_100).collect::<Vec<_>>();
        assert!(samples.iter().map(|sample| sample.abs()).sum::<f32>() > 10.);
        assert!(samples.chunks_exact(2).all(|pair| pair[0] == pair[1]));
        controls.cue_volume.store(0, Ordering::Relaxed);
        // Let the gain smooth to silence before another save.
        decoder.by_ref().take(44_100).for_each(drop);
        controls.cue.fetch_add(1, Ordering::Relaxed);
        assert!(decoder.take(44_100).all(|sample| sample.abs() < 0.00001));
    }
}
