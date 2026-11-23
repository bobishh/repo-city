//! Native projections of the provider action history.
use super::*;
use agent_engine::activity::{Entry, Kind, Phase, Timeline};
use std::collections::HashMap;

#[derive(Component, Clone, Copy)]
enum Action {
    Toggle,
    Inspect(u64),
}

#[derive(Resource, Default)]
struct State {
    open: bool,
    inspected: Option<u64>,
    version: u64,
}

impl native_panel::Actions<Action> for State {
    fn activate(&mut self, action: Action) {
        match action {
            Action::Toggle => self.open = !self.open,
            Action::Inspect(serial) => {
                self.inspected = (self.inspected != Some(serial)).then_some(serial)
            }
        }
        self.version = self.version.wrapping_add(1);
    }
}

#[derive(Component)]
struct Summary;

#[derive(Component)]
struct Rows {
    scroll: Entity,
    drawn: Option<(u64, u64)>,
}

pub(super) struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<State>().add_systems(Update, update);
    }
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: percent(100.),
            align_items: AlignItems::Center,
            column_gap: px(9.),
            ..default()
        })
        .with_children(|bar| {
            native_panel::button::<_, State>(
                bar,
                "Agent activity",
                native_panel::INK,
                Action::Toggle,
            );
            bar.spawn((
                Summary,
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
                scroll,
                drawn: None,
            },
            Node {
                width: percent(100.),
                height: px(205.),
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
    mut summaries: Query<&mut Text, With<Summary>>,
    mut views: Query<(&mut Rows, &mut Node)>,
) {
    let timeline = session.timeline(&repo.0.root);
    let entries = timeline.map(|timeline| &timeline.entries);
    let version = timeline.map_or(0, |timeline| timeline.version);
    let latest = entries.and_then(|events| {
        events
            .iter()
            .max_by_key(|entry| (entry.updated, entry.serial))
    });
    for mut label in &mut summaries {
        let value = latest.map_or_else(
            || "No activity in this checkout yet.".into(),
            |entry| format!("{} · {}", entry.phase.label(), entry.details.title),
        );
        if label.0 != value {
            label.0 = value;
        }
    }
    for (mut rows, mut node) in &mut views {
        let visible = state.open && latest.is_some();
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        let stamp = (state.version, version);
        if !visible || (rows.drawn == Some(stamp) && !repo.is_changed()) {
            continue;
        }
        rows.drawn = Some(stamp);
        let Some(entries) = entries else { continue };
        commands
            .entity(rows.scroll)
            .despawn_children()
            .with_children(|parent| {
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
            });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileActivity {
    pub line: usize,
    updated: u64,
    serial: u64,
    kind: &'static str,
}

impl FileActivity {
    fn new(entry: &Entry, line: usize) -> Self {
        let kind = match (entry.details.kind, entry.phase) {
            (Kind::Edit, Phase::Completed) => "edited",
            (Kind::Edit, Phase::Failed) => "edit failed",
            (Kind::Edit, _) => "editing",
            (Kind::Tool, _) => "tool target",
            (Kind::Command, Phase::Running) => "reading",
            (_, Phase::Failed) => "read failed",
            _ => "read",
        };
        Self {
            line,
            updated: entry.updated,
            serial: entry.serial,
            kind,
        }
    }

    pub(super) fn order(self) -> (u64, u64) {
        (self.updated, self.serial)
    }

    pub(super) fn label(self, now: u64, include_age: bool) -> String {
        let action = format!("Agent {} · L{}", self.kind, self.line);
        if !include_age {
            return action;
        }
        let age = match now.saturating_sub(self.updated) {
            0..5 => "just now".into(),
            time @ 5..60 => format!("{time}s ago"),
            time @ 60..3600 => format!("{}m ago", time / 60),
            time => format!("{}h ago", time / 3600),
        };
        format!("{action} · {age}")
    }
}

pub(super) fn files(timeline: &Timeline) -> HashMap<String, FileActivity> {
    let mut entries = timeline
        .entries
        .iter()
        .filter(|entry| !entry.details.locations.is_empty())
        .collect::<Vec<_>>();
    entries.sort_unstable_by_key(|entry| std::cmp::Reverse((entry.updated, entry.serial)));
    let mut files = HashMap::new();
    for entry in entries {
        for location in &entry.details.locations {
            files
                .entry(location.path.clone())
                .or_insert_with(|| FileActivity::new(entry, location.line));
        }
    }
    files
}
