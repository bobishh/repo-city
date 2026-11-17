//! Review annotations are anchored to a snapshot, independent of the wall that displays them.
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

use bevy::input::mouse::MouseScrollUnit;
use serde::{Deserialize, Serialize};

use super::*;

#[cfg(not(target_arch = "wasm32"))]
mod color_picker;
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod mcp;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod ui;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use color_picker::update as update_color_picker;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use ui::{ReviewUi, update as panels};

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
        ("note", "Note", "Note", "", [255, 126, 141]),
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
    .chain(std::iter::repeat_n(None, 3))
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

    fn touches(&self, other: &Self) -> bool {
        self.path == other.path
            && self.before == other.before
            && self.start <= other.end.saturating_add(1)
            && other.start <= self.end.saturating_add(1)
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
    #[serde(default)]
    pub attention: bool,
}

impl Note {
    pub(super) fn comment(&self) -> &str {
        if self.text.trim().is_empty() {
            &self.preset.prompt
        } else {
            &self.text
        }
    }

    fn duplicate_key(&self) -> (&str, Option<&str>, &str, bool, usize, usize, &str, &str) {
        (
            &self.revision,
            self.baseline.as_deref(),
            &self.anchor.path,
            self.anchor.before,
            self.anchor.start,
            self.anchor.end,
            &self.preset.id,
            self.comment(),
        )
    }

    pub(super) fn ink(&self) -> [u8; 3] {
        if self.attention {
            [255, 202, 104]
        } else {
            self.preset.color
        }
    }
}

fn duplicate_drafts(notes: &[Note]) -> Vec<String> {
    let mut seen = HashSet::new();
    notes
        .iter()
        .filter(|note| !note.remote && note.published.is_none())
        .filter_map(|note| (!seen.insert(note.duplicate_key())).then(|| note.id.clone()))
        .collect()
}

#[derive(Deserialize)]
pub(super) struct Destination {
    pub anchor: Anchor,
    pub revision: String,
}

pub(super) fn take_destination(_review: Option<&mut Review>) -> Option<Destination> {
    #[cfg(target_arch = "wasm32")]
    return serde_json::from_str(&browser_destination()).ok();
    #[cfg(not(target_arch = "wasm32"))]
    _review?.destination.take()
}

#[derive(Serialize, Deserialize)]
struct Payload {
    palette: Vec<Option<Preset>>,
    notes: Vec<Note>,
}

#[derive(Component)]
pub(super) struct ToolHud;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
pub(super) struct EmptyNotesHint;

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn spawn_empty_hint(commands: &mut Commands, font: TextFont, ink: Color) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(50.),
                width: percent(100.),
                padding: UiRect::top(px(54.)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            EmptyNotesHint,
            Visibility::Hidden,
            bevy::picking::Pickable::IGNORE,
        ))
        .with_child((
            Text::new("No review notes yet. Choose a paint and spray the lines you want to mark."),
            font,
            TextColor(ink),
            BackgroundColor(Color::srgba(0.024, 0.066, 0.08, 0.95)),
            Node {
                max_width: percent(80.),
                padding: UiRect::axes(px(12.), px(8.)),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));
}

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
    pub tablet_open: bool,
    #[cfg(not(target_arch = "wasm32"))]
    destination: Option<Destination>,
    pub aim_note: Option<String>,
    pub phrase: Option<(f32, String)>,
    #[cfg(not(target_arch = "wasm32"))]
    pub auto_publish: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) auto_drafts: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub feedback: Option<(f32, String)>,
    #[cfg(not(target_arch = "wasm32"))]
    local_states: HashMap<String, native::LocalState>,
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
            tablet_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            destination: None,
            aim_note: None,
            phrase: None,
            #[cfg(not(target_arch = "wasm32"))]
            auto_publish: false,
            #[cfg(not(target_arch = "wasm32"))]
            auto_drafts: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            feedback: None,
            #[cfg(not(target_arch = "wasm32"))]
            local_states: HashMap::new(),
            scroll: 0.,
            version: 0,
            scope: String::new(),
        }
    }
}

impl Review {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn notify(&mut self, now: f32, text: impl Into<String>) {
        self.feedback = Some((now + 5., text.into()));
    }

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

    fn matching_paint(&self, repo: &Repository, anchor: &Anchor, preset: &str) -> Option<usize> {
        self.notes.iter().position(|note| {
            !note.remote
                && Self::on_snapshot(note, repo)
                && note.preset.id == preset
                && note.anchor.touches(anchor)
                && (note.published.is_none()
                    || (note.anchor.start <= anchor.start && note.anchor.end >= anchor.end))
        })
    }

