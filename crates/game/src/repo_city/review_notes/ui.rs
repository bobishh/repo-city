use std::collections::HashMap;

use bevy::{
    clipboard::Clipboard,
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    text::EditableText,
};

use super::{native, *};
use crate::native_panel::{self, Actions, INK};

#[derive(Clone)]
enum View {
    Palette(usize),
    EditPaint(usize),
    Notes { editing: bool },
}

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Close,
    Select(usize),
    Equip(usize),
    Edit(usize),
    Save(usize),
    EditNotes,
    EditNote(usize),
    AllComments,
    OpenTablet(review_tablet::Page),
    Remove(usize),
    CopyNote(usize),
    MarkViewed(usize),
    Visit(usize),
    OpenDiscussion(usize),
    Publish,
    AutoPublish,
    SignIn,
    CopyCode,
}

#[derive(Resource, Default)]
pub(crate) struct ReviewUi {
    view: Option<View>,
    focused_note: Option<String>,
    panel: Option<Entity>,
    fields: Vec<Entity>,
    action: Option<Action>,
    publication: Option<(String, native_github::Job<Vec<Result<Note, String>>>)>,
    draft_fields: Vec<(usize, Entity)>,
    primary: Option<Entity>,
    auto_revision: Option<(String, Option<String>)>,
    message: String,
    auth: Option<repository_picker::AuthPrompt>,
    scroll: f32,
    empty_until: f32,
    status_scope: String,
    status_job: Option<native_github::Job<HashMap<String, native::LocalState>>>,
    status_due: f32,
}

impl Actions<Action> for ReviewUi {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl ReviewUi {
    pub(crate) fn smoke_edit_paint(&mut self) {
        self.action = Some(Action::Edit(1));
    }

    fn refresh_local_states(&mut self, review: &mut Review, now: f32) -> bool {
        if review.scope != self.status_scope {
            self.status_scope.clone_from(&review.scope);
            review.local_states.clear();
            self.status_job = None;
            self.status_due = 0.;
        }
        let mut changed = false;
        if let Some(result) = self.status_job.as_ref().and_then(native_github::Job::take) {
            self.status_job = None;
            if let Ok(states) = result
                && states != review.local_states
            {
                review.local_states = states;
                changed = true;
            }
        }
        if self.status_job.is_none() && now >= self.status_due {
            let scope = review.scope.clone();
            self.status_job = Some(native_github::Job::spawn(move || native::states(&scope)));
            self.status_due = now + 1.;
        }
        changed
    }

    fn select_view(&mut self, panel: Panel, review: &Review, repo: &Repository, github: bool) {
        self.focused_note = match panel {
            Panel::Aimed => review
                .aim_note
                .as_ref()
                .filter(|id| {
                    review
                        .notes
                        .iter()
                        .any(|note| &note.id == *id && review.shows_paint(note, repo))
                })
                .cloned(),
            _ => None,
        };
        self.view = match panel {
            Panel::Paints => Some(View::Palette(review.slot)),
            Panel::Notes | Panel::Aimed
                if github
                    || review
                        .notes
                        .iter()
                        .any(|note| Review::on_snapshot(note, repo)) =>
            {
                Some(View::Notes { editing: false })
            }
            _ => None,
        };
    }

    fn includes(&self, note: &Note, repo: &Repository) -> bool {
        self.focused_note.as_ref().is_none_or(|id| id == &note.id)
            && (note.remote || Review::on_snapshot(note, repo))
    }
}

fn button(parent: &mut ChildSpawnerCommands, label: &str, ink: Color, action: Action) {
    native_panel::button::<_, ReviewUi>(parent, label, ink, action);
}

fn tab_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    ink: Color,
    action: Action,
    index: i32,
) -> Entity {
    let entity = native_panel::button::<_, ReviewUi>(parent, label, ink, action);
    parent.commands().entity(entity).insert(TabIndex(index));
    entity
}

fn is_draft(note: &Note, repo: &Repository) -> bool {
    !note.remote && note.published.is_none() && Review::on_snapshot(note, repo)
}

#[derive(Component)]
pub(crate) struct NoteRows;

fn paint_color(preset: &Preset) -> Color {
    let [r, g, b] = preset.color;
    Color::srgb_u8(r, g, b)
}

fn empty_preset() -> Preset {
    Preset {
        id: new_id(),
        label: "Note".into(),
        prompt: String::new(),
        spray: "Note".into(),
        color: [255, 126, 141],
    }
}

