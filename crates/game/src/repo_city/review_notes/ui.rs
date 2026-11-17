use bevy::{
    input_focus::{FocusCause, InputFocus},
    text::EditableText,
};

use super::{native, *};
use crate::native_panel::{self, Actions, INK};

#[derive(Clone)]
enum View {
    Palette(usize),
    EditPaint(usize),
    Notes,
    Note(String),
}

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Close,
    Select(usize),
    Equip(usize),
    Edit(usize),
    Save(usize),
    Notes,
    Open(usize),
    Delete,
    Export,
    Publish,
}

#[derive(Resource, Default)]
pub(crate) struct ReviewUi {
    view: Option<View>,
    panel: Option<Entity>,
    fields: Vec<Entity>,
    action: Option<Action>,
    publication: Option<(String, native_github::Job<Note>)>,
    empty_until: f32,
}

impl Actions<Action> for ReviewUi {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl ReviewUi {
    fn select_view(&mut self, panel: Panel, review: &Review) {
        self.view = match panel {
            Panel::Paints => Some(View::Palette(review.slot)),
            Panel::Aimed
                if review
                    .aim_note
                    .as_ref()
                    .is_some_and(|id| review.notes.iter().any(|note| note.id == *id)) =>
            {
                review.aim_note.clone().map(View::Note)
            }
            Panel::Notes | Panel::Aimed if !review.notes.is_empty() => Some(View::Notes),
            _ => None,
        };
    }
}

fn button(parent: &mut ChildSpawnerCommands, label: &str, ink: Color, action: Action) {
    native_panel::button::<_, ReviewUi>(parent, label, ink, action);
}

fn paint_color(preset: &Preset) -> Color {
    let [r, g, b] = preset.color;
    Color::srgb_u8(r, g, b)
}

fn empty_preset() -> Preset {
    Preset {
        id: new_id(),
        label: String::new(),
        prompt: String::new(),
        spray: String::new(),
        color: [255, 126, 141],
    }
}

fn draw(
    commands: &mut Commands,
    ui: &mut ReviewUi,
    review: &Review,
    repo: &Repository,
    picker: Option<&repository_picker::RepositoryPicker>,
) {
    let Some(view) = &ui.view else {
        return;
    };
    let title = match view {
        View::Palette(_) => "Paints",
        View::EditPaint(_) => "Edit paint",
        View::Notes => "Review notes",
        View::Note(_) => "Review note",
    };
    let panel = native_panel::panel(commands, title, 490.);
    ui.panel = Some(panel);
    ui.fields.clear();
    commands.entity(panel).with_children(|parent| {
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
                                preset.map_or("Empty", |paint| paint.label.as_str())
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
                        "Set up a reusable comment for this slot."
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
                        if *selected == 0 || paint.is_some() {
                            button(actions, "Equip", INK, Action::Equip(*selected));
                        }
                        if *selected != 0 {
                            button(actions, "Edit paint", INK, Action::Edit(*selected));
                        }
                        button(actions, "Close", INK, Action::Close);
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
                        "Color",
                        &format!("#{r:02x}{g:02x}{b:02x}"),
                        3,
                        false,
                    ),
                ];
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        ..default()
                    })
                    .with_children(|actions| {
                        button(actions, "Save paint", INK, Action::Save(*slot));
                        button(actions, "Back", INK, Action::Select(*slot));
                        button(actions, "Close", INK, Action::Close);
                    });
            }
            View::Notes => {
                parent
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4.),
                            max_height: px(320.),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                    ))
                    .with_children(|list| {
                        for (index, note) in review.notes.iter().enumerate().rev() {
                            let label = format!(
                                "{}:{} · {}",
                                note.anchor.path, note.anchor.start, note.preset.label
                            );
                            button(list, &label, paint_color(&note.preset), Action::Open(index));
                        }
                    });
                button(parent, "Close", INK, Action::Close);
            }
            View::Note(id) => {
                if let Some(note) = review.notes.iter().find(|note| &note.id == id) {
                    native_panel::text(
                        parent,
                        &format!(
                            "{}:{}–{}",
                            note.anchor.path, note.anchor.start, note.anchor.end
                        ),
                        INK,
                    );
                    native_panel::text(parent, &note.preset.label, paint_color(&note.preset));
                    let comment = if note.text.trim().is_empty() {
                        &note.preset.prompt
                    } else {
                        &note.text
                    };
                    native_panel::text(parent, comment, Color::srgb_u8(221, 235, 240));
                    if note.published.is_some() {
                        native_panel::text(parent, "Published on GitHub", INK);
                    } else if picker
                        .and_then(|picker| picker.github.as_ref())
                        .is_some_and(native_github::Target::is_pull)
                        && Review::on_snapshot(note, repo)
                    {
                        button(parent, "Publish to GitHub", INK, Action::Publish);
                    }
                    parent
                        .spawn(Node {
                            column_gap: px(8.),
                            ..default()
                        })
                        .with_children(|actions| {
                            if note.published.is_none() {
                                button(actions, "Delete draft", INK, Action::Delete);
                            }
                            button(actions, "Export", INK, Action::Export);
                            button(actions, "Back", INK, Action::Notes);
                            button(actions, "Close", INK, Action::Close);
                        });
                }
            }
        }
        parent.spawn((
            Feedback,
            Text::new(""),
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
    let color = color.trim().trim_start_matches('#');
    let rgb = (color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(color, 16).expect("validated color"))
        .ok_or("Enter a color as #RRGGBB.")?;
    let mut palette = review.palette.clone();
    let mut paint = palette[slot - 1].take().unwrap_or_else(empty_preset);
    paint.label = name.into();
    paint.spray = spray.clone();
    paint.prompt = prompt.clone();
    let [_, red, green, blue] = rgb.to_be_bytes();
    paint.color = [red, green, blue];
    palette[slot - 1] = Some(paint);
    native::save(&review.scope, "palette", &palette)?;
    review.palette = palette;
    review.select(slot);
    Ok(())
}

pub(crate) fn update(
    mut commands: Commands,
    mut review: ResMut<Review>,
    mut ui: ResMut<ReviewUi>,
    mut explorer: ResMut<Explorer>,
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
    picker: Option<Res<repository_picker::RepositoryPicker>>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let fallback = repository_picker::empty();
    let repo = repo.as_ref().map_or(&fallback, |repo| &repo.0);
    let mut redraw = false;
    let mut message = String::new();
    if let Some(result) = ui.publication.as_ref().and_then(|(_, job)| job.take()) {
        let (scope, _) = ui.publication.take().expect("pending publication");
        match result.and_then(|note| {
            native::save(&scope, &note.id, &note)?;
            Ok(note)
        }) {
            Ok(note) => {
                if scope == review.scope
                    && let Some(saved) = review.notes.iter_mut().find(|saved| saved.id == note.id)
                {
                    *saved = note;
                    redraw = ui.panel.is_some();
                }
                explorer.notice = "Published on GitHub.".into();
            }
            Err(error) => message = error,
        }
    }
    let was_open = ui.view.is_some();
    if let Some(panel) = review.request.take() {
        ui.select_view(panel, &review);
        if ui.view.is_none() {
            ui.empty_until = time.elapsed_secs() + 3.5;
        } else {
            ui.empty_until = 0.;
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

    let action = ui
        .action
        .take()
        .or_else(|| (was_open && keys.just_pressed(KeyCode::Escape)).then_some(Action::Close));
    if let Some(action) = action {
        match action {
            Action::Close => {
                ui.view = None;
                redraw = true;
            }
            Action::Select(slot) => {
                ui.view = Some(View::Palette(slot));
                redraw = true;
            }
            Action::Equip(slot) => {
                review.select(slot);
                ui.view = None;
                redraw = true;
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
            Action::Notes => {
                ui.view = Some(View::Notes);
                redraw = true;
            }
            Action::Open(index) => {
                if let Some(note) = review.notes.get(index) {
                    ui.view = Some(View::Note(note.id.clone()));
                    redraw = true;
                }
            }
            Action::Delete | Action::Export | Action::Publish => {
                if let Some(View::Note(id)) = &ui.view
                    && let Some(note) = review.notes.iter().find(|note| note.id == *id).cloned()
                {
                    match action {
                        Action::Delete if note.published.is_none() && ui.publication.is_none() => {
                            match native::remove(&review.scope, &note.id) {
                                Ok(()) => {
                                    review.notes.retain(|other| other.id != note.id);
                                    ui.view = if review.notes.is_empty() {
                                        None
                                    } else {
                                        Some(View::Notes)
                                    };
                                    redraw = true;
                                }
                                Err(error) => message = error,
                            }
                        }
                        Action::Export => {
                            if let Err(error) = native::export(review.scope.clone(), note) {
                                message = error;
                            }
                        }
                        Action::Publish
                            if note.published.is_none()
                                && ui.publication.is_none()
                                && Review::on_snapshot(&note, repo) =>
                        {
                            match picker.as_ref().and_then(|picker| picker.publication()) {
                                Some((target, token)) => {
                                    ui.publication = Some((
                                        review.scope.clone(),
                                        native_github::publish(target, note, token),
                                    ));
                                    message = "Publishing…".into();
                                }
                                None => {
                                    message =
                                        "Sign in from the GitHub menu to publish this draft.".into()
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if redraw {
        if let Some(panel) = ui.panel.take() {
            native_panel::close(&mut commands, panel, &mut focus, &parents);
        }
        draw(&mut commands, &mut ui, &review, repo, picker.as_deref());
        if let Some(field) = ui.fields.first().filter(|_| ui.panel.is_some()) {
            focus.set(*field, FocusCause::Navigated);
        }
    }
    if !message.is_empty() {
        if let Ok(mut feedback) = feedback.single_mut() {
            feedback.0.clone_from(&message);
        }
        explorer.notice = message;
    }
    review.blocked =
        was_open || ui.view.is_some() || picker.is_some_and(|picker| picker.blocks_controls());
}

#[cfg(test)]
mod tests {
    use super::*;

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
