//! Native projections of the same provider action history in Chat and Files.
use super::*;
use agent_engine::activity::{self, Entry, Kind, Phase};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Chat,
    Files,
}

#[derive(Component, Clone, Copy)]
enum Action {
    Toggle(View),
    Inspect(u64),
}

#[derive(Resource)]
struct State {
    chat_open: bool,
    files_open: bool,
    inspected: Option<u64>,
    version: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            chat_open: false,
            files_open: true,
            inspected: None,
            version: 0,
        }
    }
}

impl State {
    fn open(&self, view: View) -> bool {
        match view {
            View::Chat => self.chat_open,
            View::Files => self.files_open,
        }
    }
}

impl native_panel::Actions<Action> for State {
    fn activate(&mut self, action: Action) {
        match action {
            Action::Toggle(View::Chat) => self.chat_open = !self.chat_open,
            Action::Toggle(View::Files) => self.files_open = !self.files_open,
            Action::Inspect(serial) => {
                self.inspected = (self.inspected != Some(serial)).then_some(serial)
            }
        }
        self.version = self.version.wrapping_add(1);
    }
}

#[derive(Component)]
struct Summary(View);

#[derive(Component)]
struct Rows {
    view: View,
    scroll: Entity,
    drawn: Option<(u64, u64, u64)>,
}

pub(super) struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<State>().add_systems(Update, update);
    }
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands, view: View) {
    parent
        .spawn(Node {
            width: percent(100.),
            align_items: AlignItems::Center,
            column_gap: px(9.),
            ..default()
        })
        .with_children(|bar| {
            let label = if view == View::Chat {
                "Agent activity"
            } else {
                "Recent agent files"
            };
            native_panel::button::<_, State>(bar, label, native_panel::INK, Action::Toggle(view));
            bar.spawn((
                Summary(view),
                Text::default(),
                native_panel::panel_font(),
                TextColor(Color::srgb_u8(151, 184, 189)),
                Node {
                    flex_grow: 1.,
                    min_width: px(0.),
                    max_height: px(34.),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
        });
    let scroll = parent
        .commands()
        .spawn((
            tablet_scroll::Area,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                min_height: px(0.),
                row_gap: px(4.),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .id();
    parent
        .spawn((
            Rows {
                view,
                scroll,
                drawn: None,
            },
            Node {
                width: percent(100.),
                height: px(if view == View::Chat { 205. } else { 110. }),
                flex_shrink: 0.,
                display: Display::None,
                ..default()
            },
        ))
        .add_child(scroll);
}

fn update(
    mut commands: Commands,
    state: Res<State>,
    session: Res<agent_session::Session>,
    repo: Res<Repo>,
    mut summaries: Query<(&Summary, &mut Text)>,
    mut views: Query<(&mut Rows, &mut Node)>,
) {
    let timeline = session.timeline(&repo.0.root);
    let entries = timeline.map(|timeline| &timeline.entries);
    let version = timeline.map_or(0, |timeline| timeline.version);
    let now = activity::now();
    let latest = entries.and_then(|events| {
        events
            .iter()
            .max_by_key(|entry| (entry.updated, entry.serial))
    });
    let latest_file = entries.and_then(|events| {
        events
            .iter()
            .filter(|entry| !entry.details.locations.is_empty())
            .max_by_key(|entry| (entry.updated, entry.serial))
    });
    for (summary, mut label) in &mut summaries {
        let value = match summary.0 {
            View::Chat => {
                latest.map(|entry| format!("{} · {}", entry.phase.label(), entry.details.title))
            }
            View::Files => latest_file.map(|entry| {
                format!(
                    "{} · {}",
                    entry.details.locations[0].path,
                    age(entry.updated, now)
                )
            }),
        }
        .unwrap_or_else(|| "No activity in this checkout yet.".into());
        if label.0 != value {
            label.0 = value;
        }
    }
    for (mut rows, mut node) in &mut views {
        let visible = state.open(rows.view)
            && if rows.view == View::Chat {
                latest.is_some()
            } else {
                latest_file.is_some()
            };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        let stamp = (
            state.version,
            version,
            if rows.view == View::Files {
                now / 10
            } else {
                0
            },
        );
        if !visible || (rows.drawn == Some(stamp) && !repo.is_changed()) {
            continue;
        }
        rows.drawn = Some(stamp);
        let Some(entries) = entries else {
            continue;
        };
        commands
            .entity(rows.scroll)
            .despawn_children()
            .with_children(|parent| match rows.view {
                View::Chat => {
                    for entry in entries.iter().rev().take(32) {
                        let marked = state.inspected == Some(entry.serial);
                        let owner = session.activity_owner(&entry.thread);
                        let marker = if marked { "−" } else { "+" };
                        let label = format!(
                            "{marker} {} · {} · {} · {}",
                            owner,
                            entry.details.kind.label(),
                            entry.phase.label(),
                            entry.details.title
                        );
                        native_panel::button::<_, State>(
                            parent,
                            &label,
                            native_panel::INK,
                            Action::Inspect(entry.serial),
                        );
                        if marked {
                            if !entry.details.body.trim().is_empty() {
                                native_panel::text(
                                    parent,
                                    &entry.details.body,
                                    Color::srgb_u8(204, 220, 224),
                                );
                            }
                            for location in &entry.details.locations {
                                let label = format!("{}:{}", location.path, location.line);
                                agent_navigation::draw_measured_link(
                                    parent,
                                    &repo.0,
                                    &location.path,
                                    location.line,
                                    &label,
                                );
                            }
                        }
                    }
                }
                View::Files => recent_files(parent, &repo.0, entries, now),
            });
    }
}

fn recent_files(
    parent: &mut ChildSpawnerCommands,
    repo: &Repository,
    entries: &std::collections::VecDeque<Entry>,
    now: u64,
) {
    let mut entries = entries
        .iter()
        .filter(|entry| !entry.details.locations.is_empty())
        .collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| std::cmp::Reverse((entry.updated, entry.serial)));
    let mut seen = HashSet::new();
    for (entry, location) in entries
        .into_iter()
        .flat_map(|entry| {
            entry
                .details
                .locations
                .iter()
                .map(move |location| (entry, location))
        })
        .filter(|(_, location)| seen.insert(location.path.as_str()))
        .take(6)
    {
        let kind = match (entry.details.kind, entry.phase) {
            (Kind::Edit, Phase::Completed) => "Edited",
            (Kind::Edit, Phase::Failed) => "Edit failed",
            (Kind::Edit, _) => "Edit",
            (Kind::Tool, _) => "Tool target",
            (Kind::Command, Phase::Running) => "Reading",
            (_, Phase::Failed) => "Read failed",
            _ => "Read",
        };
        let label = format!(
            "{}:{} · {} · {}",
            location.path,
            location.line,
            kind,
            age(entry.updated, now)
        );
        agent_navigation::draw_measured_link(parent, repo, &location.path, location.line, &label);
    }
}

fn age(updated: u64, now: u64) -> String {
    match now.saturating_sub(updated) {
        0..5 => "just now".into(),
        time @ 5..60 => format!("{time}s ago"),
        time @ 60..3600 => format!("{}m ago", time / 60),
        time => format!("{}h ago", time / 3600),
    }
}