fn draw(
    commands: &mut Commands,
    ui: &mut ReviewUi,
    review: &Review,
    repo: &Repository,
    picker: Option<&repository_picker::RepositoryPicker>,
    comments: Option<&native_github::PullComments>,
) {
    ui.fields.clear();
    ui.draft_fields.clear();
    ui.primary = None;
    let Some(view) = &ui.view else {
        return;
    };
    let title = match view {
        View::Palette(_) => "Paints",
        View::EditPaint(_) => "Edit paint",
        View::Notes { .. } if ui.focused_note.is_some() => "Review comment",
        View::Notes { .. } => "Review comments",
    };
    let notes_view = matches!(view, View::Notes { .. });
    let notes = if notes_view {
        review
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| ui.includes(note, repo))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let panel = if notes_view {
        native_panel::tablet(commands, title)
    } else {
        native_panel::panel(commands, title, 490.)
    };
    ui.panel = Some(panel);
    commands.entity(panel).with_children(|parent| {
        if !notes_view {
            parent
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    right: px(8.),
                    top: px(8.),
                    ..default()
                })
                .with_children(|corner| button(corner, "×", INK, Action::Close));
        }
        if notes_view {
            parent
                .spawn(Node {
                    width: percent(100.),
                    column_gap: px(6.),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|bar| {
                    for (index, (page, label)) in
                        review_tablet::Page::NAVIGATION.into_iter().enumerate()
                    {
                        tab_button(
                            bar,
                            label,
                            INK,
                            Action::OpenTablet(page),
                            -10 + index as i32,
                        );
                    }
                    bar.spawn(Node {
                        flex_grow: 1.,
                        ..default()
                    });
                    tab_button(
                        bar,
                        "⚙",
                        INK,
                        Action::OpenTablet(review_tablet::Page::Settings),
                        -1,
                    );
                });
        }
        match view {
            View::Palette(selected) => {
                parent
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(6.),
                        row_gap: px(6.),
                        ..default()
                    })
                    .with_children(|slots| {
                        for slot in 0_usize..=9 {
                            let preset = slot
                                .checked_sub(1_usize)
                                .and_then(|i| review.palette[i].as_ref());
                            let name = if slot == 0 {
                                "Water"
                            } else {
                                preset.map_or("Note", |paint| paint.label.as_str())
                            };
                            let mark = if slot == *selected { "› " } else { "" };
                            let label = format!("{mark}{slot} · {name}");
                            button(
                                slots,
                                &label,
                                preset.map_or(INK, paint_color),
                                Action::Select(slot),
                            );
                        }
                    });
                let paint = selected
                    .checked_sub(1)
                    .and_then(|i| review.palette[i].as_ref());
                let prompt = paint.map_or(
                    if *selected == 0 {
                        "Reveal code."
                    } else {
                        "No preset text."
                    },
                    |paint| paint.prompt.as_str(),
                );
                native_panel::text(parent, prompt, Color::srgb_u8(221, 235, 240));
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        ..default()
                    })
                    .with_children(|actions| {
                        ui.primary = Some(tab_button(
                            actions,
                            "Equip",
                            INK,
                            Action::Equip(*selected),
                            20,
                        ));
                        if *selected != 0 {
                            tab_button(actions, "Edit", INK, Action::Edit(*selected), 21);
                        }
                    });
            }
            View::EditPaint(slot) => {
                let paint = review.palette[*slot - 1]
                    .clone()
                    .unwrap_or_else(empty_preset);
                let [r, g, b] = paint.color;
                ui.fields = vec![
                    native_panel::input(parent, "Name", &paint.label, 0, false),
                    native_panel::input(parent, "Spray text", &paint.spray, 1, false),
                    native_panel::input(parent, "Reusable comment", &paint.prompt, 2, true),
                    native_panel::input(
                        parent,
                        "HEX",
                        &format!("#{r:02x}{g:02x}{b:02x}"),
                        3,
                        false,
                    ),
                ];
                color_picker::spawn(parent, ui.fields[3], paint.color);
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        ..default()
                    })
                    .with_children(|actions| {
                        tab_button(actions, "Save", INK, Action::Save(*slot), 4);
                        tab_button(actions, "Back", INK, Action::Select(*slot), 5);
                    });
            }
            View::Notes { editing } => {
                let editing = *editing;
                let all = ui.focused_note.is_none();
                let can_publish = all && picker.is_some_and(|picker| picker.can_publish());
                let draft_count = notes
                    .iter()
                    .filter(|(_, note)| is_draft(note, repo))
                    .count();
                parent
                    .spawn((
                        NoteRows,
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8.),
                            flex_grow: 1.,
                            min_height: px(0.),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition(Vec2::new(0., ui.scroll)),
                    ))
                    .with_children(|list| {
                        if all && let Some(comments) = comments {
                            let status = match comments {
                                native_github::PullComments::Loading => "Loading GitHub comments…",
                                native_github::PullComments::Loaded(Err(error)) => error,
                                native_github::PullComments::Loaded(Ok(notes))
                                    if notes.is_empty() =>
                                {
                                    "No GitHub review comments."
                                }
                                _ => "",
                            };
                            if !status.is_empty() {
                                native_panel::text(list, status, INK);
                            }
                        }
                        for (index, note) in notes {
                            list.spawn((
                                BackgroundColor(Color::srgb_u8(14, 35, 42)),
                                Node {
                                    padding: UiRect::all(px(10.)),
                                    row_gap: px(5.),
                                    flex_direction: FlexDirection::Column,
                                    flex_shrink: 0.,
                                    ..default()
                                },
                            ))
                            .with_children(|card| {
                                let anchor = &note.anchor;
                                let location = match (anchor.start, anchor.end) {
                                    (0, _) => anchor.path.clone(),
                                    (start, end) if start == end => {
                                        format!("{}:{start}", anchor.path)
                                    }
                                    (start, end) => format!("{}:{start}–{end}", anchor.path),
                                };
                                native_panel::text(card, &location, INK);
                                let state = if note.remote && !Review::on_snapshot(note, repo) {
                                    "Original discussion"
                                } else if note.published.is_some() || note.remote {
                                    "Published"
                                } else if repo.is_worktree() {
                                    "Saved locally"
                                } else {
                                    "Draft"
                                };
                                let label = review
                                    .local_states
                                    .get(&note.id)
                                    .filter(|state| **state != native::LocalState::Unread)
                                    .map_or_else(
                                        || format!("{} · {state}", note.preset.label),
                                        |local| {
                                            format!(
                                                "{} · {state} · {}",
                                                note.preset.label,
                                                local.label()
                                            )
                                        },
                                    );
                                native_panel::text(card, &label, paint_color(&note.preset));
                                if editing && is_draft(note, repo) {
                                    let field = native_panel::input(
                                        card,
                                        "",
                                        note.comment(),
                                        ui.draft_fields.len() as i32,
                                        true,
                                    );
                                    ui.draft_fields.push((index, field));
                                } else {
                                    markdown_preview::draw(
                                        card,
                                        markdown_preview::markdown_html(note.comment()),
                                    );
                                }
                                let can_visit = note.anchor.start > 0
                                    && Review::on_snapshot(note, repo)
                                    && repo.files.iter().any(|file| file.path == note.anchor.path);
                                if !editing {
                                    card.spawn(Node {
                                        column_gap: px(6.),
                                        ..default()
                                    })
                                    .with_children(|tools| {
                                        if is_draft(note, repo) {
                                            let edit = tab_button(
                                                tools,
                                                "Edit",
                                                INK,
                                                Action::EditNote(index),
                                                index as i32 * 4,
                                            );
                                            ui.primary = ui.primary.or(Some(edit));
                                        }
                                        tab_button(
                                            tools,
                                            "Copy",
                                            INK,
                                            Action::CopyNote(index),
                                            index as i32 * 4 + 1,
                                        );
                                        if repo.is_worktree()
                                            && !note.remote
                                            && note.published.is_none()
                                            && review.local_states.get(&note.id).is_none_or(
                                                |state| *state == native::LocalState::Unread,
                                            )
                                        {
                                            tab_button(
                                                tools,
                                                "Mark as viewed",
                                                INK,
                                                Action::MarkViewed(index),
                                                index as i32 * 4 + 2,
                                            );
                                        }
                                        tools.spawn(Node {
                                            flex_grow: 1.,
                                            ..default()
                                        });
                                        if can_visit {
                                            let visit = tab_button(
                                                tools,
                                                "Fly to lines",
                                                INK,
                                                Action::Visit(index),
                                                index as i32 * 4 + 3,
                                            );
                                            ui.primary = ui.primary.or(Some(visit));
                                        }
                                        if note.published.is_some() {
                                            let discussion = tab_button(
                                                tools,
                                                "Open on GitHub",
                                                INK,
                                                Action::OpenDiscussion(index),
                                                1000 + index as i32,
                                            );
                                            ui.primary = ui.primary.or(Some(discussion));
                                        }
                                    });
                                } else if editing && !note.remote {
                                    card.spawn(Node {
                                        column_gap: px(6.),
                                        ..default()
                                    })
                                    .with_children(|tools| {
                                        tab_button(
                                            tools,
                                            "Save",
                                            INK,
                                            Action::EditNotes,
                                            10_100 + index as i32 * 2,
                                        );
                                        if is_draft(note, repo) {
                                            tab_button(
                                                tools,
                                                "Delete",
                                                Color::srgb_u8(244, 166, 177),
                                                Action::Remove(index),
                                                10_101 + index as i32 * 2,
                                            );
                                        }
                                    });
                                }
                            });
                        }
                    });
                let busy = ui.publication.is_some();
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        flex_wrap: FlexWrap::Wrap,
                        row_gap: px(6.),
                        ..default()
                    })
                    .with_children(|actions| {
                        if !all && !editing {
                            tab_button(actions, "All comments", INK, Action::AllComments, 10_003);
                        }
                        if can_publish && draft_count != 0 && !busy {
                            let entity = tab_button(
                                actions,
                                &format!("Publish all ({draft_count})"),
                                Color::srgb_u8(197, 255, 231),
                                Action::Publish,
                                10_000,
                            );
                            ui.primary = Some(entity);
                            actions
                                .commands()
                                .entity(entity)
                                .insert(BackgroundColor(Color::srgb_u8(27, 71, 67)));
                        }
                        if draft_count != 0 && !busy && editing && ui.focused_note.is_none() {
                            let edit = tab_button(
                                actions,
                                if editing { "Save edits" } else { "Edit" },
                                INK,
                                Action::EditNotes,
                                10_001,
                            );
                            ui.primary = ui.primary.or(Some(edit));
                        }
                        if can_publish && picker.and_then(|picker| picker.publication()).is_some() {
                            tab_button(
                                actions,
                                if review.auto_publish {
                                    "Auto-publish on"
                                } else {
                                    "Auto-publish off"
                                },
                                INK,
                                Action::AutoPublish,
                                10_002,
                            );
                        }
                    });
                if can_publish
                    && draft_count != 0
                    && let Some(picker) = picker
                    && picker.publication().is_none()
                {
                    use repository_picker::AuthPrompt;
                    match picker.auth_prompt() {
                        AuthPrompt::SignIn => {
                            tab_button(parent, "Sign in with GitHub", INK, Action::SignIn, 10_003);
                        }
                        AuthPrompt::Copy(caption) => {
                            tab_button(parent, &caption, INK, Action::CopyCode, 10_003);
                        }
                        AuthPrompt::Message(message) => native_panel::text(parent, &message, INK),
                    }
                    if !picker.auth_message().is_empty() {
                        native_panel::text(parent, picker.auth_message(), INK);
                    }
                }
            }
        }
        parent.spawn((
            Feedback,
            Text::new(ui.message.clone()),
            TextFont::from_font_size(13.),
            TextColor(INK),
            Node {
                min_height: px(18.),
                ..default()
            },
        ));
    });
}

