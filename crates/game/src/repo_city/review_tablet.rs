//! Browse changed files and return to their buildings.
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    ui::RelativeCursorPosition,
};

use super::*;

const FILES_PER_PAGE: usize = 10;
type FileTimes = HashMap<String, u64>;

#[derive(Resource, Default)]
pub(super) struct Tablet {
    panel: Option<Entity>,
    action: Option<Action>,
    pending: Option<native_github::Job<FileTimes>>,
    times: FileTimes,
    scope: Option<(PathBuf, String, Option<String>)>,
    viewed_only: bool,
    page: usize,
}

#[derive(Component, Clone, Copy)]
enum Action {
    Visit(usize),
    Filter(bool),
    Page(usize),
    Close,
}

impl native_panel::Actions<Action> for Tablet {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

#[derive(Clone, Copy)]
enum Freshness {
    Watched(f64),
    Saved(u64),
    Committed(u64),
    Unknown,
}

impl Freshness {
    fn order(self) -> (bool, u64) {
        match self {
            Self::Watched(time) => (true, (time * 1000.) as u64),
            Self::Saved(time) | Self::Committed(time) => (false, time),
            Self::Unknown => (false, 0),
        }
    }

    fn label(self, elapsed: f64, now: u64) -> String {
        let (verb, age) = match self {
            Self::Watched(time) => ("Updated", (elapsed - time).max(0.) as u64),
            Self::Saved(time) => ("Saved", now.saturating_sub(time)),
            Self::Committed(time) => ("Committed", now.saturating_sub(time)),
            Self::Unknown => return String::new(),
        };
        let age = match age {
            0..60 => format!("{age}s"),
            60..3600 => format!("{}m", age / 60),
            3600..86400 => format!("{}h", age / 3600),
            _ => format!("{}d", age / 86400),
        };
        format!("{verb} {age} ago")
    }
}

fn changed_files<'a>(
    repo: &'a Repository,
    progress: &progress::Progress,
    live: &live_worktree::Live,
    times: &FileTimes,
    viewed_only: bool,
) -> Vec<(usize, &'a repo_world::File, Freshness)> {
    let mut files = repo
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            if file.change == Change::Unchanged || (viewed_only && !progress.has_visited(index)) {
                return None;
            }
            let freshness = if repo.is_worktree() {
                live.touched_at(&file.path)
                    .map(Freshness::Watched)
                    .or_else(|| times.get(&file.path).copied().map(Freshness::Saved))
            } else {
                times.get(&file.path).copied().map(Freshness::Committed)
            };
            Some((index, file, freshness.unwrap_or(Freshness::Unknown)))
        })
        .collect::<Vec<_>>();
    files.sort_unstable_by(|(_, left, a), (_, right, b)| {
        b.order()
            .cmp(&a.order())
            .then_with(|| left.path.cmp(&right.path))
    });
    files
}

fn load_times(repo: &Repository) -> native_github::Job<FileTimes> {
    let root = repo.root.clone();
    let revision = repo.revision.clone();
    let baseline = repo.baseline.clone();
    if repo.is_worktree() {
        let paths = repo
            .files
            .iter()
            .filter(|file| file.change != Change::Unchanged && file.after.is_some())
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        native_github::Job::spawn(move || {
            Ok(paths
                .into_iter()
                .filter_map(|path| {
                    let modified = root.join(&path).symlink_metadata().ok()?.modified().ok()?;
                    Some((path, modified.duration_since(UNIX_EPOCH).ok()?.as_secs()))
                })
                .collect())
        })
    } else {
        native_github::Job::spawn(move || {
            repo_world::changed_path_times(&root, baseline.as_deref(), &revision)
        })
    }
}

#[derive(Component)]
pub(super) struct FileRows;

pub(super) fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    repo: Res<Repo>,
    progress: Res<progress::Progress>,
    live: Res<live_worktree::Live>,
    menu: Res<city_menu::Menu>,
    search: Res<listing::find::Search>,
    mut picker: ResMut<file_picker::FilePicker>,
    mut tablet: ResMut<Tablet>,
    mut review: ResMut<review_notes::Review>,
    mut focus: ResMut<InputFocus>,
    (parents, entrances): (Query<&ChildOf>, Query<&native_panel::TabletEntrance>),
    wheel: Res<AccumulatedMouseScroll>,
    mut rows: Query<(&mut ScrollPosition, &RelativeCursorPosition), With<FileRows>>,
) {
    let toggle = keys.just_pressed(KeyCode::Tab)
        && !keys.any_pressed([
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]);
    let action = tablet.action.take();
    let was_open = tablet.panel.is_some();
    let mut redraw = !was_open;
    if let Some(panel) = tablet.panel {
        let same_comparison = tablet
            .scope
            .as_ref()
            .is_some_and(|(root, revision, baseline)| {
                root == &repo.0.root && revision == &repo.0.revision && baseline == &repo.0.baseline
            });
        let destination = match action {
            Some(Action::Visit(index)) if !repo.is_changed() => Some(index),
            _ => None,
        };
        if matches!(action, Some(Action::Close))
            || destination.is_some()
            || keys.just_pressed(KeyCode::Escape)
            || !same_comparison
        {
            native_panel::close(&mut commands, panel, &mut focus, &parents);
            *tablet = Tablet::default();
            picker.destination = destination;
            review.blocked |= destination.is_none();
            return;
        }
        match action {
            Some(Action::Filter(viewed)) => {
                tablet.viewed_only = viewed;
                tablet.page = 0;
                redraw = true;
            }
            Some(Action::Page(page)) => {
                tablet.page = page;
                redraw = true;
            }
            _ => {}
        }
        redraw |= repo.is_changed();
        review.blocked = true;
        review.tablet_open = true;
        if let Ok((mut scroll, relative)) = rows.single_mut()
            && relative.cursor_over()
            && wheel.delta.y != 0.
        {
            let delta = match wheel.unit {
                bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 34.,
                bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
            };
            scroll.y = (scroll.y - delta).max(0.);
        }
    } else {
        if !toggle
            || review.blocked
            || menu.is_open()
            || picker.is_open()
            || search.blocks_controls()
        {
            return;
        }
        tablet.scope = Some((
            repo.0.root.clone(),
            repo.0.revision.clone(),
            repo.0.baseline.clone(),
        ));
        tablet.pending = Some(load_times(&repo.0));
        review.blocked = true;
        review.tablet_open = true;
    }

    if let Some(result) = tablet.pending.as_ref().and_then(native_github::Job::take) {
        tablet.pending = None;
        tablet.times = result.unwrap_or_default();
        redraw = true;
    }
    if !redraw {
        return;
    }
    let entrance = if let Some(panel) = tablet.panel {
        entrances.get(panel).ok().copied()
    } else {
        Some(native_panel::TabletEntrance::default())
    };
    if let Some(panel) = tablet.panel {
        native_panel::close(&mut commands, panel, &mut focus, &parents);
    }
    let files = changed_files(&repo.0, &progress, &live, &tablet.times, tablet.viewed_only);
    tablet.page = tablet
        .page
        .min(files.len().div_ceil(FILES_PER_PAGE).saturating_sub(1));
    let (panel, first) = draw(&mut commands, &tablet, &files, time.elapsed_secs_f64());
    tablet.panel = Some(panel);
    if let Some(entrance) = entrance {
        commands.entity(panel).insert((entrance, entrance.pose()));
    }
    focus.set(first, FocusCause::Navigated);
}

