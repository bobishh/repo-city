//! Viewport-sized comparison canvas shared by the city and library tablets.
mod annotation;
mod content;
#[cfg(target_arch = "wasm32")]
mod geometry;
mod model;
mod view;

use bevy::{
    input::{ButtonState, keyboard::KeyboardInput, mouse::MouseScrollUnit},
    ui::UiTargetCamera,
};
use std::collections::HashMap;

use super::*;
use annotation::{Document, Selection};
use content::Content;
use model::{Book, Scope};

#[derive(Clone, Eq, PartialEq)]
struct SourceAnchor {
    path: String,
    line: Option<usize>,
    before: bool,
}

const RAISE: f32 = 0.38;
const EXPAND: f32 = 0.25;
const FULL: f32 = RAISE + EXPAND;

pub(super) enum Open {
    Changed,
    Files(Vec<String>),
    Choose,
}

#[derive(Default)]
enum Dialog {
    #[default]
    None,
    Canvases(Option<Vec<String>>),
    New(Vec<String>),
    Document(Document),
    Paths {
        query: String,
        selected: HashSet<String>,
        offset: usize,
    },
}

#[derive(Component, Clone)]
enum Action {
    Close,
    Canvases,
    Choose,
    New,
    Use(usize),
    Create,
    Cancel,
    SourcePage(isize),
    Toggle(String),
    Add,
    Tile(String),
    Hunk(String, bool),
    Scroll(String, isize),
    Columns(String, isize),
    Zoom(f32, Vec2),
    Pan(Vec2),
    Resize(String, Vec2),
    Select(Selection),
    Comment,
    SaveComment,
    EditLines,
    Overview,
    #[cfg(not(target_arch = "wasm32"))]
    AgentVoice,
    #[cfg(not(target_arch = "wasm32"))]
    AgentFollow,
}

#[derive(Resource, Default)]
pub(super) struct Canvas {
    request: Option<Open>,
    book: Option<Book>,
    contents: HashMap<String, Content>,
    focus: Option<String>,
    selection: Option<Selection>,
    document_rendered: bool,
    document_input: Option<Entity>,
    document_feedback: Option<Entity>,
    #[cfg(target_arch = "wasm32")]
    pending_edit: Option<Selection>,
    overview: bool,
    reveal: Option<SourceAnchor>,
    last_reveal: Option<SourceAnchor>,
    pending_reveal: Option<SourceAnchor>,
    return_to_scene: bool,
    dialog: Dialog,
    name: String,
    status: String,
    target: bool,
    elapsed: f32,
    root: Option<Entity>,
    camera: Option<Entity>,
    body: Option<Entity>,
    actions: Vec<Action>,
    dirty: bool,
    save_pending: bool,
    save_age: f32,
    shown_in_tablet: bool,
}

pub(super) fn open(commands: &mut Commands, request: Open) {
    commands.queue(move |world: &mut World| {
        if let Some(mut canvas) = world.get_resource_mut::<Canvas>() {
            canvas.request = Some(request);
        }
    });
}

pub(super) fn active(canvas: Option<Res<Canvas>>) -> bool {
    canvas.is_some_and(|canvas| canvas.is_open())
}

impl Canvas {
    pub(super) fn is_open(&self) -> bool {
        self.request.is_some() || self.target || self.elapsed > 0.
    }

    pub(super) fn typing(&self) -> bool {
        self.target
            && matches!(
                self.dialog,
                Dialog::New(_) | Dialog::Paths { .. } | Dialog::Document(_)
            )
    }

    pub(super) fn raises_tablet(&self) -> bool {
        self.request.is_some() || self.target || self.elapsed > RAISE
    }

