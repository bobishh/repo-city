//! Browse changed files and return to their buildings.
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    input_focus::{FocusCause, InputFocus, tab_navigation::TabIndex},
    text::EditableText,
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
    view: Page,
    dates_requested: bool,
    bounds: [String; 2],
    inputs: Option<[Entity; 2]>,
    mode: reading::Mode,
    status: String,
    document: Option<Document>,
}

#[derive(Default)]
struct Document {
    path: String,
    pending: Option<native_github::Job<(String, markdown_preview::ReadDocument)>>,
    result: Option<Result<(String, markdown_preview::ReadDocument), String>>,
    content: Option<Entity>,
    finished: bool,
}

impl Document {
    fn open(repository: &Repository, index: usize, before: bool) -> Option<Self> {
        let file = repository.files.get(index)?.clone();
        let path = file.path.clone();
        let file_snapshot = Repository {
            root: repository.root.clone(),
            revision: repository.revision.clone(),
            baseline: repository.baseline.clone(),
            files: vec![file],
            districts: Vec::new(),
        };
        let read_path = path.clone();
        Some(Self {
            path,
            pending: Some(native_github::Job::spawn(move || {
                let source = file_snapshot.source(0, before)?;
                let file = &file_snapshot.files[0];
                let blob = if before { &file.before } else { &file.after };
                let blob = blob
                    .as_ref()
                    .ok_or("File does not exist in this snapshot")?
                    .id
                    .clone();
                let read = markdown_preview::ReadDocument {
                    path: read_path,
                    before,
                    blob,
                    rows: source.split_inclusive('\n').count(),
                };
                Ok((markdown_preview::markdown_html(&source), read))
            })),
            ..default()
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Files,
    Help,
    Settings,
    Document,
}

impl Page {
    pub(super) const NAVIGATION: [(Self, &'static str); 3] = [
        (Self::Files, "Files"),
        (Self::Help, "Help"),
        (Self::Settings, "Settings"),
    ];
}

#[derive(Component, Clone, Copy)]
enum Action {
    Visit(usize),
    Filter(bool),
    Page(usize),
    Switch(Page),
    Mode(reading::Mode),
    Save,
    StartTour,
    Close,
}

impl native_panel::Actions<Action> for Tablet {
    fn activate(&mut self, action: Action) {
        self.action = Some(action);
    }
}

impl Tablet {
    pub(super) fn open_help(&mut self) {
        self.open_page(Page::Help);
    }

    pub(super) fn open_page(&mut self, page: Page) {
        self.action = Some(Action::Switch(page));
    }

    pub(super) fn is_open(&self) -> bool {
        self.panel.is_some()
    }

    fn switch(&mut self, view: Page, reading: &reading::Settings) {
        if view != self.view && view == Page::Settings {
            self.bounds = [reading.min_seconds, reading.max_seconds]
                .map(|value| value.map_or_else(String::new, |value| value.to_string()));
            self.mode = reading.mode;
            self.status.clear();
        }
        self.view = view;
    }
}

pub(super) fn spawn_help(commands: &mut Commands, font: &TextFont, ink: Color) {
    use bevy::ui_widgets::{Activate, observe};
    commands
        .spawn((
            GlobalZIndex(35),
            Node {
                position_type: PositionType::Absolute,
                left: px(24.),
                bottom: px(22.),
                ..default()
            },
        ))
        .with_children(|controls| {
            controls.spawn((
                file_picker::button("H", font, ink, AlignSelf::Auto),
                observe(|_: On<Activate>, mut tablet: ResMut<Tablet>| tablet.open_help()),
            ));
        });
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

pub(super) fn plain_tab(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::Tab) && unmodified(keys)
}

fn unmodified(keys: &ButtonInput<KeyCode>) -> bool {
    !keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ])
}

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
    parents: Query<&ChildOf>,
    wheel: Res<AccumulatedMouseScroll>,
    mut rows: Query<(&mut ScrollPosition, &ComputedNode), With<FileRows>>,
    (
        mut reading,
        mut tour,
        repository,
        fields,
        mut pilot,
        mut cursor,
        mut markdown,
        mut read,
        layout,
    ): (
        ResMut<reading::Settings>,
        ResMut<guide::Tour>,
        Res<repository_picker::RepositoryPicker>,
        Query<&EditableText>,
        Query<&mut Pilot, PlayerCamera>,
        Query<&mut CursorOptions, With<PrimaryWindow>>,
        Option<ResMut<markdown_preview::Request>>,
        Option<ResMut<markdown_preview::ReadQueue>>,
        Query<&ComputedNode, With<bevy_markup::prelude::HtmlUi>>,
    ),
) {
    let editing = focus.get().is_some_and(|entity| fields.contains(entity));
    let help = keys.just_pressed(KeyCode::KeyH) && unmodified(&keys) && !editing;
    let action = tablet.action.take().or_else(|| {
        help.then_some(if tablet.view == Page::Help {
            Action::Close
        } else {
            Action::Switch(Page::Help)
        })
    });
    let markdown = markdown.as_deref_mut().and_then(|request| request.0.take());
    let was_open = tablet.panel.is_some();
    let mut redraw = !was_open;
    if let Some(panel) = tablet.panel {
        let same_comparison = tablet
            .scope
            .as_ref()
            .is_some_and(|(root, revision, baseline)| {
                root == &repo.0.root && revision == &repo.0.revision && baseline == &repo.0.baseline
            });
        if same_comparison
            && tablet.view == Page::Document
            && let Some(document) = tablet
                .document
                .as_mut()
                .filter(|document| !document.finished)
            && let Some(content) = document.content.and_then(|entity| layout.get(entity).ok())
            && content.size().y > 0.
            && let Some(Ok((_, mark))) = &document.result
            && let Ok((scroll, viewport)) = rows.single()
            && viewport.size().y > 0.
            && scroll.y * viewport.inverse_scale_factor().recip() + viewport.size().y + 2.
                >= viewport.content_size().y
        {
            document.finished = true;
            if let Some(queue) = &mut read {
                queue.0 = Some(mark.clone());
            }
        }
        let destination = match action {
            Some(Action::Visit(index)) if !repo.is_changed() => Some(index),
            _ => None,
        };
        if matches!(action, Some(Action::StartTour)) {
            review.select(0);
            tour.start();
        }
        if matches!(action, Some(Action::Close | Action::StartTour))
            || destination.is_some()
            || keys.just_pressed(KeyCode::Escape)
            || plain_tab(&keys)
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
            Some(Action::Switch(page)) if page != tablet.view => {
                tablet.switch(page, &reading);
                redraw = true;
            }
            Some(Action::Mode(mode)) => {
                tablet.capture_bounds(&fields);
                tablet.mode = mode;
                tablet.status.clear();
                redraw = true;
            }
            Some(Action::Save) => {
                tablet.capture_bounds(&fields);
                tablet.status = match reading::Settings::with_bounds(
                    tablet.mode,
                    &tablet.bounds[0],
                    &tablet.bounds[1],
                )
                .and_then(|next| {
                    next.save()?;
                    Ok(next)
                }) {
                    Ok(next) => {
                        *reading = next;
                        "Saved.".into()
                    }
                    Err(error) => error,
                };
                redraw = true;
            }
            _ => {}
        }
        redraw |= tablet.view == Page::Files && repo.is_changed();
        review.blocked = true;
        review.tablet_open = true;
        if let Ok((mut scroll, viewport)) = rows.single_mut() {
            let delta = match wheel.unit {
                bevy::input::mouse::MouseScrollUnit::Line => wheel.delta.y * 34.,
                bevy::input::mouse::MouseScrollUnit::Pixel => wheel.delta.y,
            };
            scroll.y = (scroll.y - delta).max(0.);
            if !editing {
                let page = viewport.size().y * viewport.inverse_scale_factor() * 0.9;
                if keys.just_pressed(KeyCode::PageDown) {
                    scroll.y += page;
                }
                if keys.just_pressed(KeyCode::PageUp) {
                    scroll.y = (scroll.y - page).max(0.);
                }
                if keys.just_pressed(KeyCode::End) {
                    scroll.y = viewport.content_size().y * viewport.inverse_scale_factor();
                }
                if keys.just_pressed(KeyCode::Home) {
                    scroll.y = 0.;
                }
            }
        }
    } else {
        let page = match action {
            Some(Action::Switch(page)) => page,
            _ if markdown.is_some() => Page::Document,
            _ if plain_tab(&keys) => Page::Files,
            _ => return,
        };
        if editing
            || review.blocked
            || menu.is_open()
            || picker.is_open()
            || search.blocks_controls()
            || repository.blocks_controls()
        {
            return;
        }
        tablet.scope = Some((
            repo.0.root.clone(),
            repo.0.revision.clone(),
            repo.0.baseline.clone(),
        ));
        tablet.switch(page, &reading);
        if page == Page::Document
            && let Some((index, before)) = markdown
        {
            tablet.document = Document::open(&repo.0, index, before);
        }
        review.blocked = true;
        review.tablet_open = true;
        review.drag = None;
        if let Ok(mut pilot) = pilot.single_mut() {
            pilot.captured = false;
            pilot.fire_ready = false;
            pilot.firing = false;
            pilot.shot = false;
        }
        if let Ok(mut cursor) = cursor.single_mut() {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    if tablet.view == Page::Files && !tablet.dates_requested {
        tablet.dates_requested = true;
        tablet.pending = Some(load_times(&repo.0));
    }
    if let Some(result) = tablet.pending.as_ref().and_then(native_github::Job::take) {
        tablet.pending = None;
        tablet.times = result.unwrap_or_default();
        redraw |= tablet.view == Page::Files;
    }
    if let Some(document) = tablet.document.as_mut()
        && let Some(result) = document.pending.as_ref().and_then(native_github::Job::take)
    {
        document.pending = None;
        document.result = Some(result);
        redraw |= tablet.view == Page::Document;
    }
    if !redraw {
        return;
    }
    if let Some(panel) = tablet.panel {
        native_panel::close(&mut commands, panel, &mut focus, &parents);
    }
    let files = if tablet.view == Page::Files {
        changed_files(&repo.0, &progress, &live, &tablet.times, tablet.viewed_only)
    } else {
        Vec::new()
    };
    tablet.page = tablet
        .page
        .min(files.len().div_ceil(FILES_PER_PAGE).saturating_sub(1));
    let (panel, first) = draw(&mut commands, &mut tablet, &files, time.elapsed_secs_f64());
    tablet.panel = Some(panel);
    focus.set(first, FocusCause::Navigated);
}

impl Tablet {
    fn capture_bounds(&mut self, fields: &Query<&EditableText>) {
        if let Some(inputs) = self.inputs {
            for (field, value) in inputs.into_iter().zip(&mut self.bounds) {
                if let Ok(field) = fields.get(field) {
                    *value = field.value().to_string();
                }
            }
        }
    }
}

fn draw(
    commands: &mut Commands,
    tablet: &mut Tablet,
    files: &[(usize, &repo_world::File, Freshness)],
    elapsed: f64,
) -> (Entity, Entity) {
    let title = match tablet.view {
        Page::Files => "Changed files",
        Page::Help => "Help",
        Page::Settings => "Settings",
        Page::Document => tablet
            .document
            .as_ref()
            .map_or("Markdown", |document| document.path.as_str()),
    };
    let panel = native_panel::tablet(commands, title);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let first_row = tablet.page * FILES_PER_PAGE;
    let mut tab = 0;
    let mut first = None;
    tablet.inputs = None;
    commands.entity(panel).with_children(|parent| {
        parent
            .spawn(Node {
                column_gap: px(6.),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|bar| {
                for (view, label) in Page::NAVIGATION {
                    let entity = add_button(bar, label, Action::Switch(view), &mut tab);
                    first.get_or_insert(entity);
                    if view == tablet.view {
                        bar.commands()
                            .entity(entity)
                            .insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                    }
                }
            });
        match tablet.view {
            Page::Document => {
                draw_document(parent, tablet.document.as_mut());
                return;
            }
            Page::Settings => {
                first = Some(draw_settings(parent, tablet, &mut tab));
                return;
            }
            Page::Help => {
                parent
                    .spawn((
                        FileRows,
                        ScrollPosition::default(),
                        Node {
                            min_height: px(0.),
                            flex_grow: 1.,
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(guide::draw_controls);
                parent
                    .spawn(Node {
                        column_gap: px(8.),
                        ..default()
                    })
                    .with_children(|actions| {
                        first = Some(add_button(
                            actions,
                            "Start tour",
                            Action::StartTour,
                            &mut tab,
                        ));
                    });
                return;
            }
            Page::Files => {}
        }
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
                        if index == files[first_row].0 {
                            first = Some(row);
                        }
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
    (panel, first.expect("the tablet always has navigation"))
}

fn draw_document(parent: &mut ChildSpawnerCommands, document: Option<&mut Document>) {
    parent
        .spawn((
            FileRows,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                min_height: px(0.),
                flex_grow: 1.,
                ..default()
            },
        ))
        .with_children(|body| {
            let Some(document) = document else { return };
            match document.result.as_ref() {
                Some(Ok((html, _))) if !html.trim().is_empty() => {
                    document.content = Some(markdown_preview::draw(body, html.clone()));
                }
                Some(Ok(_)) => native_panel::text(body, "Empty document.", native_panel::INK),
                Some(Err(error)) => native_panel::text(body, error, native_panel::INK),
                None => native_panel::text(body, "Loading document…", native_panel::INK),
            }
        });
}

fn draw_settings(parent: &mut ChildSpawnerCommands, tablet: &mut Tablet, tab: &mut i32) -> Entity {
    let mut first = None;
    parent.spawn(Node {
        width: percent(100.),
        max_width: px(620.),
        flex_direction: FlexDirection::Column,
        row_gap: px(8.),
        ..default()
    }).with_children(|form| {
        native_panel::text(form, "Water", native_panel::INK);
        form.spawn(Node { column_gap: px(6.), ..default() }).with_children(|row| {
            for (label, mode) in [("Delayed", reading::Mode::Delayed), ("Immediate", reading::Mode::Immediate)] {
                let entity = add_button(row, label, Action::Mode(mode), tab);
                if mode == tablet.mode {
                    row.commands().entity(entity).insert(BackgroundColor(Color::srgb_u8(23, 79, 87)));
                    first = Some(entity);
                }
            }
        });
        tablet.inputs = Some(std::array::from_fn(|index| {
            let label = ["Minimum delay (seconds)", "Maximum delay (seconds)"][index];
            let input = native_panel::input(form, label, &tablet.bounds[index], *tab, false);
            *tab += 1;
            input
        }));
        native_panel::text(form, "Leave a bound blank to remove that limit. Delayed mode estimates reading time from the amount of code using Brysbaert’s 2019 estimate of 238 words per minute for English nonfiction.", native_panel::INK);
        native_panel::text(form, if tablet.status.is_empty() { " " } else { &tablet.status }, native_panel::INK);
        form.spawn(Node { column_gap: px(8.), ..default() }).with_children(|actions| {
            add_button(actions, "Save settings", Action::Save, tab);
        });
    });
    first.expect("a water mode is selected")
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
    fn help_preserves_text_input_and_opens_in_vibepad_after_focus_leaves() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Tablet>()
            .init_resource::<guide::Tour>()
            .init_resource::<reading::Settings>()
            .init_resource::<review_notes::Review>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<live_worktree::Live>()
            .init_resource::<progress::Progress>()
            .init_resource::<listing::find::Search>()
            .init_resource::<InputFocus>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Repo(repository_picker::empty()))
            .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
            .add_systems(Update, update);

        let world = app.world_mut();
        let editor = world.spawn(EditableText::default()).id();
        let camera = world
            .spawn((
                Camera3d::default(),
                Pilot {
                    captured: true,
                    ..default()
                },
            ))
            .id();
        world
            .resource_mut::<InputFocus>()
            .set(editor, FocusCause::Navigated);
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyH);
        app.update();
        assert!(!app.world().resource::<Tablet>().is_open());
        assert!(app.world().get::<Pilot>(camera).unwrap().captured);

        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        assert!(app.world().resource::<Tablet>().is_open());
        assert!(app.world().resource::<Tablet>().view == Page::Help);
        assert!(!app.world().get::<Pilot>(camera).unwrap().captured);
        assert!(app.world().resource::<review_notes::Review>().tablet_open);

        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::KeyH);
        keys.press(KeyCode::Tab);
        app.update();
        assert!(!app.world().resource::<Tablet>().is_open());

        // In the running app the review controls clear this flag before the next frame.
        app.world_mut()
            .resource_mut::<review_notes::Review>()
            .blocked = false;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::Tab);
        keys.press(KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<Tablet>().is_open());
        assert!(app.world().resource::<Tablet>().view == Page::Files);
    }

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