fn draw(
    commands: &mut Commands,
    tablet: &Tablet,
    files: &[(usize, &repo_world::File, Freshness)],
    elapsed: f64,
) -> (Entity, Entity) {
    let panel = native_panel::tablet(commands, "Changed files");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let first_row = tablet.page * FILES_PER_PAGE;
    let mut tab = 0;
    let mut first = None;
    commands.entity(panel).with_children(|parent| {
        parent
            .spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6.),
                ..default()
            },))
            .with_children(|bar| {
                for (label, viewed) in [("All changes", false), ("Viewed", true)] {
                    let entity = add_button(bar, label, Action::Filter(viewed), &mut tab);
                    if viewed == tablet.viewed_only {
                        bar.commands()
                            .entity(entity)
                            .insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                    }
                }
            });
        parent
            .spawn((
                FileRows,
                ScrollPosition::default(),
                RelativeCursorPosition::default(),
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.,
                    min_height: px(0.),
                    row_gap: px(4.),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                if tablet.pending.is_some() {
                    native_panel::text(list, "Loading file dates…", native_panel::INK);
                } else if files.is_empty() {
                    let message = if tablet.viewed_only {
                        "No changed files viewed."
                    } else {
                        "No changed files."
                    };
                    native_panel::text(list, message, native_panel::INK);
                } else {
                    for &(index, file, freshness) in
                        files.iter().skip(first_row).take(FILES_PER_PAGE)
                    {
                        let path = format!("{}\n{}", file.path, freshness.label(elapsed, now));
                        let row = add_button(list, path.trim_end(), Action::Visit(index), &mut tab);
                        first.get_or_insert(row);
                    }
                }
            });
        parent
            .spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.),
                ..default()
            })
            .with_children(|bar| {
                let close = add_button(bar, "Close", Action::Close, &mut tab);
                first.get_or_insert(close);
                if tablet.pending.is_none() && files.len() > FILES_PER_PAGE {
                    let pages = files.len().div_ceil(FILES_PER_PAGE);
                    if tablet.page > 0 {
                        add_button(bar, "Previous", Action::Page(tablet.page - 1), &mut tab);
                    }
                    native_panel::text(
                        bar,
                        &format!("{} / {pages}", tablet.page + 1),
                        native_panel::INK,
                    );
                    if tablet.page + 1 < pages {
                        add_button(bar, "Next", Action::Page(tablet.page + 1), &mut tab);
                    }
                }
            });
    });
    (panel, first.expect("the tablet always has a close button"))
}

fn add_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: Action,
    tab: &mut i32,
) -> Entity {
    let entity = native_panel::button::<_, Tablet>(parent, label, native_panel::INK, action);
    parent.commands().entity(entity).insert(TabIndex(*tab));
    *tab += 1;
    entity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tablet_sorts_changed_files_by_time_and_can_show_only_viewed() {
        let mut repo = repository_picker::empty();
        repo.files = [
            ("seen.rs", Change::Modified),
            ("unseen.rs", Change::Added),
            ("context.rs", Change::Unchanged),
        ]
        .into_iter()
        .map(|(path, change)| repo_world::File {
            path: path.into(),
            before: None,
            after: None,
            change,
            position: [0., 0.],
        })
        .collect();
        let mut progress = progress::Progress::default();
        progress.visit(0, &repo.files[0], false);
        progress.visit(2, &repo.files[2], false);
        let times = FileTimes::from([("seen.rs".into(), 10), ("unseen.rs".into(), 20)]);
        let live = live_worktree::Live::default();
        let paths = |viewed| {
            changed_files(&repo, &progress, &live, &times, viewed)
                .into_iter()
                .map(|(_, file, _)| file.path.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(false), ["unseen.rs", "seen.rs"]);
        assert_eq!(paths(true), ["seen.rs"]);
    }
}
