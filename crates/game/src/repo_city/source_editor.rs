//! Shared VibePad source editor. stb owns editing; existing wall syntax and fonts own pixels.
mod drafts;
mod kernel;
mod keyboard;

use super::*;
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    picking::events::{PointerDrag, PointerPress},
    ui_widgets::{Activate, observe},
};
pub(super) use drafts::{ApplyTarget, Identity as DraftIdentity};
const FONT: f32 = 18.;
const CELL: f32 = 10.8;
const ROW: f32 = 25.;
const GUTTER: f32 = 58.;
const TOP: f32 = 8.;

#[derive(Component)]
pub(super) struct View {
    identity: drafts::Identity,
    buffer: kernel::Buffer,
    original: String,
    full_source: String,
    bytes: std::ops::Range<usize>,
    text: String,
    status: String,
    persistence_blocked: bool,
    apply: Option<ApplyTarget>,
    image: Option<Handle<Image>>,
    size: UVec2,
    top: usize,
    dirty: bool,
    changed: bool,
    colors: Vec<syntax::Kind>,
    syntax_version: u32,
    focused: bool,
    composing: bool,
    find_input: Option<Entity>,
    find_text: String,
    find_hit: Option<usize>,
    unsaved: bool,
    restore_memory: bool,
}
impl View {
    pub(super) fn range(&self) -> std::ops::Range<usize> {
        self.identity.range.clone()
    }
    pub(super) fn full_value(&self) -> String {
        let mut full = self.full_source.clone();
        full.replace_range(self.bytes.clone(), &self.text);
        full
    }
    pub(super) fn value(&self) -> &str {
        &self.text
    }
    pub(super) fn take_change(&mut self) -> Option<String> {
        std::mem::take(&mut self.changed).then(|| self.text.clone())
    }
    fn sync(&mut self) {
        let value = self.buffer.text();
        if value != self.text {
            self.text = value;
            self.changed = true;
            self.dirty = true;
            self.recolor();
            self.save();
        }
    }
    fn save(&mut self) -> bool {
        if self.persistence_blocked {
            self.status = "Saved draft could not be read and was kept intact. Copy edits or discard the saved draft before saving.".into();
            self.unsaved = true;
            return false;
        }
        let saved = drafts::save(&self.identity, &self.text);
        self.unsaved = saved.is_err();
        self.status = saved.map_or_else(|error| error, |_| "Draft saved on this device.".into());
        !self.unsaved
    }
    fn recolor(&mut self) {
        let mut full = self.full_source.clone();
        full.replace_range(self.bytes.clone(), &self.text);
        let start = full[..self.bytes.start].chars().count();
        let end = start + self.text.chars().count();
        let mut colors = Vec::with_capacity(self.text.chars().count());
        let lines = syntax::highlight(&self.identity.path, &full);
        let mut lines = lines.iter();
        let mut line = lines.next();
        let mut byte = 0;
        for (index, ch) in full.chars().enumerate() {
            if index >= end {
                break;
            }
            if index >= start {
                colors.push(
                    line.and_then(|line| line.spans.iter().find(|span| span.bytes.contains(&byte)))
                        .map_or(syntax::Kind::Plain, |span| span.kind),
                );
            }
            if ch == '\n' {
                line = lines.next();
                byte = 0;
            } else {
                byte += ch.len_utf8();
            }
        }
        self.colors = colors;
    }
    fn insert(&mut self, text: &str) {
        if let Err(error) = self.buffer.insert(text) {
            self.status = error;
        }
        self.dirty = true;
    }
    fn pointer(
        &mut self,
        position: Vec2,
        transform: &UiGlobalTransform,
        node: &ComputedNode,
        drag: bool,
    ) {
        let transform: bevy::math::Affine2 = transform.into();
        let local = position - transform.translation + node.size() / 2.;
        let logical = local * node.inverse_scale_factor();
        self.buffer.pointer(
            (logical.x - GUTTER) / CELL,
            (logical.y - TOP) / ROW + self.top as f32,
            drag,
        );
        self.dirty = true;
    }
    fn change_chunk(&mut self, range: std::ops::Range<usize>) -> Result<(), String> {
        self.sync();
        if range == self.identity.range {
            return Ok(());
        }
        if self.unsaved && !self.save() {
            return Err("This chunk could not be saved. Keep it open, copy it, or retry Cmd/Ctrl+S before navigating.".into());
        }
        let bytes = chunk_bytes(&self.full_source, range.clone())?;
        let original = self.full_source[bytes.clone()].to_owned();
        kernel::validate(&original)?;
        let mut identity = self.identity.clone();
        identity.range = range;
        let (text, blocked, status) = match drafts::load(&identity) {
            Ok(Some(text)) => (text, false, "Restored private chunk draft.".into()),
            Ok(None) => (
                original.clone(),
                false,
                "Editing source chunk. Changes autosave privately.".into(),
            ),
            Err(error) => (original.clone(), true, error),
        };
        let mut buffer = kernel::Buffer::new(&text)?;
        buffer.columns = self.buffer.columns;
        self.buffer = buffer;
        self.identity = identity;
        self.original = original;
        self.bytes = bytes.clone();
        self.text = text;
        self.persistence_blocked = blocked;
        self.status = status;
        self.top = 0;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(target) = self.apply.as_mut() {
            target.expected = self.full_source.clone();
            target.bytes = bytes;
        }
        self.changed = true;
        self.dirty = true;
        self.unsaved = false;
        self.restore_memory = true;
        self.recolor();
        Ok(())
    }
    fn find(
        &mut self,
        previous: bool,
        settings: reading::Settings,
        repo: &Repository,
        explorer: &Explorer,
    ) {
        let hits = listing::find::matching_lines(self.full_source.lines(), &self.find_text);
        if hits.is_empty() {
            self.status = "No matching lines in this file.".into();
            return;
        }
        let line = match self.find_hit {
            Some(at) if previous => hits
                .iter()
                .rev()
                .copied()
                .find(|&line| line < at)
                .unwrap_or(*hits.last().unwrap()),
            Some(at) => hits
                .iter()
                .copied()
                .find(|&line| line > at)
                .unwrap_or(hits[0]),
            None => hits
                .iter()
                .copied()
                .find(|&line| line >= self.identity.range.start)
                .unwrap_or(hits[0]),
        };
        let range = if repo
            .files
            .get(explorer.selected)
            .is_some_and(|file| file.path == self.identity.path)
            && explorer.original == self.full_source
        {
            listing::editor_range_at_line(explorer, line, settings)
        } else {
            settings.rows(line, self.full_source.lines().count())
        };
        match self.change_chunk(range) {
            Ok(()) => {
                self.find_hit = Some(line);
                let offset = self
                    .original
                    .split_inclusive('\n')
                    .take(line.saturating_sub(self.identity.range.start))
                    .map(str::len)
                    .sum::<usize>();
                let characters = self.original[..offset].chars().count();
                self.buffer.key(kernel::START, false);
                for _ in 0..characters {
                    self.buffer.key(kernel::RIGHT, false);
                }
                self.status = format!("Found line {}. Editing its source chunk.", line + 1);
            }
            Err(error) => self.status = error,
        }
    }
    pub(super) fn status_label(&self) -> String {
        format!(
            "Lines {}–{} · {}",
            self.identity.range.start + 1,
            self.identity.range.end.max(self.identity.range.start + 1),
            self.status
        )
    }
    fn ensure_cursor(&mut self, rows: &[std::ops::Range<usize>], visible: usize) {
        let cursor = self.buffer.positions().0;
        let row = cursor_row(rows, cursor);
        if row < self.top {
            self.top = row;
        }
        if row >= self.top + visible {
            self.top = row + 1 - visible;
        }
    }
    fn render(&mut self) -> Image {
        let width = self.size.x.max(160);
        let height = self.size.y.max(100);
        let mut canvas = code_texture::Canvas::new(width, height);
        canvas.rect(0, 0, width, height, [13, 23, 32]);
        let rows = self.buffer.rows();
        let visible = ((height as f32 - TOP * 2.) / ROW).floor().max(1.) as usize;
        self.top = self.top.min(rows.len().saturating_sub(visible));
        let (cursor, selected) = self.buffer.positions();
        let cursor_row = cursor_row(&rows, cursor);
        let data = self.buffer.chars();
        let mut number = 1
            + self.identity.range.start
            + data[..rows.get(self.top).map_or(0, |row| row.start)]
                .iter()
                .filter(|&&c| c == '\n' as u32)
                .count();
        for (offset, row) in rows.iter().enumerate().skip(self.top).take(visible) {
            let y = TOP + (offset - self.top) as f32 * ROW;
            let continuation = row.start > 0 && data[row.start - 1] != '\n' as u32;
            canvas.text(
                if continuation {
                    "·".into()
                } else {
                    number.to_string()
                }
                .as_str(),
                8.,
                y + FONT,
                14.,
                |_| [0.40, 0.56, 0.59],
            );
            let mut x = GUTTER;
            for index in row.clone() {
                let ch = char::from_u32(data[index]).expect("Unicode scalar buffer");
                let advance = match ch {
                    '\t' => CELL * 4.,
                    '\n' | '\r' => 0.,
                    _ => CELL,
                };
                if selected.contains(&index) {
                    canvas.rect(
                        x as u32,
                        y as u32,
                        advance.max(CELL) as u32,
                        ROW as u32,
                        [36, 76, 88],
                    );
                }
                if !matches!(ch, '\n' | '\r' | '\t') {
                    canvas.text(&ch.to_string(), x, y + FONT, FONT, |_| {
                        code_texture::ink(
                            self.colors
                                .get(index)
                                .copied()
                                .unwrap_or(syntax::Kind::Plain),
                        )
                    });
                }
                if self.focused && cursor_row == offset && cursor == index {
                    canvas.rect(x as u32, y as u32, 2, ROW as u32, [155, 242, 216]);
                }
                x += advance;
                if ch == '\n' {
                    number += 1;
                }
            }
            if self.focused && cursor_row == offset && cursor == row.end {
                canvas.rect(x as u32, y as u32, 2, ROW as u32, [155, 242, 216]);
            }
        }
        canvas.image()
    }
}
pub(super) fn chunk_bytes(
    source: &str,
    lines: std::ops::Range<usize>,
) -> Result<std::ops::Range<usize>, String> {
    let missing = || "The selected source chunk is unavailable in this snapshot.".to_owned();
    let count = lines.end.checked_sub(lines.start).ok_or_else(missing)?;
    let mut boundaries =
        std::iter::once(0).chain(source.split_inclusive('\n').scan(0, |end, line| {
            *end += line.len();
            Some(*end)
        }));
    let start = boundaries.nth(lines.start).ok_or_else(missing)?;
    let end = if count == 0 {
        start
    } else {
        boundaries.nth(count - 1).ok_or_else(missing)?
    };
    Ok(start..end)
}
fn cursor_row(rows: &[std::ops::Range<usize>], cursor: usize) -> usize {
    rows.iter()
        .rposition(|row| row.start <= cursor)
        .unwrap_or(0)
}
#[derive(Component)]
struct Status(Entity);
#[derive(Clone, Copy)]
enum Action {
    Copy,
    Paste,
    Undo,
    Redo,
    Discard,
    Apply,
    Find(bool),
}
#[derive(Resource, Default)]
struct Actions(Vec<(Entity, Action)>);
#[derive(Resource, Default)]
struct Paste(Option<(Entity, String, ClipboardRead)>);
#[derive(Resource, Default)]
struct Unsaved(std::collections::HashMap<String, String>);

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    mut identity: DraftIdentity,
    source: &str,
    apply: Option<ApplyTarget>,
) -> Entity {
    let create = || -> Result<View, String> {
        identity.validate()?;
        let count = source.split_inclusive('\n').count();
        identity.range.end = identity.range.end.min(count);
        let bytes = chunk_bytes(source, identity.range.clone())?;
        let original = source[bytes.clone()].to_owned();
        kernel::validate(&original)?;
        let restored = drafts::load(&identity);
        let persistence_blocked = restored.is_err();
        let (text, status) = match restored {
            Ok(Some(text)) => (
                text,
                "Restored private draft. Original snapshot is unchanged.".into(),
            ),
            Ok(None) => (
                original.clone(),
                "Edits autosave privately on this device. Original snapshot is unchanged.".into(),
            ),
            Err(error) => (original.clone(), error),
        };
        let mut view = View {
            identity,
            buffer: kernel::Buffer::new(&text)?,
            original,
            full_source: source.into(),
            bytes: bytes.clone(),
            text,
            status,
            persistence_blocked,
            apply: apply.map(|mut target| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    target.bytes = bytes;
                }
                target
            }),
            image: None,
            size: UVec2::ZERO,
            top: 0,
            dirty: true,
            changed: true,
            colors: Vec::new(),
            syntax_version: 0,
            focused: false,
            composing: false,
            find_input: None,
            find_text: String::new(),
            find_hit: None,
            unsaved: false,
            restore_memory: true,
        };
        view.recolor();
        Ok(view)
    };
    let mut view = match create() {
        Ok(view) => view,
        Err(error) => {
            return parent
                .spawn((
                    Text::new(error),
                    native_panel::panel_font(),
                    TextColor(native_panel::INK),
                ))
                .id();
        }
    };
    view.buffer.columns = 80;
    let entity = parent.commands().spawn_empty().id();
    let can_apply = view.apply.is_some();
    parent.spawn(Node { width:percent(100.),flex_direction:FlexDirection::Column,row_gap:px(8.),min_height:px(0.),flex_grow:1., ..default() }).with_children(|body| {
        body.spawn(Node{ column_gap:px(6.),flex_wrap:FlexWrap::Wrap,..default() }).with_children(|bar| {
            for (label,action) in [("Copy",Action::Copy),("Paste",Action::Paste),("Undo",Action::Undo),("Redo",Action::Redo),("Discard draft",Action::Discard)] {
                bar.spawn((file_picker::button(label,&native_panel::panel_font(),native_panel::INK,AlignSelf::Auto),TabIndex(45),observe(move |_:On<Activate>,mut actions:ResMut<Actions>|actions.0.push((entity,action)))));
            }
            if can_apply { bar.spawn((file_picker::button("Apply to file",&native_panel::panel_font(),native_panel::INK,AlignSelf::Auto),TabIndex(45),observe(move |_:On<Activate>,mut actions:ResMut<Actions>|actions.0.push((entity,Action::Apply))))); }
        });
        body.spawn(Node{width:percent(100.),column_gap:px(6.),align_items:AlignItems::Center,..default()}).with_children(|search| {
            search.spawn(Node{flex_grow:1.,min_width:px(80.),..default()}).with_children(|field|{view.find_input=Some(native_panel::input(field,"", "",46,false));});
            for (label,previous) in [("Previous match",true),("Next match",false)] {search.spawn((file_picker::button(label,&native_panel::panel_font(),native_panel::INK,AlignSelf::Auto),TabIndex(47),observe(move |_:On<Activate>,mut actions:ResMut<Actions>|actions.0.push((entity,Action::Find(previous))))));}
        });
        native_panel::text(body,"Cmd/Ctrl+F finds · F6 controls/editor · Tab puts away · Ctrl+] indents",Color::srgb_u8(120,158,164));
        body.spawn((Status(entity),Text::new(view.status_label()),native_panel::panel_font(),TextColor(native_panel::INK)));
        body.commands().entity(entity).insert((
            Name::new("Source editor"),view,ImageNode::default(),TabIndex(50),Outline::new(px(2.),px(2.),Color::NONE),
            Node{width:percent(100.),height:px(380.),min_height:px(180.),flex_grow:1.,flex_shrink:0.,..default()},
        )).observe(|mut event:On<bevy::input_focus::FocusedInput<KeyboardInput>>|event.propagate(false))
          .observe(|event:On<PointerPress>,mut focus:ResMut<InputFocus>,mut views:Query<(&mut View,&UiGlobalTransform,&ComputedNode)>| {
              if let Ok((mut view,transform,node))=views.get_mut(event.entity) { focus.set(event.entity,FocusCause::Pressed);view.pointer(event.pointer.position,transform,node,false); }
          }).observe(|event:On<PointerDrag>,mut views:Query<(&mut View,&UiGlobalTransform,&ComputedNode)>| {
              if let Ok((mut view,transform,node))=views.get_mut(event.entity) {view.pointer(event.pointer.position,transform,node,true);}
          });
        let root = body.target_entity();
        body.commands().entity(root).add_child(entity);
    });
    entity
}