    pub(super) fn hides_tablet(&self) -> bool {
        self.elapsed >= FULL
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn smoke_shows(&self, path: &str) -> bool {
        self.hides_tablet() && self.model().tiles.iter().any(|tile| tile.path == path)
    }

    pub(super) fn overlay_camera(&self) -> Option<Entity> {
        self.camera.filter(|_| self.hides_tablet())
    }

    pub(super) fn show_change(&mut self, path: &str, line: Option<usize>) {
        self.queue_anchor(path, line, false);
    }

    pub(super) fn show_anchor(&mut self, path: &str, line: usize, before: bool) -> bool {
        if !self.target {
            return false;
        }
        self.reveal = Some(SourceAnchor {
            path: path.to_owned(),
            line: Some(line),
            before,
        });
        true
    }

    fn queue_anchor(&mut self, path: &str, line: Option<usize>, before: bool) -> bool {
        if !self.target {
            return false;
        }
        let anchor = SourceAnchor {
            path: path.to_owned(),
            line,
            before,
        };
        if self.last_reveal.as_ref() != Some(&anchor) {
            self.reveal = Some(anchor);
        }
        true
    }

    fn model(&self) -> &model::Canvas {
        self.book.as_ref().unwrap().active()
    }

    fn model_mut(&mut self) -> &mut model::Canvas {
        self.book.as_mut().unwrap().active_mut()
    }

    fn save(&mut self) {
        self.save_pending = false;
        self.save_age = 0.;
        if let Some(book) = self.book.as_ref()
            && let Err(error) = book.save()
        {
            self.status = error;
            self.dirty = true;
        }
    }

    fn edit(&mut self) {
        self.dirty = true;
        self.save_pending = true;
        self.save_age = 0.;
    }
}

pub(super) fn register(app: &mut App) {
    let tablet_pose = android::show_tablet.after(review_notes::refresh);
    #[cfg(not(target_arch = "wasm32"))]
    let tablet_pose = tablet_pose.after(review_tablet::update);
    app.init_resource::<Canvas>()
        .add_systems(Update, tablet_pose.run_if(repository_picker::has_city));
    #[cfg(target_arch = "wasm32")]
    app.add_systems(
        PostUpdate,
        geometry::publish.after(bevy::ui::UiSystems::PostLayout),
    );
    app.add_systems(
        Update,
        (update, input, view::render, view::world_camera)
            .chain()
            .after(android::show_tablet)
            .before(tablet_display::toggle),
    );
}

fn update(
    mut canvas: ResMut<Canvas>,
    repo: Res<Repo>,
    review: Res<review_notes::Review>,
    time: Res<Time>,
    mut commands: Commands,
    mut previous_window: Local<Vec2>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut pilots: Query<&mut Pilot, PlayerCamera>,
    #[cfg(target_arch = "wasm32")] mut source_version: Local<u32>,
) {
    if !canvas.is_open() {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let size = Vec2::new(window.width(), window.height());
    if *previous_window != size {
        *previous_window = size;
        canvas.dirty = true;
    }
    if canvas.request.is_some() || repo.is_changed() {
        let scope = Scope::for_repository(&repo.0);
        if canvas
            .book
            .as_ref()
            .is_none_or(|book| book.scope() != &scope)
        {
            if canvas.save_pending {
                canvas.save();
            }
            let (book, error) = Book::open(scope, &repo.0);
            canvas.book = Some(book);
            canvas.status = error.unwrap_or_default();
            canvas.contents.clear();
            canvas.pending_reveal = None;
            canvas.focus = None;
            canvas.selection = None;
            canvas.dialog = Dialog::None;
            canvas.dirty = true;
        } else {
            if canvas.book.as_mut().unwrap().refresh(&repo.0) {
                canvas.edit();
            }
            if canvas
                .selection
                .as_ref()
                .is_some_and(|selection| selection.index(&repo.0).is_err())
            {
                canvas.selection = None;
            }
            canvas.dirty = true;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let current = repo_world::browser_source_version();
        if *source_version != current {
            *source_version = current;
            let mut pending = false;
            for content in canvas.contents.values_mut() {
                pending |= content.retry_if_pending();
            }
            canvas.dirty |= pending;
            if let Some(selection) = canvas.pending_edit.take() {
                start_edit(&mut canvas, &repo.0, &selection);
            }
        }
    }
    if let Some(request) = canvas.request.take() {
        if !canvas.target {
            canvas.shown_in_tablet = review.tablet_open;
            canvas.last_reveal = None;
            if canvas.shown_in_tablet {
                canvas.elapsed = canvas.elapsed.max(RAISE);
            }
            if let Ok(mut pilot) = pilots.single_mut() {
                pilot.captured = false;
                pilot.fire_ready = false;
                pilot.firing = false;
                pilot.shot = false;
            }
            browser_open(true);
        }
        canvas.target = true;
        canvas.overview = false;
        canvas.selection = None;
        canvas.dialog = Dialog::None;
        canvas.pending_reveal = None;
        canvas.return_to_scene = !canvas.shown_in_tablet && matches!(request, Open::Files(_));
        match request {
            Open::Changed => {
                if canvas.book.as_mut().unwrap().select(0) {
                    canvas.edit();
                }
                canvas.focus = None;
            }
            Open::Files(paths) => canvas.dialog = Dialog::Canvases(Some(paths)),
            Open::Choose => canvas.dialog = path_dialog(),
        }
        canvas.dirty = true;
    }
    if let Some(anchor) = canvas.reveal.take() {
        let path = &anchor.path;
        if !canvas.model().tiles.iter().any(|tile| &tile.path == path)
            && canvas
                .book
                .as_mut()
                .unwrap()
                .add_paths(&repo.0, [path.as_str()])
        {
            canvas.edit();
        }
        if let Some(index) = canvas
            .model()
            .tiles
            .iter()
            .position(|tile| &tile.path == path)
        {
            canvas.last_reveal = Some(anchor.clone());
            let content = canvas.contents.entry(path.clone()).or_default();
            content.refresh(&repo.0, &path);
            let row = content.row_at_anchor(anchor.line, anchor.before);
            let pending = content.pending();
            canvas.pending_reveal = pending.then_some(anchor.clone());
            let tile = &mut canvas.model_mut().tiles[index];
            tile.viewport.row = row;
            tile.positioned = !pending;
            let layout = view::Layout::new(size, canvas.model());
            canvas.model_mut().pan = layout.reveal(index).to_array();
            canvas.overview = false;
            canvas.focus = Some(anchor.path);
            canvas.edit();
        }
    }
    let dt = time.delta_secs();
    if canvas.save_pending {
        canvas.save_age += dt;
        if canvas.save_age >= 0.6 {
            canvas.save();
        }
    }
    let minimum = if canvas.shown_in_tablet { RAISE } else { 0. };
    if canvas.target {
        canvas.elapsed = (canvas.elapsed + dt).min(FULL);
    } else {
        canvas.elapsed = (canvas.elapsed - dt).max(minimum);
        if canvas.elapsed <= minimum {
            canvas.elapsed = 0.;
            canvas.dialog = Dialog::None;
            if let Some(root) = canvas.root.take() {
                commands.entity(root).despawn();
            }
            canvas.body = None;
            if let Some(camera) = canvas.camera.take() {
                commands.entity(camera).despawn();
            }
            browser_open(false);
        }
    }
}

fn path_dialog() -> Dialog {
    Dialog::Paths {
        query: String::new(),
        selected: HashSet::new(),
        offset: 0,
    }
}

fn browser_open(open: bool) {
    #[cfg(target_arch = "wasm32")]
    if let Some(document) = web_sys::window().and_then(|window| window.document())
        && let Some(canvas) = document.get_element_by_id("repo-city")
    {
        if open {
            document.exit_pointer_lock();
            let _ = canvas.set_attribute("data-diff-canvas", "");
        } else {
            let _ = canvas.remove_attribute("data-diff-canvas");
            let _ = canvas.remove_attribute("data-diff-canvas-files");
            let _ = canvas.remove_attribute("data-diff-canvas-status");
            let _ = canvas.remove_attribute("data-diff-canvas-layout");
            let _ = canvas.remove_attribute("data-diff-canvas-editor");
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = open;
}

enum Gesture {
    Pan,
    Resize(String, Vec2),
    Select(Selection),
}

fn selection_at(
    canvas: &Canvas,
    layout: &view::Layout,
    repo: &Repository,
    point: Vec2,
) -> Option<Selection> {
    let index = layout.tile_body_at(point)?;
    let tile = &canvas.model().tiles[index];
    let content = canvas.contents.get(&tile.path)?;
    let row = layout.row_at(point, index);
    let lines = content.lines(content.first_visible(tile), row + 1);
    let line = lines.get(row)?;
    let file = tile.file(repo)?;
    let blob = if line.before {
        file.before.as_ref()?
    } else {
        file.after.as_ref()?
    };
    let line_number = line.line + 1;
    Some(Selection {
        anchor: review_notes::Anchor {
            path: tile.path.clone(),
            before: line.before,
            start: line_number,
            end: line_number,
        },
        blob: blob.id.clone(),
    })
}

fn input(
    mut canvas: ResMut<Canvas>,
    repo: Res<Repo>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut drag: Local<Option<(Gesture, Vec2)>>,
    (fields, mut review): (
        Query<&bevy::text::EditableText>,
        ResMut<review_notes::Review>,
    ),
    window: Query<&Window, With<PrimaryWindow>>,
    mut typing: MessageReader<KeyboardInput>,
    mut touches: MessageReader<bevy::input::touch::TouchInput>,
    mut touch_drag: Local<Option<(u64, Vec2, f32, Option<String>)>>,
    #[cfg(not(target_arch = "wasm32"))] mut session: Option<ResMut<agent_session::Session>>,
) {
    if !canvas.target {
        typing.clear();
        touches.clear();
        *touch_drag = None;
        *drag = None;
        return;
    }
    let Ok(window) = window.single() else { return };
    let size = Vec2::new(window.width(), window.height());
    let mut actions = std::mem::take(&mut canvas.actions);
    if keys.just_pressed(KeyCode::Escape) {
        actions.push(if matches!(canvas.dialog, Dialog::None) {
            Action::Close
        } else {
            Action::Cancel
        });
    }
    if matches!(canvas.dialog, Dialog::New(_) | Dialog::Paths { .. }) {
        let target = match &mut canvas.dialog {
            Dialog::Paths { query, offset, .. } => {
                if keys.just_pressed(KeyCode::Backspace) {
                    *offset = 0;
                }
                query
            }
            _ => &mut canvas.name,
        };
        let mut typed = false;
        for event in typing.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            if event.key_code == KeyCode::Backspace {
                typed |= target.pop().is_some();
            } else if let Some(text) = &event.text {
                for character in text.chars().filter(|character| !character.is_control()) {
                    if target.len() < 256 {
                        target.push(character);
                        typed = true;
                    }
                }
            }
        }
        if typed {
            if let Dialog::Paths { offset, .. } = &mut canvas.dialog {
                *offset = 0;
            }
            canvas.dirty = true;
        }
        if keys.just_pressed(KeyCode::Enter) {
            actions.push(if matches!(canvas.dialog, Dialog::New(_)) {
                Action::Create
            } else {
                Action::Add
            });
        }
    } else {
        typing.clear();
    }
    if matches!(canvas.dialog, Dialog::None) && canvas.elapsed >= FULL {
        let layout = view::Layout::for_view(size, canvas.model(), canvas.overview);
        let cursor = window.cursor_position().unwrap_or(size / 2.);
        let index = layout.tile_at(cursor);
        let hovered = layout
            .tile_body_at(cursor)
            .map(|index| canvas.model().tiles[index].path.clone());
        let control = keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if !canvas.overview && buttons.just_pressed(MouseButton::Left) && cursor.y >= layout.top {
            let gesture = match index {
                Some(index) if layout.grip_at(cursor, index) => {
                    let tile = &canvas.model().tiles[index];
                    Some(Gesture::Resize(
                        tile.path.clone(),
                        layout.rect(index).1 / layout.zoom,
                    ))
                }
                _ if hovered.is_some() => {
                    selection_at(&canvas, &layout, &repo.0, cursor).map(|selection| {
                        actions.push(Action::Select(selection.clone()));
                        Gesture::Select(selection)
                    })
                }
                Some(index)
                    if cursor.y - layout.top
                        >= layout.rect(index).0.y + layout.rect(index).1.y - view::TILE_FOOTER =>
                {
                    None
                }
                _ => Some(Gesture::Pan),
            };
            *drag = gesture.map(|gesture| (gesture, cursor));
        }
        if let Some((gesture, previous)) = drag.as_mut()
            && cursor != *previous
            && (buttons.pressed(MouseButton::Left) || buttons.just_released(MouseButton::Left))
        {
            let delta = cursor - *previous;
            *previous = cursor;
            match gesture {
                Gesture::Pan => actions.push(Action::Pan(-delta)),
                Gesture::Resize(path, dimensions) => {
                    *dimensions = (*dimensions + delta / layout.zoom)
                        .clamp(Vec2::new(280., 175.), Vec2::splat(2400.));
                    actions.push(Action::Resize(path.clone(), *dimensions));
                }
                Gesture::Select(origin) => {
                    if let Some(end) = selection_at(&canvas, &layout, &repo.0, cursor)
                        && origin.blob == end.blob
                        && let Some(anchor) = origin.anchor.extend(&end.anchor)
                    {
                        actions.push(Action::Select(Selection {
                            anchor,
                            blob: origin.blob.clone(),
                        }));
                    }
                }
            }
        }
        if !buttons.pressed(MouseButton::Left) {
            *drag = None;
        }
        if !control && !canvas.overview {
            let held = |codes| f32::from(keys.any_pressed(codes));
            let movement = Vec2::new(
                held([KeyCode::KeyD, KeyCode::ArrowRight])
                    - held([KeyCode::KeyA, KeyCode::ArrowLeft]),
                held([KeyCode::KeyS, KeyCode::ArrowDown]) - held([KeyCode::KeyW, KeyCode::ArrowUp]),
            );
            if movement != Vec2::ZERO {
                actions.push(Action::Pan(movement.normalize() * time.delta_secs() * 850.));
            }
        }
        if wheel.delta != Vec2::ZERO && (control || !canvas.overview) {
            if control {
                actions.push(Action::Zoom(
                    wheel.delta.y.signum(),
                    cursor - Vec2::Y * layout.top,
                ));
            } else if shift {
                actions.push(Action::Pan(
                    Vec2::new(-wheel.delta.x - wheel.delta.y, 0.) * 68.,
                ));
            } else if wheel.delta.x.abs() > wheel.delta.y.abs() {
                if let Some(path) = hovered {
                    let scale = match wheel.unit {
                        MouseScrollUnit::Line => 6.,
                        MouseScrollUnit::Pixel => 0.12,
                    };
                    actions.push(Action::Columns(
                        path,
                        (-wheel.delta.x * scale).round() as isize,
                    ));
                } else {
                    actions.push(Action::Pan(Vec2::new(-wheel.delta.x, 0.) * 68.));
                }
            } else if let Some(path) = hovered {
                let scale = match wheel.unit {
                    MouseScrollUnit::Line => 3.,
                    MouseScrollUnit::Pixel => 0.08,
                };
                actions.push(Action::Scroll(
                    path,
                    (-wheel.delta.y * scale).round() as isize,
                ));
            } else {
                actions.push(Action::Pan(Vec2::new(0., -wheel.delta.y) * 68.));
            }
        }
        for event in touches.read() {
            if canvas.overview {
                *touch_drag = None;
                continue;
            }
            use bevy::input::touch::TouchPhase;
            match event.phase {
                TouchPhase::Started if touch_drag.is_none() => {
                    let path = layout
                        .tile_body_at(event.position)
                        .map(|index| canvas.model().tiles[index].path.clone());
                    *touch_drag = Some((event.id, event.position, 0., path));
                }
                TouchPhase::Moved
                    if let Some((id, previous, carry, path)) = touch_drag.as_mut()
                        && *id == event.id =>
                {
                    let delta = *previous - event.position;
                    *previous = event.position;
                    actions.push(Action::Pan(Vec2::new(
                        delta.x,
                        if path.is_none() { delta.y } else { 0. },
                    )));
                    if let Some(path) = path {
                        *carry += delta.y / 17.;
                        let rows = carry.trunc();
                        *carry -= rows;
                        if rows != 0. {
                            actions.push(Action::Scroll(path.clone(), rows as isize));
                        }
                    }
                }
                TouchPhase::Ended | TouchPhase::Canceled
                    if touch_drag
                        .as_ref()
                        .is_some_and(|(id, _, _, _)| *id == event.id) =>
                {
                    *touch_drag = None;
                }
                _ => {}
            }
        }
        if !canvas.overview
            && let Some(path) = canvas.focus.as_ref()
        {
            for (key, rows) in [(KeyCode::PageDown, 15), (KeyCode::PageUp, -15)] {
                if keys.just_pressed(key) {
                    actions.push(Action::Scroll(path.clone(), rows));
                }
            }
            for (key, next) in [(KeyCode::BracketLeft, false), (KeyCode::BracketRight, true)] {
                if keys.just_pressed(key) {
                    actions.push(Action::Hunk(path.clone(), next));
                }
            }
        }
        let center = layout.viewport / 2.;
        if control && keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
            actions.push(Action::Zoom(1., center));
        }
        if control && keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
            actions.push(Action::Zoom(-1., center));
        }
        if control && keys.just_pressed(KeyCode::Digit0) {
            actions.push(Action::Overview);
        }
    } else {
        touches.clear();
        *touch_drag = None;
        *drag = None;
    }
    for action in actions {
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(action, Action::AgentVoice | Action::AgentFollow) {
            if let Some(session) = session.as_deref_mut() {
                match action {
                    Action::AgentVoice => session.toggle_canvas_voice(),
                    Action::AgentFollow => session.toggle_canvas_follow(),
                    _ => unreachable!(),
                }
            }
            continue;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(
            action,
            Action::Tile(_)
                | Action::Scroll(..)
                | Action::Columns(..)
                | Action::Hunk(..)
                | Action::Pan(_)
                | Action::Zoom(..)
                | Action::Resize(..)
                | Action::Select(_)
                | Action::Overview
        ) && let Some(session) = session.as_deref_mut()
        {
            session.pause_following();
        }
        if matches!(
            action,
            Action::Comment | Action::EditLines | Action::SaveComment
        ) {
            annotate(&mut canvas, &repo.0, &mut review, &fields, action);
        } else {
            apply(&mut canvas, &repo.0, size, action);
        }
    }
}

fn apply(canvas: &mut Canvas, repo: &Repository, size: Vec2, action: Action) {
    #[cfg(target_arch = "wasm32")]
    if matches!(
        action,
        Action::Close
            | Action::Select(_)
            | Action::Choose
            | Action::Canvases
            | Action::Use(_)
            | Action::Overview
    ) {
        canvas.pending_edit = None;
    }
    if matches!(
        action,
        Action::Close
            | Action::Use(_)
            | Action::Create
            | Action::Tile(_)
            | Action::Scroll(..)
            | Action::Hunk(..)
    ) {
        canvas.pending_reveal = None;
    }
    canvas.dirty = true;
    match action {
        Action::Comment | Action::EditLines | Action::SaveComment => unreachable!(),
        #[cfg(not(target_arch = "wasm32"))]
        Action::AgentVoice | Action::AgentFollow => unreachable!(),
        Action::Close => {
            canvas.target = false;
            if canvas.save_pending {
                canvas.save();
            }
        }
        Action::Canvases => canvas.dialog = Dialog::Canvases(None),
        Action::Choose => canvas.dialog = path_dialog(),
        Action::Cancel => {
            canvas.dialog = Dialog::None;
            if canvas.return_to_scene {
                canvas.target = false;
            }
        }
        Action::New => {
            let dialog = std::mem::take(&mut canvas.dialog);
            if let Dialog::Canvases(paths) = dialog {
                canvas.dialog = Dialog::New(paths.unwrap_or_default());
                canvas.name.clear();
                canvas.status.clear();
            }
        }
        Action::Use(index) => {
            let dialog = std::mem::take(&mut canvas.dialog);
            let book = canvas.book.as_mut().unwrap();
            book.select(index);
            if let Dialog::Canvases(Some(paths)) = dialog {
                book.add_paths(repo, paths.iter().map(String::as_str));
            }
            canvas.focus = None;
            canvas.selection = None;
            canvas.edit();
            if canvas.return_to_scene {
                canvas.target = false;
                canvas.save();
            }
        }
        Action::Create => {
            let Dialog::New(paths) = &canvas.dialog else {
                return;
            };
            let book = canvas.book.as_mut().unwrap();
            match book.create(&canvas.name) {
                Ok(()) => {
                    book.add_paths(repo, paths.iter().map(String::as_str));
                    canvas.dialog = Dialog::None;
                    canvas.focus = None;
                    canvas.selection = None;
                    canvas.status.clear();
                    canvas.edit();
                    if canvas.return_to_scene {
                        canvas.target = false;
                        canvas.save();
                    }
                }
                Err(error) => canvas.status = error,
            }
        }
        Action::SourcePage(direction) => {
            if let Dialog::Paths { offset, .. } = &mut canvas.dialog {
                *offset = offset.saturating_add_signed(direction);
            }
        }
        Action::Toggle(path) => {
            if let Dialog::Paths { selected, .. } = &mut canvas.dialog
                && !selected.remove(&path)
            {
                selected.insert(path);
            }
        }
        Action::Add => {
            if let Dialog::Paths { selected, .. } = &canvas.dialog
                && !selected.is_empty()
            {
                // Preserve repository order for selected paths.
                canvas.dialog = Dialog::Canvases(Some(
                    repo.files
                        .iter()
                        .filter(|file| selected.contains(&file.path))
                        .map(|file| file.path.clone())
                        .collect(),
                ));
            }
        }
        Action::Tile(path) => {
            if canvas.overview || canvas.model().zoom < 0.42 {
                canvas.overview = false;
                if canvas.model().zoom < 0.42 {
                    canvas.model_mut().zoom = 1.;
                }
                if let Some(index) = canvas
                    .model()
                    .tiles
                    .iter()
                    .position(|tile| tile.path == path)
                {
                    let reading = view::Layout::new(size, canvas.model());
                    canvas.model_mut().pan = reading.reveal(index).to_array();
                    canvas.edit();
                }
            }
            canvas.focus = Some(path);
        }
        Action::Select(selection) => {
            canvas.focus = Some(selection.anchor.path.clone());
            canvas.selection = Some(selection);
            canvas.status.clear();
        }
        Action::Hunk(path, next) => {
            if let Some(content) = canvas.contents.get(&path) {
                let current = canvas
                    .model()
                    .tiles
                    .iter()
                    .find(|tile| tile.path == path)
                    .filter(|tile| tile.positioned)
                    .map_or_else(|| content.row_at_line(None), |tile| tile.viewport.row);
                let row = content.hunk(current, next);
                if let Some(tile) = canvas
                    .model_mut()
                    .tiles
                    .iter_mut()
                    .find(|tile| tile.path == path)
                {
                    tile.viewport.row = row;
                    tile.positioned = true;
                }
                canvas.focus = Some(path);
                canvas.edit();
            }
        }
        Action::Scroll(path, delta) => {
            let (rows, first) = canvas
                .contents
                .get(&path)
                .map_or((0, 0), |content| (content.len(), content.row_at_line(None)));
            if rows > 0
                && delta != 0
                && let Some(tile) = canvas
                    .model_mut()
                    .tiles
                    .iter_mut()
                    .find(|tile| tile.path == path)
            {
                let current = if tile.positioned {
                    tile.viewport.row
                } else {
                    first
                };
                tile.viewport.row = current
                    .saturating_add_signed(delta)
                    .min(rows.saturating_sub(1));
                tile.positioned = true;
                canvas.focus = Some(path);
                canvas.edit();
            }
        }
        Action::Columns(path, delta) => {
            if let Some(tile) = canvas
                .model_mut()
                .tiles
                .iter_mut()
                .find(|tile| tile.path == path)
            {
                tile.viewport.column = tile
                    .viewport
                    .column
                    .saturating_add_signed(delta)
                    .min(32_000);
                canvas.focus = Some(path);
                canvas.edit();
            }
        }
        Action::Resize(path, extent) => {
            if let Some(tile) = canvas
                .model_mut()
                .tiles
                .iter_mut()
                .find(|tile| tile.path == path)
            {
                tile.size = extent.to_array();
                canvas.edit();
            }
        }
        Action::Zoom(delta, center) => {
            let view = view::Layout::for_view(size, canvas.model(), canvas.overview);
            canvas.overview = false;
            let model = canvas.model_mut();
            model.zoom = (view.zoom * 1.22_f32.powf(delta)).clamp(0.025, 2.5);
            let pan = (view.pan + center) * (model.zoom / view.zoom) - center;
            model.pan = view::Layout::clamp(pan).to_array();
            canvas.edit();
        }
        Action::Pan(delta) => {
            let model = canvas.model_mut();
            model.pan = view::Layout::clamp(Vec2::from_array(model.pan) + delta).to_array();
            canvas.edit();
        }
        Action::Overview => canvas.overview = !canvas.overview,
    }
}

fn annotate(
    canvas: &mut Canvas,
    repo: &Repository,
    review: &mut review_notes::Review,
    fields: &Query<&bevy::text::EditableText>,
    action: Action,
) {
    let result = (|| -> Result<(), String> {
        if matches!(action, Action::SaveComment) {
            let Dialog::Document(Document::Comment(selection, _)) = &canvas.dialog else {
                return Ok(());
            };
            selection.index(repo)?;
            let text = canvas
                .document_input
                .and_then(|input| fields.get(input).ok())
                .map(|field| field.value().to_string())
                .unwrap_or_default();
            review.save_comment(repo, selection.anchor.clone(), selection.blob.clone(), text)?;
            canvas.dialog = Dialog::None;
            return Ok(());
        }
        let Some(selection) = &canvas.selection else {
            return Ok(());
        };
        let document = if matches!(action, Action::Comment) {
            #[cfg(target_arch = "wasm32")]
            if !review.can_paint {
                return Err("Open a pull request and sign in to comment.".into());
            }
            let text = review
                .local_comment(repo, &selection.anchor)
                .map(|note| note.comment().to_owned())
                .unwrap_or_default();
            Document::Comment(selection.clone(), text)
        } else {
            let selection = selection.clone();
            start_edit(canvas, repo, &selection);
            return if canvas.status.is_empty() {
                Ok(())
            } else {
                Err(canvas.status.clone())
            };
        };
        canvas.dialog = Dialog::Document(document);
        Ok(())
    })();
    canvas.status = result.err().unwrap_or_default();
    canvas.dirty = true;
}

fn start_edit(canvas: &mut Canvas, repo: &Repository, selection: &Selection) {
    canvas.dirty = true;
    match Document::edit(repo, selection) {
        Ok(document) => {
            canvas.status.clear();
            canvas.dialog = Dialog::Document(document);
        }
        Err(error) => {
            #[cfg(target_arch = "wasm32")]
            if error.starts_with("Loading source from GitHub") {
                canvas.pending_edit = Some(selection.clone());
            }
            canvas.status = error;
        }
    }
}