#[derive(Component)]
pub(crate) struct Feedback;

fn save_paint(review: &mut Review, slot: usize, values: &[String]) -> Result<(), String> {
    let [name, spray, prompt, color] = values else {
        return Err("The paint editor is still opening.".into());
    };
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the paint a name.".into());
    }
    let rgb = color_picker::parse_hex(color).ok_or("Enter a color as #RRGGBB.")?;
    let mut palette = review.palette.clone();
    let mut paint = palette[slot - 1].take().unwrap_or_else(empty_preset);
    paint.label = name.into();
    paint.spray = spray.clone();
    paint.prompt = prompt.clone();
    paint.color = rgb;
    palette[slot - 1] = Some(paint);
    native::save(&review.scope, "palette", &palette)?;
    review.palette = palette;
    review.select(slot);
    Ok(())
}

fn save_edits(
    review: &mut Review,
    ui: &ReviewUi,
    fields: &Query<&EditableText>,
) -> Result<(), String> {
    let updates = ui
        .draft_fields
        .iter()
        .filter_map(|&(index, entity)| {
            let note = review.notes.get(index)?;
            let value = fields.get(entity).ok()?.value().to_string();
            if value == note.comment() {
                return None;
            }
            let mut updated = note.clone();
            updated.text = value;
            Some((index, updated))
        })
        .collect::<Vec<_>>();
    if !updates.is_empty() {
        native::save_notes(
            &review.scope,
            &updates
                .iter()
                .map(|(_, note)| note.clone())
                .collect::<Vec<_>>(),
        )?;
        for (index, note) in updates {
            review.notes[index] = note;
        }
    }
    Ok(())
}