fn modifier(key: KeyCode) -> Option<u8> {
    use keyboard::Modifiers as M;
    Some(match key {
        KeyCode::ControlLeft => M::CONTROL_LEFT,
        KeyCode::ControlRight => M::CONTROL_RIGHT,
        KeyCode::SuperLeft => M::SUPER_LEFT,
        KeyCode::SuperRight => M::SUPER_RIGHT,
        KeyCode::ShiftLeft => M::SHIFT_LEFT,
        KeyCode::ShiftRight => M::SHIFT_RIGHT,
        KeyCode::AltLeft => M::ALT_LEFT,
        KeyCode::AltRight => M::ALT_RIGHT,
        _ => return None,
    })
}
pub(super) fn register(app: &mut App) {
    app.init_resource::<Actions>()
        .init_resource::<Paste>()
        .init_resource::<Unsaved>();
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, update.after(review_tablet::update));
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, update.after(browser_tablet::update));
}
fn update(
    mut views: Query<(Entity, &ComputedNode, &mut ImageNode, &mut View)>,
    mut statuses: Query<(&Status, &mut Text)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    mut ime: MessageReader<bevy::window::Ime>,
    mut focus: ResMut<InputFocus>,
    mut images: ResMut<Assets<Image>>,
    mut clipboard: ResMut<Clipboard>,
    mut actions: ResMut<Actions>,
    mut paste: ResMut<Paste>,
    mut unsaved: ResMut<Unsaved>,
    wheel: Res<AccumulatedMouseScroll>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    (fields, settings, repo, explorer): (
        Query<&bevy::text::EditableText>,
        Res<reading::Settings>,
        Res<Repo>,
        Res<Explorer>,
    ),
) {
    for (_, _, _, mut view) in &mut views {
        if std::mem::take(&mut view.restore_memory)
            && let Some(text) = unsaved.0.get(&view.identity.key()).cloned()
        {
            if let Ok(mut buffer) = kernel::Buffer::new(&text) {
                buffer.columns = view.buffer.columns;
                view.buffer = buffer;
                view.text = text;
                view.unsaved = true;
                view.changed = true;
                view.dirty = true;
                view.recolor();
                view.save();
            }
        }
    }
    let raw: Vec<_> = events.read().cloned().collect();
    let final_modifiers = keyboard::Modifiers(
        [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]
        .into_iter()
        .fold(0, |held, key| {
            held | if keys.pressed(key) {
                modifier(key).unwrap_or(0)
            } else {
                0
            }
        }),
    );
    let changes: Vec<_> = raw
        .iter()
        .map(|event| keyboard::Input {
            modifier: modifier(event.key_code),
            pressed: event.state == ButtonState::Pressed,
            repeat: event.repeat,
        })
        .collect();
    let modifiers = keyboard::states(final_modifiers, &changes);
    let events: Vec<_> = raw
        .into_iter()
        .zip(modifiers)
        .filter(|(event, _)| event.state == ButtonState::Pressed)
        .collect();
    let ime: Vec<_> = ime.read().cloned().collect();
    for (entity, _, _, mut view) in &mut views {
        if let Some(field) = view.find_input.and_then(|entity| fields.get(entity).ok()) {
            let text = field.value().to_string();
            if view.find_text != text {
                view.find_text = text;
                view.find_hit = None;
            }
        }
        if focus.get() == view.find_input {
            for (event, modifiers) in &events {
                if matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
                    actions.0.push((entity, Action::Find(modifiers.shift())));
                }
            }
        }
    }
    if let Some(entity) = focus.get()
        && views.contains(entity)
    {
        if let Ok(mut window) = windows.single_mut() {
            window.ime_enabled = true;
        }
        if let Ok((_, _, _, mut view)) = views.get_mut(entity) {
            let before = view.buffer.positions();
            let mut committed = false;
            for event in &ime {
                match event {
                    bevy::window::Ime::Preedit { value, .. } => view.composing = !value.is_empty(),
                    bevy::window::Ime::Commit { value, .. } => {
                        view.composing = false;
                        view.insert(value);
                        committed = true;
                    }
                    _ => {}
                }
            }
            for (event, modifiers) in &events {
                let shift = modifiers.shift();
                let command = modifiers.command();
                let alt = modifiers.alt();
                if command {
                    match event.key_code {
                        KeyCode::KeyF => {
                            if let Some(field) = view.find_input {
                                focus.set(field, FocusCause::Navigated);
                            }
                        }
                        KeyCode::KeyA => view.buffer.select_all(),
                        KeyCode::KeyZ => view
                            .buffer
                            .key(if shift { kernel::REDO } else { kernel::UNDO }, false),
                        KeyCode::KeyY => view.buffer.key(kernel::REDO, false),
                        KeyCode::KeyS => {
                            view.save();
                        }
                        KeyCode::KeyC | KeyCode::KeyX => {
                            let selected = view.buffer.selection();
                            if !selected.is_empty() {
                                match clipboard.set_text(selected) {
                                    Ok(()) => {
                                        if event.key_code == KeyCode::KeyX {
                                            view.buffer.key(kernel::DELETE, false);
                                        }
                                    }
                                    Err(error) => view.status = error.to_string(),
                                }
                            }
                        }
                        KeyCode::KeyV => {
                            paste.0 = Some((entity, view.identity.key(), clipboard.fetch_text()))
                        }
                        KeyCode::BracketRight => view.insert("    "),
                        KeyCode::ArrowLeft => view.buffer.key(kernel::WORD_LEFT, shift),
                        KeyCode::ArrowRight => view.buffer.key(kernel::WORD_RIGHT, shift),
                        KeyCode::Home | KeyCode::ArrowUp => view.buffer.key(kernel::START, shift),
                        KeyCode::End | KeyCode::ArrowDown => view.buffer.key(kernel::FINISH, shift),
                        _ => {}
                    }
                } else {
                    let key = match event.key_code {
                        KeyCode::ArrowLeft => {
                            Some(if alt { kernel::WORD_LEFT } else { kernel::LEFT })
                        }
                        KeyCode::ArrowRight => Some(if alt {
                            kernel::WORD_RIGHT
                        } else {
                            kernel::RIGHT
                        }),
                        KeyCode::ArrowUp => Some(kernel::UP),
                        KeyCode::ArrowDown => Some(kernel::DOWN),
                        KeyCode::Home => Some(kernel::HOME),
                        KeyCode::End => Some(kernel::END),
                        KeyCode::PageUp => Some(kernel::PAGE_UP),
                        KeyCode::PageDown => Some(kernel::PAGE_DOWN),
                        KeyCode::Backspace => Some(kernel::BACKSPACE),
                        KeyCode::Delete => Some(kernel::DELETE),
                        _ => None,
                    };
                    if let Some(key) = key {
                        view.buffer.key(key, shift);
                    } else if event.key_code == KeyCode::Enter && !committed && !view.composing {
                        view.insert("\n");
                    } else if !committed
                        && !view.composing
                        && !alt
                        && let Some(text) = &event.text
                    {
                        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
                        if !text.is_empty() {
                            view.insert(&text);
                        }
                    }
                }
            }
            view.sync();
            if before != view.buffer.positions() || !events.is_empty() || committed {
                let rows = view.buffer.rows();
                let visible = ((view.size.y as f32 - TOP * 2.) / ROW).max(1.) as usize;
                view.ensure_cursor(&rows, visible);
                view.dirty = true;
            }
            if wheel.delta.y != 0. {
                let delta = if wheel.unit == bevy::input::mouse::MouseScrollUnit::Line {
                    wheel.delta.y * 3.
                } else {
                    wheel.delta.y / ROW
                };
                view.top = (view.top as f32 - delta).max(0.) as usize;
                view.dirty = true;
            }
        }
    }
    for (entity, action) in std::mem::take(&mut actions.0) {
        if let Ok((_, _, _, mut view)) = views.get_mut(entity) {
            match action {
                Action::Copy => {
                    view.status = clipboard
                        .set_text(view.text.clone())
                        .map_or_else(|e| e.to_string(), |_| "Copied edited source.".into())
                }
                Action::Paste => {
                    paste.0 = Some((entity, view.identity.key(), clipboard.fetch_text()))
                }
                Action::Undo => view.buffer.key(kernel::UNDO, false),
                Action::Redo => view.buffer.key(kernel::REDO, false),
                Action::Discard => match drafts::remove(&view.identity) {
                    Ok(()) => {
                        if let Ok(mut buffer) = kernel::Buffer::new(&view.original) {
                            buffer.columns = view.buffer.columns;
                            view.buffer = buffer;
                            view.top = 0;
                            view.persistence_blocked = false;
                            view.unsaved = false;
                            view.text = view.original.clone();
                            view.changed = true;
                            view.recolor();
                            view.status =
                                "Discarded private draft. Original snapshot restored.".into();
                        }
                    }
                    Err(error) => view.status = error,
                },
                Action::Find(previous) => {
                    let previous_key = view.identity.key();
                    view.find(previous, *settings, &repo.0, &explorer);
                    if view.identity.key() != previous_key {
                        unsaved.0.remove(&previous_key);
                    }
                    focus.set(entity, FocusCause::Navigated);
                }
                Action::Apply => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let text = view.text.clone();
                        if let Some(target) = view.apply.as_mut() {
                            view.status = target.apply(&text).map_or_else(
                                |error| error,
                                |_| "Applied to worktree. Nothing committed or pushed.".into(),
                            );
                        }
                    }
                }
            }
            view.sync();
            view.dirty = true;
        }
    }
    if let Some((entity, identity, request)) = paste.0.as_mut()
        && let Some(result) = request.poll_result()
    {
        if let Ok((_, _, _, mut view)) = views.get_mut(*entity)
            && &view.identity.key() == identity
        {
            match result {
                Ok(text) => {
                    view.insert(&text);
                    view.sync();
                }
                Err(error) => view.status = error.to_string(),
            }
        }
        paste.0 = None;
    }
    for (entity, node, mut image, mut view) in &mut views {
        // A chunk switch may still be waiting to recover this key's in-memory draft.
        if !view.restore_memory {
            if view.unsaved {
                unsaved.0.insert(view.identity.key(), view.text.clone());
            } else {
                unsaved.0.remove(&view.identity.key());
            }
        }
        let size = (node.size() * node.inverse_scale_factor())
            .clamp(Vec2::new(160., 100.), Vec2::new(2400., 1400.))
            .as_uvec2();
        if view.size != size {
            view.size = size;
            view.buffer.columns = ((size.x as f32 - GUTTER - 12.) / CELL).max(1.) as usize;
            view.dirty = true;
        }
        let focused = focus.get() == Some(entity);
        if view.focused != focused {
            view.focused = focused;
            view.dirty = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let version = repo_world::browser_syntax_version();
            if version != view.syntax_version {
                view.syntax_version = version;
                view.recolor();
                view.dirty = true;
            }
        }
        if view.dirty {
            let frame = view.render();
            if let Some(handle) = &view.image {
                let _ = images.insert(handle.id(), frame);
            } else {
                view.image = Some(images.add(frame));
            }
            if let Some(handle) = &view.image {
                image.image = handle.clone();
            }
            view.dirty = false;
        }
    }
    for (owner, mut label) in &mut statuses {
        if let Ok((_, _, _, view)) = views.get(owner.0) {
            label.set_if_neq(Text::new(view.status_label()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_editor_renders_only_its_chunk_with_shared_syntax_and_unicode_fonts() {
        let source = "// prefix\nfn chosen() {\n    let α = \"∀ x, x ∈ ℕ\";\n}\n// unseen tail\n";
        let identity = DraftIdentity {
            repository: "native-render-fixture".into(),
            revision: "immutable".into(),
            baseline: None,
            path: "example.rs".into(),
            blob: format!("native-pixels-{}", std::process::id()),
            before: false,
            range: 1..4,
        };
        let mut world = World::new();
        let root = world.spawn(Node::default()).id();
        world.commands().entity(root).with_children(|parent| {
            spawn(parent, identity, source, None);
        });
        world.flush();
        let mut query = world.query::<&mut View>();
        let mut editor = query.single_mut(&mut world).unwrap();
        assert!(!editor.value().contains("prefix") && !editor.value().contains("unseen tail"));
        assert!(editor.colors.contains(&syntax::Kind::Keyword));
        assert!(editor.colors.contains(&syntax::Kind::String));
        editor.size = UVec2::new(1100, 420);
        editor.buffer.columns = 95;
        editor.focused = true;
        let frame = editor.render();
        let pixels = frame.data.unwrap();
        let keywords = pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 120 && p[2] > 130 && p[2] as f32 > p[1] as f32 * 1.1)
            .count();
        let strings = pixels
            .chunks_exact(4)
            .filter(|p| p[1] > 120 && p[1] > p[0] && p[1] > p[2])
            .count();
        assert!(
            keywords > 30,
            "native raster must contain purple keyword glyphs"
        );
        assert!(
            strings > 30,
            "native raster must contain green string glyphs"
        );
        if let Some(path) = std::env::var_os("REPO_CITY_EDITOR_CAPTURE") {
            let image = image::RgbaImage::from_raw(1100, 420, pixels).unwrap();
            image.save(path).unwrap();
        }
    }

    #[test]
    fn chunk_replacement_preserves_unicode_prefix_suffix_and_source_line_coordinates() {
        let source = "prefix α\nfn first() {\n  work();\n}\nTAIL\n";
        let bytes = chunk_bytes(source, 1..4).unwrap();
        let mut edited = source.to_owned();
        edited.replace_range(bytes, "fn first() {}\n");
        assert_eq!(edited, "prefix α\nfn first() {}\nTAIL\n");
        assert!(chunk_bytes(source, 4..20).is_err());
    }
}
