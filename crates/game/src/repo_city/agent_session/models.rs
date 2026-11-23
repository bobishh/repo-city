//! Model choices come from the connected provider; only explicit choices persist.
use super::*;
use agent_engine::models::Catalog;

#[derive(Default)]
pub(super) struct Picker {
    catalog: Option<Catalog>,
    error: Option<String>,
    pending: Option<String>,
    loading: bool,
    expanded: bool,
    revision: u64,
    action: Option<ModelAction>,
}
impl Picker {
    pub(super) fn failure(&self) -> Option<String> {
        self.error.clone()
    }
    pub(super) fn selecting(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn reset(&mut self) {
        self.catalog = None;
        self.error = None;
        self.pending = None;
        self.loading = false;
        self.expanded = false;
        self.action = None;
        self.revision = self.revision.wrapping_add(1);
    }
    pub(super) fn begin(&mut self) {
        self.loading = true;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }
    pub(super) fn apply(
        &mut self,
        result: Result<Catalog, String>,
        provider: agent_engine::Provider,
    ) {
        self.loading = false;
        self.revision = self.revision.wrapping_add(1);
        match result {
            Ok(catalog) => {
                self.error = None;
                if let Some(requested) = self.pending.take() {
                    self.error = if catalog.current.as_deref() == Some(requested.as_str()) {
                        self.expanded = false;
                        save_choice(provider, Some(requested.as_str())).err()
                    } else {
                        Some("Provider did not confirm the selected model.".into())
                    };
                }
                self.catalog = Some(catalog);
            }
            Err(error) => {
                self.pending = None;
                self.error = Some(error);
            }
        }
    }
}

fn preferences() -> Result<PathBuf, String> {
    repository_picker::settings_file()
        .map(|p| p.with_file_name("agent-models.json"))
        .ok_or_else(|| "Could not locate model preferences.".into())
}
fn choices(path: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Could not read model preferences: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub(super) fn preferred(provider: agent_engine::Provider) -> Result<Option<String>, String> {
    Ok(choices(&preferences()?)?.remove(provider.id))
}
fn save_choice(provider: agent_engine::Provider, model: Option<&str>) -> Result<(), String> {
    save_at(&preferences()?, provider, model)
}
fn save_at(
    path: &Path,
    provider: agent_engine::Provider,
    model: Option<&str>,
) -> Result<(), String> {
    let mut choices = choices(path)?;
    if let Some(model) = model {
        choices.insert(provider.id.into(), model.into());
    } else {
        choices.remove(provider.id);
    }
    write_state(
        path,
        &serde_json::to_vec_pretty(&choices).map_err(|e| e.to_string())?,
    )
}

#[derive(Component, Clone, Copy)]
enum ModelAction {
    Toggle,
    Refresh,
    ProviderSettings,
    Choose {
        index: usize,
        revision: u64,
        provider: agent_engine::Provider,
    },
}
impl native_panel::Actions<ModelAction> for Session {
    fn activate(&mut self, action: ModelAction) {
        self.models.action = Some(action);
    }
}
#[derive(Component, Default)]
pub(super) struct ModelPanel {
    rendered: Option<(u64, bool, bool)>,
    list: Option<Entity>,
}

pub(super) fn settings(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        ModelPanel::default(),
        Node {
            width: percent(100.),
            flex_direction: FlexDirection::Column,
            row_gap: px(6.),
            ..default()
        },
    ));
}
fn busy(session: &Session) -> bool {
    session.projection.turn.is_some()
        || session.pending_send.is_some()
        || session.projection.voice.is_some()
        || session.voice_waiting
        || session.voice_connected
        || session.stopping
}
fn act(session: &mut Session, action: ModelAction) -> Result<(), String> {
    if matches!(action, ModelAction::Toggle) {
        session.models.expanded = !session.models.expanded;
        return Ok(());
    }
    if busy(session) {
        return Err("Stop agent work and voice before changing models.".into());
    }
    if session.models.loading {
        return Err("Wait for the provider's model response.".into());
    }
    match action {
        ModelAction::Toggle => unreachable!(),
        ModelAction::Refresh => {
            let connected = session.client.is_some();
            session.connect()?;
            session.models.begin();
            if connected {
                session.send(Command::RefreshModels)?;
            }
        }
        ModelAction::ProviderSettings => {
            save_choice(session.provider(), None)?;
            session.disconnect();
            session.connect()?;
        }
        ModelAction::Choose {
            index,
            revision,
            provider,
        } => {
            if revision != session.models.revision || provider != session.provider() {
                return Err("The model list changed. Choose again.".into());
            }
            let choice = session
                .models
                .catalog
                .as_ref()
                .and_then(|c| c.choices.get(index))
                .ok_or("This model is no longer available.")?
                .id
                .clone();
            session.send(Command::SelectModel(choice.clone()))?;
            session.models.pending = Some(choice);
            session.models.begin();
        }
    }
    Ok(())
}
pub(super) fn update(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut panels: Query<(Entity, &mut ModelPanel)>,
    positions: Query<&ScrollPosition>,
) {
    if let Some(action) = session.models.action.take() {
        if let Err(error) = act(&mut session, action) {
            session.models.error = Some(error);
            session.models.revision = session.models.revision.wrapping_add(1);
        }
    }
    let busy = busy(&session);
    for (entity, mut panel) in &mut panels {
        let expanded = session.models.expanded;
        let stamp = (session.models.revision, busy, expanded);
        if panel.rendered == Some(stamp) {
            continue;
        }
        panel.rendered = Some(stamp);
        let position = panel
            .list
            .and_then(|entity| positions.get(entity).ok())
            .cloned()
            .unwrap_or_default();
        panel.list = None;
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|body| {
                let current = session
                    .models
                    .catalog
                    .as_ref()
                    .and_then(|catalog| {
                        let id = catalog.current.as_deref()?;
                        Some(
                            catalog
                                .choices
                                .iter()
                                .find(|choice| choice.id == id)
                                .map_or(id, |choice| choice.name.as_str()),
                        )
                    })
                    .unwrap_or("provider settings");
                let button = native_panel::button::<_, Session>(
                    body,
                    &format!("Model: {current} {}", if expanded { "▴" } else { "▾" }),
                    native_panel::INK,
                    ModelAction::Toggle,
                );
                body.commands()
                    .entity(button)
                    .insert(bevy::input_focus::tab_navigation::TabIndex(320));
                if !expanded {
                    if session.models.loading {
                        native_panel::text(body, "Loading models…", native_panel::INK);
                    } else if let Some(error) = &session.models.error {
                        native_panel::text(body, error, native_panel::INK);
                    }
                    return;
                }
                body.spawn(Node {
                    height: px(64.),
                    min_height: px(64.),
                    column_gap: px(6.),
                    row_gap: px(4.),
                    flex_wrap: FlexWrap::Wrap,
                    align_content: AlignContent::Start,
                    ..default()
                })
                .with_children(|row| {
                    for (offset, (label, action)) in [
                        ("Refresh models", ModelAction::Refresh),
                        ("Use provider settings", ModelAction::ProviderSettings),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let button = native_panel::button::<_, Session>(
                            row,
                            label,
                            native_panel::INK,
                            action,
                        );
                        row.commands().entity(button).insert(
                            bevy::input_focus::tab_navigation::TabIndex(321 + offset as i32),
                        );
                        if busy || session.models.loading {
                            row.commands()
                                .entity(button)
                                .insert(bevy::ui::InteractionDisabled);
                        }
                    }
                });
                let status = if session.models.loading {
                    "Loading models…"
                } else if let Some(error) = session.models.error.as_deref() {
                    error
                } else if busy {
                    "Model changes are paused while the agent is working."
                } else if session
                    .models
                    .catalog
                    .as_ref()
                    .is_some_and(|c| c.choices.is_empty())
                {
                    "Provider returned no selectable models."
                } else if session.models.catalog.is_none() {
                    "Connect or refresh to load the provider's model list."
                } else {
                    ""
                };
                body.spawn((
                    ScrollPosition::default(),
                    Node {
                        height: px(48.),
                        min_height: px(48.),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|area| {
                    native_panel::text(area, status, native_panel::INK);
                });
                let list = body
                    .spawn((
                        tablet_scroll::Area,
                        position,
                        Node {
                            height: px(120.),
                            min_height: px(120.),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::scroll_y(),
                            row_gap: px(4.),
                            ..default()
                        },
                    ))
                    .with_children(|list| {
                        if let Some(catalog) = &session.models.catalog {
                            for (index, choice) in catalog.choices.iter().enumerate() {
                                let button = native_panel::button::<_, Session>(
                                    list,
                                    &choice.name,
                                    native_panel::INK,
                                    ModelAction::Choose {
                                        index,
                                        revision: session.models.revision,
                                        provider: session.provider(),
                                    },
                                );
                                list.commands().entity(button).insert(
                                    bevy::input_focus::tab_navigation::TabIndex(323 + index as i32),
                                );
                                if catalog.current.as_deref() == Some(choice.id.as_str()) {
                                    list.commands()
                                        .entity(button)
                                        .insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                                }
                                if busy || session.models.loading {
                                    list.commands()
                                        .entity(button)
                                        .insert(bevy::ui::InteractionDisabled);
                                }
                            }
                        }
                    })
                    .id();
                panel.list = Some(list);
            });
    }
}

impl Session {
    pub(crate) fn smoke_models(&mut self, error: bool) {
        use agent_engine::models::Model;
        let catalog = Catalog {
            choices: vec![
                Model {
                    id: "preview-default".into(),
                    name: "Preview default model".into(),
                },
                Model {
                    id: "preview-fast".into(),
                    name: "Preview fast model".into(),
                },
            ],
            current: Some("preview-default".into()),
        };
        self.models.apply(Ok(catalog), self.provider());
        if error {
            self.models.apply(Err("Preview: provider could not refresh models. Previous choices remain available.".into()), self.provider());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_choices_and_busy_turns_never_send_model_changes() {
        let mut session = Session::default();
        session.models.apply(
            Ok(Catalog {
                choices: vec![agent_engine::models::Model {
                    id: "a".into(),
                    name: "A".into(),
                }],
                current: Some("a".into()),
            }),
            agent_engine::MUSE,
        );
        let stale = ModelAction::Choose {
            index: 0,
            revision: session.models.revision - 1,
            provider: session.provider(),
        };
        assert!(act(&mut session, stale).unwrap_err().contains("changed"));
        session.projection.turn = Some("working".into());
        assert!(
            act(&mut session, ModelAction::Refresh)
                .unwrap_err()
                .contains("Stop")
        );
        assert!(session.client.is_none());
    }
    #[test]
    fn model_settings_preserve_choices_on_failure_and_disable_controls_during_work() {
        let mut app = App::new();
        app.init_resource::<Session>().add_systems(Update, update);
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(settings);
        app.world_mut().flush();
        app.world_mut()
            .resource_mut::<Session>()
            .smoke_models(false);
        app.update();
        assert!(
            app.world_mut()
                .query::<&ModelPanel>()
                .single(app.world())
                .unwrap()
                .list
                .is_none()
        );
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|t| t.0 == "Model: Preview default model ▾")
        );
        app.world_mut().resource_mut::<Session>().models.action = Some(ModelAction::Toggle);
        app.update();
        let list = app
            .world_mut()
            .query::<&ModelPanel>()
            .single(app.world())
            .unwrap()
            .list
            .unwrap();
        app.world_mut().get_mut::<ScrollPosition>(list).unwrap().y = 99.;
        app.world_mut().resource_mut::<Session>().models.begin();
        app.update();
        let list = app
            .world_mut()
            .query::<&ModelPanel>()
            .single(app.world())
            .unwrap()
            .list
            .unwrap();
        assert_eq!(app.world().get::<ScrollPosition>(list).unwrap().y, 99.);
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|t| t.0 == "Loading models…")
        );
        app.world_mut().resource_mut::<Session>().smoke_models(true);
        app.update();
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|t| t.0.contains("Previous choices remain"))
        );
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|t| t.0 == "Preview fast model")
        );
        app.world_mut().resource_mut::<Session>().projection.turn = Some("turn".into());
        app.update();
        assert!(matches!(
            app.world_mut()
                .query_filtered::<&ModelAction, Without<bevy::ui::InteractionDisabled>>()
                .single(app.world()),
            Ok(ModelAction::Toggle)
        ));
    }
    #[test]
    fn model_preferences_are_isolated_by_provider_and_bad_data_is_preserved() {
        let folder = std::env::temp_dir().join(format!(
            "city-model-preferences-{}-{}",
            std::process::id(),
            nonce()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("models.json");
        save_at(&path, agent_engine::CODEX, Some("first")).unwrap();
        save_at(&path, agent_engine::MUSE, Some("second")).unwrap();
        assert_eq!(choices(&path).unwrap()["codex"], "first");
        assert_eq!(choices(&path).unwrap()["muse"], "second");
        save_at(&path, agent_engine::CODEX, None).unwrap();
        assert!(!choices(&path).unwrap().contains_key("codex"));
        assert_eq!(choices(&path).unwrap()["muse"], "second");
        std::fs::write(&path, "broken").unwrap();
        assert!(save_at(&path, agent_engine::MUSE, None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        std::fs::remove_dir_all(folder).unwrap();
    }
}
