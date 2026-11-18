//! The browser receives bounded semantic activity; synthesis stays in its AudioWorklet.
use super::*;
use serde::Serialize;
use std::hash::{DefaultHasher, Hash, Hasher};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = audioActivity)]
    fn audio_activity(value: &str);
}

#[derive(Serialize)]
struct Focus {
    id: String,
    text: String,
    seconds: f32,
}

#[derive(Serialize)]
struct Activity {
    wind: f32,
    spray: f32,
    changed: bool,
    pan: f32,
    blocked: bool,
    #[serde(rename = "silenceSpeech")]
    silence_speech: bool,
    focus: Option<Focus>,
}

#[derive(Default)]
pub(super) struct Sampling {
    next: f64,
    previous: Option<(f64, Vec3)>,
}

pub(super) fn update(
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    nearest: Res<Nearest>,
    review: Res<review_notes::Review>,
    listing: Res<listing::Listing>,
    navigation: Res<city_navigation::Navigation>,
    filter: Res<file_facade::ChangesOnly>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Transform, PlayerCamera>,
    mut sampling: Local<Sampling>,
) {
    let now = time.elapsed_secs_f64();
    if now < sampling.next {
        return;
    }
    sampling.next = now + 0.2;
    let (Ok(camera), Ok(window)) = (camera.single(), windows.single()) else {
        return;
    };
    let speed = sampling
        .previous
        .replace((now, camera.translation))
        .map_or(0., |(at, position)| {
            position.distance(camera.translation) / (now - at).max(0.001) as f32
        });
    let blocked = review.blocked || navigation.is_flying();
    let focus = if blocked {
        None
    } else {
        repo.0
            .files
            .get(explorer.selected)
            .filter(|file| filter.includes(file))
            .and_then(|file| {
                let rows = listing.visible_source_rows(
                    file,
                    camera,
                    &explorer,
                    window.width() / window.height().max(1.),
                )?;
                let lines = listing
                    .visible_source_fragments(
                        file,
                        camera,
                        &explorer,
                        window.width() / window.height().max(1.),
                    )
                    .into_iter()
                    .take(24)
                    .map(|(_, text)| text.chars().take(200).collect::<String>())
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return None;
                }
                let text = lines.join("\n");
                let seconds = reading::Settings {
                    mode: reading::Mode::Delayed,
                    min_seconds: None,
                    max_seconds: None,
                    ..default()
                }
                .seconds(lines.iter().map(String::as_str));
                let mut hash = DefaultHasher::new();
                text.hash(&mut hash);
                Some(Focus {
                    id: format!(
                        "{}:{}:{}:{}:{}-{}:{:x}",
                        repo.0.revision,
                        file.path,
                        explorer.before,
                        explorer.selected,
                        rows.start,
                        rows.end,
                        hash.finish()
                    ),
                    text,
                    seconds,
                })
            })
    };
    let activity = Activity {
        wind: (0.15 + camera.translation.y.max(0.) / 250. + speed / 120.).clamp(0.15, 1.),
        spray: if !blocked { review.nozzle } else { 0. },
        changed: nearest
            .file
            .and_then(|aim| repo.0.files.get(aim.index))
            .is_some_and(|file| file.change != Change::Unchanged),
        pan: camera.forward().x,
        blocked,
        silence_speech: review.blocked,
        focus,
    };
    if let Ok(json) = serde_json::to_string(&activity) {
        audio_activity(&json);
    }
}