    pub fn save_draft(&mut self, repo: &Repository, anchor: Anchor) -> Result<Note, String> {
        let Some(preset) = self.paint().cloned() else {
            return Err("Select a paint first.".into());
        };
        if let Some(index) = self.matching_paint(repo, &anchor, &preset.id) {
            let mut note = self.notes[index].clone();
            let range = note.anchor.extend(&anchor).expect("same source side");
            if range != note.anchor {
                note.anchor = range;
                self.write_note(&note)?;
                self.notes[index] = note.clone();
            }
            #[cfg(not(target_arch = "wasm32"))]
            if self.local_states.get(&note.id) == Some(&native::LocalState::Addressed) {
                native::mark_worktree_note(&self.scope, &note.id, native::LocalState::Unread)?;
                self.local_states.remove(&note.id);
            }
            return Ok(note);
        }
        let note = Note {
            id: new_id(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            anchor,
            text: String::new(),
            preset,
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        };
        self.write_note(&note)?;
        #[cfg(not(target_arch = "wasm32"))]
        if self.auto_publish {
            self.auto_drafts.push(note.id.clone());
        }
        self.notes.push(note.clone());
        Ok(note)
    }

    pub fn shows_paint(&self, note: &Note, repo: &Repository) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.local_states.get(&note.id) == Some(&native::LocalState::Addressed) {
            return false;
        }
        Self::on_snapshot(note, repo)
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
    navigation: Res<city_navigation::Navigation>,
    camera: Query<&Pilot, PlayerCamera>,
    listing: Option<Res<listing::Listing>>,
    #[cfg(not(target_arch = "wasm32"))] search: Option<Res<listing::find::Search>>,
    #[cfg(not(target_arch = "wasm32"))] comments: Option<Res<native_github::PullComments>>,
) {
    review.washing = false;
    review.tablet_open = false;
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
        review.tablet_open = browser_tablet();
        review.can_paint = browser_can_paint();
        if !review.can_paint {
            review.select(0);
        }
        if let Ok(slot) = usize::try_from(browser_slot()) {
            review.select(slot);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if repo.is_changed() && review.scope != repo.0.root.to_string_lossy() {
        review.scope = repo.0.root.to_string_lossy().into_owned();
        review.local_states.clear();
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            review.auto_publish = false;
            review.auto_drafts.clear();
            review.feedback = None;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if repo.is_changed()
        || comments
            .as_ref()
            .is_some_and(|comments| comments.is_changed())
    {
        review.notes.retain(|note| !note.remote);
        if let Some(native_github::PullComments::Loaded(Ok(imported))) = comments.as_deref() {
            for note in imported {
                if !review
                    .notes
                    .iter()
                    .any(|local| local.published == note.published)
                {
                    review.notes.push(note.clone());
                }
            }
        }
    }

    let Ok(pilot) = camera.single() else {
        return;
    };
    let blocked =
        review.blocked || picker.is_open() || menu.is_open() || keys.just_pressed(KeyCode::Escape);
    #[cfg(not(target_arch = "wasm32"))]
    let blocked = blocked || search.is_some_and(|search| search.blocks_controls());
    if blocked {
        review.drag = None;
    } else if keys.just_pressed(KeyCode::KeyJ)
        && !navigation.is_flying()
        && (pilot.captured || cfg!(not(target_arch = "wasm32")))
    {
        review.request = Some(Panel::Aimed);
    } else if pilot.captured {
        if keys.just_pressed(KeyCode::KeyU) && review.can_paint {
            review.request = Some(Panel::Paints);
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
    if navigation.guides_camera() {
        review.nozzle = 0.;
        return;
    }
    let spraying = if review.slot == 0 {
        pilot.firing || listing.as_ref().is_some_and(|listing| listing.water_on())
    } else {
        pilot.firing
    };
    let raised = spraying && !review.blocked && !picker.is_open() && !menu.is_open();
    let direction = if raised { 1. } else { -1. };
    review.nozzle = (review.nozzle + direction * time.delta_secs() / 0.11).clamp(0., 1.);
}

#[cfg(target_arch = "wasm32")]
pub(super) fn panels(mut review: ResMut<Review>, mut camera: Query<&mut Pilot, PlayerCamera>) {
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
    browser_panel(
        &serde_json::to_string(&panel).unwrap_or_default(),
        &serde_json::to_string(&review.palette).unwrap_or_default(),
        review.slot,
        review.aim_note.as_deref().unwrap_or(""),
    );
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
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewDestination)]
    fn browser_destination() -> String;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewModal)]
    fn browser_modal() -> bool;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = reviewTablet)]
    fn browser_tablet() -> bool;
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
    fn plain_note_paint_has_no_required_comment_template() {
        let palette = defaults();
        let paint = palette
            .iter()
            .flatten()
            .find(|paint| paint.id == "note")
            .expect("plain note paint");
        assert_eq!(paint.label, "Note");
        assert!(paint.prompt.is_empty());
    }

    #[test]
    fn repaint_extends_the_same_note_and_keeps_edited_text() {
        let repo = repository_picker::empty();
        let anchor = Anchor {
            path: "src/main.rs".into(),
            before: false,
            start: 3,
            end: 5,
        };
        let mut review = Review::default();
        let preset = review.palette[0].as_ref().unwrap().clone();
        let note = Note {
            id: "existing".into(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            anchor: anchor.clone(),
            preset,
            text: "Keep this explanation.".into(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        };
        review.notes.push(note.clone());
        let repeat = review.matching_paint(&repo, &anchor, &note.preset.id);
        assert_eq!(repeat, Some(0));
        let added = Anchor {
            start: 6,
            end: 7,
            ..anchor.clone()
        };
        assert_eq!(
            review.matching_paint(&repo, &added, &note.preset.id),
            Some(0)
        );
        assert_eq!(note.anchor.extend(&added).unwrap().end, 7);
        assert_eq!(review.notes[0].comment(), "Keep this explanation.");
        let old_duplicate = Note {
            id: "repeat".into(),
            ..note.clone()
        };
        let different_text = Note {
            id: "custom".into(),
            text: "Other context.".into(),
            ..note.clone()
        };
        assert_eq!(
            duplicate_drafts(&[note, old_duplicate, different_text]),
            ["repeat"]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_empty_notes_keeps_capture_and_the_hint_expires() {
        let mut app = App::new();
        app.init_resource::<ReviewUi>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Explorer::new(0))
            .insert_resource(Review {
                request: Some(Panel::Notes),
                ..default()
            })
            .add_systems(Update, panels);
        let hint = app
            .world_mut()
            .spawn((EmptyNotesHint, Visibility::Hidden))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(hint),
            Some(&Visibility::Inherited)
        );
        assert!(app.world().get::<Pilot>(camera).unwrap().captured);
        assert!(app.world().resource::<Review>().request.is_none());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(4));
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(hint),
            Some(&Visibility::Hidden)
        );
    }

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
