//! The native VibePad projects the room's bounded chat history and its draft.
use super::*;
use bevy::text::EditableText;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Entry {
    seq: u64,
    #[serde(flatten)]
    message: BoardMessage,
}

#[derive(Default)]
pub(super) struct Chat {
    entries: Vec<Entry>,
    sketch: Option<sketch::Sketch>,
    draft: String,
    retry: Option<(String, String)>,
    pending: Option<(Action, native_github::Job<Value>)>,
    feedback: String,
    action: Option<Action>,
}

impl Chat {
    pub(super) fn receive(&mut self, event: &Value) {
        if event["type"] == "sketch" {
            self.sketch = serde_json::from_value(event.clone()).ok();
            return;
        }
        let Ok(entry) = serde_json::from_value::<Entry>(event.clone()) else {
            return;
        };
        match self
            .entries
            .binary_search_by_key(&entry.seq, |item| item.seq)
        {
            Ok(_) => return,
            Err(position) => self.entries.insert(position, entry),
        }
        if self.entries.len() > 32 {
            self.entries.remove(0);
        }
    }

    pub(super) fn board_content(&self) -> BoardContent {
        BoardContent {
            chat: self
                .entries
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|entry| entry.message.clone())
                .collect(),
            sketch: self.sketch.clone(),
        }
    }
}

#[derive(Component)]
struct Draft(bool);
#[derive(Component)]
struct Feedback;
#[derive(Component, Default)]
struct History(Option<u64>);
#[derive(Component, Clone, Copy)]
enum Action {
    SendMessage,
    ShareSketch,
}

impl native_panel::Actions<Action> for Party {
    fn activate(&mut self, action: Action) {
        self.native.chat.action = Some(action);
    }
}

pub(in super::super) fn spawn(parent: &mut ChildSpawnerCommands, host: bool) -> Entity {
    native_panel::text(parent, "Room chat", native_panel::INK);
    agent_timeline::spawn(parent, agent_timeline::View::Chat);
    let field = native_panel::input(parent, "Room message", "", 100, true);
    parent.commands().entity(field).insert(Draft(false));
    parent
        .spawn(Node {
            column_gap: px(10.),
            ..default()
        })
        .with_children(|row| {
            native_panel::button::<_, Party>(
                row,
                "Send message",
                native_panel::INK,
                Action::SendMessage,
            );
            if host {
                native_panel::button::<_, Party>(
                    row,
                    "Share staged sketch",
                    native_panel::INK,
                    Action::ShareSketch,
                );
            }
        });
    parent.spawn((
        Feedback,
        Text::default(),
        native_panel::panel_font(),
        TextColor(native_panel::INK),
        Node {
            min_height: px(24.),
            ..default()
        },
    ));
    parent.spawn((
        History::default(),
        Node {
            width: percent(100.),
            flex_direction: FlexDirection::Column,
            row_gap: px(8.),
            ..default()
        },
    ));
    field
}

pub(super) fn register(app: &mut App) {
    app.add_systems(
        Update,
        update.after(native::update).after(review_tablet::update),
    );
}

fn update(
    mut commands: Commands,
    mut party: ResMut<Party>,
    repo: Res<Repo>,
    mut fields: Query<(&mut EditableText, &mut Draft)>,
    mut feedback: Query<&mut Text, With<Feedback>>,
    mut histories: Query<(Entity, &mut History)>,
    buttons: Query<(Entity, &Action, Has<bevy::ui::InteractionDisabled>)>,
) {
    if party.chat_room().is_none() {
        return;
    }
    let chat = &mut party.native.chat;
    if let Ok((mut field, mut marker)) = fields.single_mut() {
        if !marker.0 {
            field.editor_mut().set_text(&chat.draft);
            marker.0 = true;
        }
        chat.draft = field.editor().text().to_string();
    }
    if let Some((action, result)) = chat
        .pending
        .as_ref()
        .and_then(|(action, job)| job.take().map(|result| (*action, result)))
    {
        chat.pending = None;
        match (action, result) {
            (Action::SendMessage, Ok(_)) => {
                if chat
                    .retry
                    .take()
                    .is_some_and(|(_, text)| text == chat.draft.trim())
                {
                    chat.draft.clear();
                    if let Ok((mut field, _)) = fields.single_mut() {
                        field.editor_mut().set_text("");
                    }
                }
                chat.feedback.clear();
            }
            (Action::ShareSketch, Ok(_)) => chat.feedback = "Sketch shared.".into(),
            (_, Err(error)) => chat.feedback = error,
        }
    }
    if let Some(action) = chat.action.take()
        && chat.pending.is_none()
    {
        let job = action_body(&mut party, action, &repo.0.root)
            .and_then(|body| party.native.post_event(body));
        let chat = &mut party.native.chat;
        match job {
            Ok(job) => {
                chat.pending = Some((action, job));
                chat.feedback = "Sending…".into();
            }
            Err(error) => chat.feedback = error,
        }
    }
    let chat = &party.native.chat;
    for (button, action, disabled) in &buttons {
        let enabled = chat.pending.is_none()
            && match action {
                Action::SendMessage => !chat.draft.trim().is_empty(),
                Action::ShareSketch => party.hosts_room(),
            };
        if enabled && disabled {
            commands
                .entity(button)
                .remove::<bevy::ui::InteractionDisabled>();
        } else if !enabled && !disabled {
            commands
                .entity(button)
                .insert(bevy::ui::InteractionDisabled);
        }
    }
    for mut label in &mut feedback {
        if label.0 != chat.feedback {
            label.0.clone_from(&chat.feedback);
        }
    }
    let last = chat.entries.last().map(|entry| entry.seq);
    for (entity, mut history) in &mut histories {
        if history.0 != last {
            history.0 = last;
            commands
                .entity(entity)
                .despawn_children()
                .with_children(|parent| {
                    for entry in chat.entries.iter().rev() {
                        native_panel::text(parent, &entry.message.login, native_panel::INK);
                        native_panel::text(
                            parent,
                            &entry.message.text,
                            Color::srgb_u8(194, 218, 221),
                        );
                    }
                });
        }
    }
}

fn action_body(party: &mut Party, action: Action, root: &std::path::Path) -> Result<Value, String> {
    match action {
        Action::SendMessage => {
            let chat = &mut party.native.chat;
            let text = chat.draft.trim().to_owned();
            if text.is_empty() || text.len() > 4096 {
                return Err("Write a room message of at most 4,096 bytes.".into());
            }
            let id = chat
                .retry
                .as_ref()
                .filter(|(_, previous)| previous == &text)
                .map_or_else(
                    || bevy::asset::uuid::Uuid::new_v4().to_string(),
                    |(id, _)| id.clone(),
                );
            let body = json!({"type":"chat", "id":id, "text":text});
            chat.retry = Some((id, text));
            Ok(body)
        }
        Action::ShareSketch if party.hosts_room() => {
            let sketch = root
                .to_str()
                .and_then(sketch::staged)
                .ok_or("Stage a sketch with the show_sketch MCP tool first.")?;
            Ok(
                json!({"type":"sketch", "title":sketch.title, "source":sketch.source, "format":sketch.format}),
            )
        }
        Action::ShareSketch => Err("Only the host can share a sketch.".into()),
    }
}