pub(crate) fn update(
    mut commands: Commands,
    mut review: ResMut<Review>,
    mut ui: ResMut<ReviewUi>,
    _explorer: Res<Explorer>,
    mut camera: Query<&mut Pilot, PlayerCamera>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    parents: Query<&ChildOf>,
    fields: Query<&EditableText>,
    mut hint: Query<&mut Visibility, With<EmptyNotesHint>>,
    mut feedback: Query<&mut Text, With<Feedback>>,
    repo: Option<Res<Repo>>,
    mut picker: Option<ResMut<repository_picker::RepositoryPicker>>,
    (mut tablet, mut clipboard, wheel, mut lists, comments): (
        Option<ResMut<review_tablet::Tablet>>,
        Option<ResMut<Clipboard>>,
        Option<Res<AccumulatedMouseScroll>>,
        Query<&mut ScrollPosition, With<NoteRows>>,
        Option<Res<native_github::PullComments>>,
    ),
) {
    let fallback = repository_picker::empty();
    let repo = repo.as_ref().map_or(&fallback, |repo| &repo.0);
    let mut redraw = false;
    let mut handoff = false;
    if comments
        .as_ref()
        .is_some_and(|comments| comments.is_changed())
        && matches!(ui.view, Some(View::Notes { editing: false }))
    {
        redraw = true;
    }
    let mut message = String::new();
    if let Ok(mut position) = lists.single_mut() {
        if let Some(wheel) = wheel.as_ref() {
            let delta = match wheel.unit {
                bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 34.,
                bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
            };
            if delta != 0. {
                position.y = (position.y - delta).max(0.);
            }
        }
        ui.scroll = position.y;
    }
    if let Some(result) = ui.publication.as_ref().and_then(|(_, job)| job.take()) {
        let (scope, _) = ui.publication.take().expect("pending publication");
        let mut published = 0;
        let mut errors = Vec::new();
        match result {
            Ok(results) => {
                for result in results {
                    match result.and_then(|note| {
                        native::save(&scope, &note.id, &note)?;
                        Ok(note)
                    }) {
                        Ok(note) => {
                            if scope == review.scope
                                && let Some(saved) =
                                    review.notes.iter_mut().find(|saved| saved.id == note.id)
                            {
                                *saved = note;
                            }
                            published += 1;
                        }
                        Err(error) => errors.push(error),
                    }
                }
            }
            Err(error) => errors.push(error),
        }
        if let Some(error) = errors.first() {
            review.auto_publish = false;
            review.auto_drafts.clear();
            if errors.iter().any(|error| error.starts_with("GitHub 401:"))
                && let Some(picker) = picker.as_deref_mut()
            {
                picker.invalidate_session();
            }
            message = if published == 0 {
                format!("Drafts kept. {error}")
            } else {
                format!(
                    "Published {published}. {} kept as drafts. {error}",
                    errors.len()
                )
            };
        } else {
            message = format!("Published {published} on GitHub.");
        }
        redraw = ui.panel.is_some();
    }
    let auth = picker.as_ref().map(|picker| picker.auth_prompt());
    if ui.auth != auth {
        ui.auth = auth;
        if matches!(ui.view, Some(View::Notes { editing: false })) {
            redraw = true;
        }
    }
    let was_open = ui.view.is_some();
    if let Some(panel) = review.request.take() {
        ui.select_view(panel, &review, repo, comments.is_some());
        if ui.view.is_none() {
            ui.empty_until = time.elapsed_secs() + 3.5;
        } else {
            ui.empty_until = 0.;
            ui.message.clear();
            ui.scroll = 0.;
            redraw = true;
            review.drag = None;
            if let Ok(mut pilot) = camera.single_mut() {
                pilot.captured = false;
                pilot.firing = false;
                pilot.shot = false;
                pilot.fire_ready = false;
            }
            if let Ok(mut cursor) = cursor.single_mut() {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
        }
    }
    if let Ok(mut hint) = hint.single_mut() {
        hint.set_if_neq(if time.elapsed_secs() < ui.empty_until {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }

    let action = ui.action.take().or_else(|| {
        let dismiss = keys.just_pressed(KeyCode::Escape)
            || (matches!(ui.view, Some(View::Notes { .. })) && review_tablet::plain_tab(&keys));
        (was_open && dismiss).then_some(Action::Close)
    });
    if let Some(action) = action {
        match action {
            Action::OpenTablet(page) => match save_edits(&mut review, &ui, &fields) {
                Ok(()) => {
                    if let Some(tablet) = tablet.as_deref_mut() {
                        tablet.open_page(page);
                        ui.view = None;
                        handoff = true;
                        redraw = true;
                    }
                }
                Err(error) => message = error,
            },
            Action::AllComments => {
                ui.select_view(Panel::Notes, &review, repo, comments.is_some());
                ui.scroll = 0.;
                redraw = true;
            }
            Action::MarkViewed(index) => {
                if let Some(note) = review
                    .notes
                    .get(index)
                    .filter(|note| repo.is_worktree() && !note.remote && note.published.is_none())
                {
                    let id = note.id.clone();
                    match native::mark_worktree_note(&review.scope, &id, native::LocalState::Read) {
                        Ok(()) => {
                            review.local_states.insert(id, native::LocalState::Read);
                            ui.status_job = None;
                            ui.status_due = 0.;
                            message = "Viewed locally.".into();
                        }
                        Err(error) => message = error,
                    }
                    redraw = true;
                }
            }
            Action::EditNote(index) => {
                if let Some(note) = review.notes.get(index).filter(|note| is_draft(note, repo)) {
                    ui.focused_note = Some(note.id.clone());
                    ui.view = Some(View::Notes { editing: true });
                    ui.message.clear();
                    redraw = true;
                }
            }
            Action::Close => match save_edits(&mut review, &ui, &fields) {
                Ok(()) => {
                    ui.view = None;
                    redraw = true;
                }
                Err(error) => message = error,
            },
            Action::Visit(index) => {
                if let Some(note) = review.notes.get(index)
                    && Review::on_snapshot(note, repo)
                {
                    review.destination = Some(Destination {
                        anchor: note.anchor.clone(),
                        revision: note.revision.clone(),
                    });
                    ui.view = None;
                    redraw = true;
                }
            }
            Action::OpenDiscussion(index) => {
                if let Some(url) = review
                    .notes
                    .get(index)
                    .and_then(|note| note.published.as_deref())
                    && let Err(error) = native_github::open_discussion(url)
                {
                    message = error;
                }
            }
            Action::Select(slot) => {
                ui.view = Some(View::Palette(slot));
                redraw = true;
            }
            Action::Equip(slot) => {
                let mut palette = review.palette.clone();
                if let Some(paint) = slot.checked_sub(1).and_then(|index| palette.get_mut(index))
                    && paint.is_none()
                {
                    *paint = Some(empty_preset());
                }
                let saved = if palette != review.palette {
                    native::save(&review.scope, "palette", &palette)
                } else {
                    Ok(())
                };
                match saved {
                    Ok(()) => {
                        review.palette = palette;
                        review.select(slot);
                        ui.view = None;
                        redraw = true;
                    }
                    Err(error) => message = error,
                }
            }
            Action::Edit(slot) => {
                ui.view = Some(View::EditPaint(slot));
                redraw = true;
            }
            Action::Save(slot) => {
                let values = ui
                    .fields
                    .iter()
                    .filter_map(|&entity| fields.get(entity).ok())
                    .map(|field| field.value().to_string())
                    .collect::<Vec<_>>();
                match save_paint(&mut review, slot, &values) {
                    Ok(()) => {
                        ui.view = Some(View::Palette(slot));
                        redraw = true;
                    }
                    Err(error) => message = error,
                }
            }
            Action::EditNotes
            | Action::Remove(_)
            | Action::CopyNote(_)
            | Action::Publish
            | Action::AutoPublish => {
                if let Some(View::Notes { editing }) = ui.view
                    && ui.publication.is_none()
                {
                    match save_edits(&mut review, &ui, &fields) {
                        Err(error) => message = error,
                        Ok(()) => match action {
                            Action::EditNotes => {
                                ui.view = Some(View::Notes { editing: !editing });
                                if editing {
                                    message = if repo.is_worktree() {
                                        "Saved locally."
                                    } else {
                                        "Draft saved."
                                    }
                                    .into();
                                }
                                redraw = true;
                            }
                            Action::Remove(index) => {
                                if let Some(note) =
                                    review.notes.get(index).filter(|note| is_draft(note, repo))
                                {
                                    match native::remove(&review.scope, &note.id) {
                                        Ok(()) => {
                                            review.notes.remove(index);
                                            redraw = true;
                                        }
                                        Err(error) => message = error,
                                    }
                                }
                            }
                            Action::CopyNote(index) => {
                                message = match (review.notes.get(index), clipboard.as_deref_mut())
                                {
                                    (Some(note), Some(clipboard)) => {
                                        clipboard.set_text(note.comment()).map_or_else(
                                            |_| "Could not copy the comment.".into(),
                                            |_| "Copied.".into(),
                                        )
                                    }
                                    _ => "Clipboard is unavailable.".into(),
                                };
                            }
                            Action::AutoPublish => {
                                if picker
                                    .as_ref()
                                    .and_then(|picker| picker.publication())
                                    .is_some()
                                {
                                    review.auto_publish = !review.auto_publish;
                                    ui.auto_revision = review
                                        .auto_publish
                                        .then(|| (repo.revision.clone(), repo.baseline.clone()));
                                    review.auto_drafts.clear();
                                    message = if review.auto_publish {
                                        "New marks will publish to this PR."
                                    } else {
                                        "New marks will save as local drafts."
                                    }
                                    .into();
                                    redraw = true;
                                }
                            }
                            Action::Publish => {
                                let drafts = review
                                    .notes
                                    .iter()
                                    .filter(|note| is_draft(note, repo))
                                    .cloned()
                                    .collect::<Vec<_>>();
                                if let Some((target, token)) =
                                    picker.as_ref().and_then(|picker| picker.publication())
                                {
                                    if !drafts.is_empty() {
                                        message = format!("Publishing {} to GitHub…", drafts.len());
                                        ui.publication = Some((
                                            review.scope.clone(),
                                            native_github::publish(target, drafts, token),
                                        ));
                                        review.auto_drafts.clear();
                                        ui.view = Some(View::Notes { editing: false });
                                        redraw = true;
                                    }
                                } else if let Some(picker) = picker.as_deref_mut() {
                                    picker.request_sign_in();
                                    ui.view = Some(View::Notes { editing: false });
                                    redraw = true;
                                }
                            }
                            _ => {}
                        },
                    }
                }
            }
            Action::CopyCode => {
                if let (Some(picker), Some(clipboard)) =
                    (picker.as_deref_mut(), clipboard.as_deref_mut())
                {
                    picker.copy_code(clipboard);
                    redraw = true;
                }
            }
            Action::SignIn => {
                if let Some(picker) = picker.as_deref_mut() {
                    picker.request_sign_in();
                    redraw = true;
                }
            }
        }
    }
    if review.auto_publish
        && ui
            .auto_revision
            .as_ref()
            .is_none_or(|(revision, base)| revision != &repo.revision || base != &repo.baseline)
    {
        review.auto_publish = false;
        review.auto_drafts.clear();
    }
    if review.auto_publish
        && ui.publication.is_none()
        && !review.auto_drafts.is_empty()
        && !matches!(ui.view, Some(View::Notes { editing: true }))
    {
        let ids = std::mem::take(&mut review.auto_drafts);
        let drafts = review
            .notes
            .iter()
            .filter(|note| ids.contains(&note.id) && is_draft(note, repo))
            .cloned()
            .collect::<Vec<_>>();
        if let Some((target, token)) = picker.as_ref().and_then(|picker| picker.publication()) {
            if !drafts.is_empty() {
                message = format!("Publishing {} to GitHub…", drafts.len());
                ui.publication = Some((
                    review.scope.clone(),
                    native_github::publish(target, drafts, token),
                ));
                redraw = ui.panel.is_some();
            }
        } else {
            review.auto_publish = false;
            message = "Auto-publish off. Drafts saved locally.".into();
        }
    }
    if repo.is_worktree()
        && !review.notes.is_empty()
        && ui.refresh_local_states(&mut review, time.elapsed_secs())
    {
        redraw |= matches!(ui.view, Some(View::Notes { editing: false }));
    }
    if !message.is_empty() {
        ui.message.clone_from(&message);
        review.notify(time.elapsed_secs(), message);
        if !redraw && let Ok(mut feedback) = feedback.single_mut() {
            feedback.0.clone_from(&ui.message);
        }
    }
    if redraw {
        if let Some(panel) = ui.panel.take() {
            native_panel::close(&mut commands, panel, &mut focus, &parents);
        }
        draw(
            &mut commands,
            &mut ui,
            &review,
            repo,
            picker.as_deref(),
            comments.as_deref(),
        );
        if let Some(field) = ui
            .fields
            .first()
            .copied()
            .or_else(|| ui.draft_fields.first().map(|(_, field)| *field))
            .or(ui.primary)
        {
            focus.set(field, FocusCause::Navigated);
        }
    }
    review.tablet_open = matches!(ui.view, Some(View::Notes { .. }));
    review.blocked = (was_open && !handoff)
        || ui.view.is_some()
        || tablet.as_ref().is_some_and(|tablet| tablet.is_open())
        || picker
            .as_ref()
            .is_some_and(|picker| picker.blocks_controls());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_paint_keeps_the_candidate_until_equip_and_focuses_that_action() {
        let mut review = Review::default();
        review.select(4);
        review.request = Some(Panel::Paints);
        let mut app = App::new();
        app.init_resource::<ReviewUi>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Explorer::new(0))
            .insert_resource(review)
            .add_systems(Update, update);
        app.update();
        app.world_mut()
            .resource_mut::<ReviewUi>()
            .activate(Action::Select(5));
        app.update();
        assert_eq!(app.world().resource::<Review>().slot, 4);
        let focused = app
            .world()
            .resource::<InputFocus>()
            .get()
            .expect("Equip must have keyboard focus");
        assert!(matches!(
            app.world().get::<Action>(focused),
            Some(Action::Equip(5))
        ));
        app.world_mut()
            .resource_mut::<ReviewUi>()
            .activate(Action::Equip(5));
        app.update();
        assert_eq!(app.world().resource::<Review>().slot, 5);
        assert!(app.world().resource::<ReviewUi>().view.is_none());
    }

    #[test]
    fn an_unconfigured_paint_can_be_used_as_a_plain_note() {
        let paint = empty_preset();
        assert_eq!(paint.label, "Note");
        assert_eq!(paint.spray, "Note");
        assert!(paint.prompt.is_empty());
    }

    #[test]
    fn local_viewed_status_is_a_separate_action_and_leaves_editable_text_unchanged() {
        let mut repo = repository_picker::empty();
        repo.revision = "worktree:test".into();
        let mut review = Review::default();
        review.notes.push(Note {
            id: "viewed-note".into(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            anchor: Anchor {
                path: "demo.rs".into(),
                before: false,
                start: 1,
                end: 1,
            },
            preset: review.palette[0].clone().unwrap(),
            text: "Explain this condition".into(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        });
        review.request = Some(Panel::Notes);
        let mut app = App::new();
        app.init_resource::<ReviewUi>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Explorer::new(0))
            .insert_resource(Repo(repo))
            .insert_resource(review)
            .add_systems(Update, update);
        app.update();
        assert!(
            app.world_mut()
                .query::<&Action>()
                .iter(app.world())
                .any(|action| matches!(action, Action::MarkViewed(0)))
        );
        app.world_mut()
            .resource_mut::<Review>()
            .local_states
            .insert("viewed-note".into(), native::LocalState::Read);
        app.world_mut()
            .resource_mut::<ReviewUi>()
            .activate(Action::EditNote(0));
        app.update();
        let field = app.world().resource::<ReviewUi>().draft_fields[0].1;
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .value()
                .to_string(),
            "Explain this condition"
        );
        assert_eq!(
            app.world().resource::<Review>().notes[0].text,
            "Explain this condition"
        );
    }

    #[test]
    fn editing_one_note_focuses_its_text_and_keeps_its_identity() {
        let repo = repository_picker::empty();
        let mut review = Review::default();
        review.notes.push(Note {
            id: "editing-note".into(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            anchor: Anchor {
                path: "demo.rs".into(),
                before: false,
                start: 3,
                end: 7,
            },
            preset: review.palette[0].clone().unwrap(),
            text: "Explain the condition".into(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        });
        let mut app = App::new();
        app.init_resource::<ReviewUi>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Explorer::new(0))
            .insert_resource(Repo(repo))
            .insert_resource(review)
            .add_systems(Update, update);
        app.world_mut()
            .resource_mut::<ReviewUi>()
            .activate(Action::EditNote(0));
        app.update();
        let ui = app.world().resource::<ReviewUi>();
        assert_eq!(ui.focused_note.as_deref(), Some("editing-note"));
        assert_eq!(ui.draft_fields.len(), 1);
        let field = ui.draft_fields[0].1;
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .value()
                .to_string(),
            "Explain the condition"
        );
    }

    #[test]
    fn aimed_review_shows_only_the_current_note_and_can_return_to_all() {
        let repo = repository_picker::empty();
        let mut review = Review::default();
        let preset = review.palette[0].clone().unwrap();
        review.notes = ["first", "aimed"]
            .into_iter()
            .map(|id| Note {
                id: id.into(),
                revision: repo.revision.clone(),
                baseline: repo.baseline.clone(),
                anchor: Anchor {
                    path: "demo.rs".into(),
                    before: false,
                    start: 7,
                    end: 7,
                },
                preset: preset.clone(),
                text: format!("Comment {id}"),
                published: None,
                remote: false,
                resolved: None,
                attention: false,
            })
            .collect();
        review.aim_note = Some("aimed".into());
        let mut ui = ReviewUi::default();
        ui.select_view(Panel::Aimed, &review, &repo, false);
        assert!(!ui.includes(&review.notes[0], &repo));
        assert!(ui.includes(&review.notes[1], &repo));
        ui.select_view(Panel::Notes, &review, &repo, false);
        assert!(review.notes.iter().all(|note| ui.includes(note, &repo)));
        review.aim_note = Some("missing".into());
        ui.select_view(Panel::Aimed, &review, &repo, false);
        assert!(review.notes.iter().all(|note| ui.includes(note, &repo)));
    }

    #[test]
    fn opening_the_paint_editor_releases_camera_and_focuses_a_prefilled_field() {
        let mut app = App::new();
        app.init_resource::<ReviewUi>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Explorer::new(0))
            .insert_resource(Review {
                request: Some(Panel::Paints),
                ..default()
            })
            .add_systems(Update, update);
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
        let window = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                CursorOptions {
                    grab_mode: CursorGrabMode::Locked,
                    visible: false,
                    ..default()
                },
            ))
            .id();
        app.update();
        assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(cursor.visible);
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(app.world().resource::<Review>().blocked);

        app.world_mut()
            .resource_mut::<ReviewUi>()
            .activate(Action::Edit(5));
        app.update();
        let ui = app.world().resource::<ReviewUi>();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(ui.fields[0])
        );
        let prompt = app
            .world()
            .get::<EditableText>(ui.fields[2])
            .unwrap()
            .value();
        assert!(prompt.to_string().contains("Explain"));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<InputFocus>().get().is_none());
        app.update();
        assert!(!app.world().resource::<Review>().blocked);
    }
}
