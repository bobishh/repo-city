//! Review annotations are anchored to a snapshot, independent of the wall that displays them.
use bevy::input::mouse::MouseScrollUnit;
use serde::{Deserialize, Serialize};

use super::*;

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Preset {
    pub id: String,
    pub label: String,
    pub spray: String,
    pub prompt: String,
    pub color: [u8; 3],
}

fn defaults() -> Vec<Option<Preset>> {
    [
        (
            "refactor",
            "Refactor",
            "Simplify",
            "Simplify this code while preserving behavior.",
            [255, 103, 181],
        ),
        (
            "tests",
            "Tests",
            "Add tests",
            "Cover the relevant behavior and edge cases.",
            [67, 224, 241],
        ),
        (
            "performance",
            "Performance",
            "Measure this",
            "Check the suspected bottleneck and support any change with evidence.",
            [255, 169, 85],
        ),
        (
            "bug",
            "Bug",
            "Check this",
            "Reproduce the issue and fix its cause.",
            [161, 244, 99],
        ),
        (
            "docs",
            "Explain or document",
            "Explain this",
            "Explain how this works or document the contract.",
            [191, 148, 255],
        ),
    ]
    .into_iter()
    .map(|(id, label, spray, prompt, color)| {
        Some(Preset {
            id: id.into(),
            label: label.into(),
            spray: spray.into(),
            prompt: prompt.into(),
            color,
        })
    })
    .chain(std::iter::repeat_n(None, 4))
    .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Anchor {
    pub path: String,
    pub before: bool,
    pub start: usize,
    pub end: usize,
}

impl Anchor {
    pub fn includes(&self, other: &Self) -> bool {
        self.path == other.path
            && self.before == other.before
            && (self.start..=self.end).contains(&other.start)
    }

    pub fn extend(&self, other: &Self) -> Option<Self> {
        (self.path == other.path && self.before == other.before).then(|| Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            ..self.clone()
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Note {
    pub id: String,
    pub revision: String,
    pub baseline: Option<String>,
    pub anchor: Anchor,
    pub preset: Preset,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub published: Option<String>,
    #[serde(default)]
    pub remote: bool,
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    palette: Vec<Option<Preset>>,
    notes: Vec<Note>,
}

#[derive(Component)]
pub(super) struct ToolHud;

#[derive(Resource)]
pub(super) struct Review {
    pub palette: Vec<Option<Preset>>,
    pub notes: Vec<Note>,
    pub slot: usize,
    pub drag: Option<(Anchor, Anchor)>,
    pub request: Option<Panel>,
    pub blocked: bool,
    pub can_paint: bool,
    pub nozzle: f32,
    pub washing: bool,
    pub aim_note: Option<String>,
    pub phrase: Option<(f32, String)>,
    scroll: f32,
    version: u32,
    scope: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Panel {
    Paints,
    Notes,
    Aimed,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            palette: defaults(),
            notes: vec![],
            slot: 0,
            drag: None,
            request: None,
            blocked: false,
            can_paint: !cfg!(target_arch = "wasm32"),
            nozzle: 0.,
            washing: false,
            aim_note: None,
            phrase: None,
            scroll: 0.,
            version: 0,
            scope: String::new(),
        }
    }
}

impl Review {
    pub fn paint(&self) -> Option<&Preset> {
        self.slot
            .checked_sub(1)
            .and_then(|index| self.palette.get(index)?.as_ref())
    }

    pub(super) fn select(&mut self, slot: usize) {
        if self.slot != slot
            && (slot == 0
                || self.can_paint && self.palette.get(slot - 1).is_some_and(Option::is_some))
        {
            self.slot = slot;
            self.drag = None;
        }
    }

    pub fn save_draft(&mut self, repo: &Repository, anchor: Anchor) -> Result<Note, String> {
        let Some(preset) = self.paint().cloned() else {
            return Err("Select a paint first.".into());
        };
        let note = Note {
            id: new_id(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            anchor,
            preset,
            text: String::new(),
            published: None,
            remote: false,
            resolved: None,
        };
        self.write_note(&note)?;
        self.notes.push(note.clone());
        Ok(note)
    }

    pub fn on_snapshot(note: &Note, repo: &Repository) -> bool {
        note.revision == repo.revision && note.baseline == repo.baseline
    }

    fn write_note(&self, note: &Note) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        return native::save(&self.scope, &note.id, note);
        #[cfg(target_arch = "wasm32")]
        {
            let error = browser_save(&serde_json::to_string(note).map_err(|e| e.to_string())?);
            if error.is_empty() { Ok(()) } else { Err(error) }
        }
    }
}

pub(super) fn command_scroll(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight])
}

const SLOTS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub(super) fn update(
    repo: Res<Repo>,
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut review: ResMut<Review>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    camera: Query<&Pilot, PlayerCamera>,
) {
    review.washing = false;
    #[cfg(target_arch = "wasm32")]
    {
        let version = browser_version();
        if review.version != version || repo.is_changed() {
            if let Ok(payload) = serde_json::from_str::<Payload>(&browser_read()) {
                review.palette = if payload.palette.len() == 9 {
                    payload.palette
                } else {
                    defaults()
                };
                review.notes = payload.notes;
                if review.slot > 0 && review.paint().is_none() {
                    review.slot = 0;
                }
            }
            review.version = version;
        }
        review.blocked = browser_modal();
        review.can_paint = browser_can_paint();
        if !review.can_paint {
            review.select(0);
        }
        if let Ok(slot) = usize::try_from(browser_slot()) {
            review.select(slot);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if repo.is_changed() {
        review.scope = repo.0.root.to_string_lossy().into_owned();
        match native::load(&review.scope) {
            Ok(payload) => {
                review.palette = payload.palette;
                review.notes = payload.notes;
            }
            Err(error) => {
                warn!("Could not load review notes: {error}");
                review.palette = defaults();
                review.notes.clear();
            }
        }
        review.slot = 0;
        review.drag = None;
    }

    let Ok(pilot) = camera.single() else {
        return;
    };
    if review.blocked || picker.is_open() || menu.is_open() || keys.just_pressed(KeyCode::Escape) {
        review.drag = None;
    } else if pilot.captured {
        if keys.just_pressed(KeyCode::KeyU) && review.can_paint {
            review.request = Some(Panel::Paints);
        } else if keys.just_pressed(KeyCode::KeyJ) {
            review.request = Some(Panel::Aimed);
        }
        if let Some(slot) = SLOTS.iter().position(|key| keys.just_pressed(*key)) {
            review.select(slot);
        }
        if command_scroll(&keys) {
            let delta = wheel.delta.y
                / if wheel.unit == MouseScrollUnit::Pixel {
                    56.
                } else {
                    1.
                };
            if delta != 0. {
                if review.scroll.signum() != delta.signum() {
                    review.scroll = 0.;
                }
                review.scroll += delta;
                if review.scroll.abs() >= 1. {
                    let direction = if review.scroll > 0. { 9 } else { 1 };
                    let next = (1..=10)
                        .map(|step| (review.slot + direction * step) % 10)
                        .find(|&slot| slot == 0 || review.palette[slot - 1].is_some());
                    if let Some(slot) = next {
                        review.select(slot);
                    }
                    review.scroll = 0.;
                }
            }
        } else {
            review.scroll = 0.;
        }
    }
    let raised = pilot.firing && !review.blocked && !picker.is_open() && !menu.is_open();
    let direction = if raised { 1. } else { -1. };
    review.nozzle = (review.nozzle + direction * time.delta_secs() / 0.11).clamp(0., 1.);
}

pub(super) fn panels(
    mut review: ResMut<Review>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<&mut Pilot, PlayerCamera>,
    #[cfg(not(target_arch = "wasm32"))] mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    #[cfg(not(target_arch = "wasm32"))] _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let Some(panel) = review.request.take() else {
        return;
    };
    if let Ok(mut pilot) = camera.single_mut() {
        pilot.captured = false;
        pilot.firing = false;
        pilot.shot = false;
        pilot.fire_ready = false;
    }
    review.drag = None;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = &mut explorer;
        browser_panel(
            &serde_json::to_string(&panel).unwrap_or_default(),
            &serde_json::to_string(&review.palette).unwrap_or_default(),
            review.slot,
            review.aim_note.as_deref().unwrap_or(""),
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(mut cursor) = cursor.single_mut() {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        if let Err(error) = native::panel(&mut review, panel) {
            explorer.notice = error;
        }
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewVersion)]
    fn browser_version() -> u32;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewRead)]
    fn browser_read() -> String;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewSave)]
    fn browser_save(note: &str) -> String;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewPanel)]
    fn browser_panel(panel: &str, palette: &str, slot: usize, note: &str);
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewSlot)]
    fn browser_slot() -> i32;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewModal)]
    fn browser_modal() -> bool;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewCanPaint)]
    fn browser_can_paint() -> bool;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewId)]
    fn new_id() -> String;
}

#[cfg(not(target_arch = "wasm32"))]
fn new_id() -> String {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "local-{stamp:x}-{:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_extends_only_within_one_file_and_side_and_can_move_back() {
        let first = Anchor {
            path: "src/a.rs".into(),
            before: false,
            start: 8,
            end: 8,
        };
        let point = |line| Anchor {
            start: line,
            end: line,
            ..first.clone()
        };
        let extended = first.extend(&point(13)).unwrap();
        assert_eq!(8..=13, extended.start..=extended.end);
        let reversed = first.extend(&point(4)).unwrap();
        assert_eq!(4..=8, reversed.start..=reversed.end);
        assert!(
            first
                .extend(&Anchor {
                    before: true,
                    ..point(9)
                })
                .is_none()
        );
        assert!(
            first
                .extend(&Anchor {
                    path: "src/b.rs".into(),
                    ..point(9)
                })
                .is_none()
        );
    }
}
