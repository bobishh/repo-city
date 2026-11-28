use bevy::{
    camera::visibility::RenderLayers,
    picking::{Pickable, events::PointerClick, pointer::PointerButton},
    text::LineBreak,
    ui::{UiTransform, Val2},
};

use super::{content::Rows, *};

pub(super) const TILE_HEADER: f32 = 52.;
pub(super) const TILE_FOOTER: f32 = 29.;

fn toolbar_height(width: f32) -> f32 {
    if width >= 1060. { 44. } else { 82. }
}
use model::GAP;
const CACHED_TILES: usize = 32;
const INK: Color = Color::srgb(0.89, 0.91, 0.94);
const MUTED: Color = Color::srgb(0.64, 0.70, 0.78);
const CHROME: Color = Color::srgb(0.10, 0.13, 0.18);
const SURFACE: Color = Color::srgb(0.067, 0.083, 0.12);
const CANVAS: Color = Color::srgb(0.033, 0.045, 0.065);

mod layout;
pub(super) use layout::Layout;

#[derive(Default)]
pub(super) struct RenderedTiles {
    surface: Option<Surface>,
    tiles: HashMap<String, Pane>,
    header: Option<Header>,
    dialog: bool,
    empty: Option<Entity>,
}

#[derive(Clone, Copy)]
struct Surface {
    body: Entity,
    board: Entity,
    header: Entity,
    overlay: Entity,
}

#[derive(PartialEq)]
struct Header {
    width: f32,
    name: String,
    zoom: f32,
    overview: bool,
    files: usize,
    has_selection: bool,
    status: String,
    #[cfg(not(target_arch = "wasm32"))]
    agent: Option<AgentLabels>,
}

struct Pane {
    entity: Entity,
    drawn: PaneKey,
}

#[derive(PartialEq)]
struct PaneKey {
    extent: Vec2,
    zoom: f32,
    overview: bool,
    content: u64,
    viewport: model::Viewport,
    positioned: bool,
    focus: bool,
    selection: Option<review_notes::Anchor>,
    change: Option<Change>,
}

pub(super) fn render(
    mut commands: Commands,
    mut canvas: ResMut<Canvas>,
    windows: Query<&Window, With<PrimaryWindow>>,
    pose: Query<&Transform, With<android::ViewTablet>>,
    mut nodes: Query<&mut Node>,
    mut transforms: Query<&mut UiTransform>,
    repo: Res<Repo>,
    mut rendered: Local<RenderedTiles>,
    #[cfg(not(target_arch = "wasm32"))] mut worker: Local<content::Worker>,
    #[cfg(not(target_arch = "wasm32"))] session: Option<Res<agent_session::Session>>,
) {
    if !canvas.is_open() || canvas.elapsed < RAISE || canvas.book.is_none() {
        if let Some(root) = canvas.root
            && let Ok(mut node) = nodes.get_mut(root)
        {
            node.display = Display::None;
        }
        return;
    }
    let Ok(window) = windows.single() else { return };
    let size = Vec2::new(window.width(), window.height());
    let t = ((canvas.elapsed - RAISE) / EXPAND).clamp(0., 1.);
    let t = t * t * (3. - 2. * t);
    let mut bounds = (Vec2::ZERO, size);
    if t < 1.
        && let Ok(pose) = pose.single()
        && let Some(corners) = android::tablet_corners(pose, size)
    {
        let min = Vec2::new(corners[0], corners[1]);
        let max = Vec2::new(corners[4], corners[5]);
        bounds = (min.lerp(Vec2::ZERO, t), max.lerp(size, t));
    }
    let extent = (bounds.1 - bounds.0).max(Vec2::ONE);
    let show_document = matches!(canvas.dialog, Dialog::Document(_));
    if canvas.document_rendered && show_document {
        if let Some(feedback) = canvas.document_feedback {
            let value = canvas.status.clone();
            commands.queue(move |world: &mut World| {
                if let Some(mut text) = world.get_mut::<Text>(feedback)
                    && text.0 != value
                {
                    text.0 = value;
                }
            });
        }
        return;
    }
    canvas.document_rendered = show_document;
    canvas.document_input = None;
    canvas.document_feedback = None;
    let mut redraw = canvas.dirty;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((scope, batch)) = worker.take() {
        redraw = true;
        if canvas
            .book
            .as_ref()
            .is_some_and(|book| book.scope() == &scope)
        {
            for (path, content) in batch {
                let file = repo
                    .0
                    .files
                    .binary_search_by(|file| file.path.cmp(&path))
                    .ok()
                    .map(|index| &repo.0.files[index]);
                if content.matches(file) {
                    canvas.contents.insert(path, content);
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let agent = session.as_deref().map(AgentLabels::new);
    #[cfg(not(target_arch = "wasm32"))]
    {
        redraw |= rendered
            .header
            .as_ref()
            .and_then(|header| header.agent.as_ref())
            != agent.as_ref();
    }
    let (root, body) = match (canvas.root, canvas.body) {
        (Some(root), Some(body)) => (root, body),
        _ => {
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: 3,
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderLayers::none(),
                ))
                .id();
            canvas.camera = Some(camera);
            let root = commands
                .spawn((
                    UiTargetCamera(camera),
                    GlobalZIndex(1_100),
                    Node {
                        position_type: PositionType::Absolute,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(CANVAS),
                ))
                .id();
            let body = commands
                .spawn((
                    ChildOf(root),
                    Node {
                        width: px(size.x),
                        height: px(size.y),
                        flex_shrink: 0.,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .id();
            canvas.root = Some(root);
            canvas.body = Some(body);
            redraw = true;
            (root, body)
        }
    };
    let update_bounds = move |node: &mut Node| {
        node.display = Display::Flex;
        node.left = px(bounds.0.x);
        node.top = px(bounds.0.y);
        node.width = px(extent.x);
        node.height = px(extent.y);
    };
    if let Ok(mut node) = nodes.get_mut(root) {
        if node.display != Display::Flex
            || node.left != px(bounds.0.x)
            || node.top != px(bounds.0.y)
            || node.width != px(extent.x)
            || node.height != px(extent.y)
        {
            update_bounds(&mut node);
        }
    } else {
        commands.queue(move |world: &mut World| {
            if let Some(mut node) = world.get_mut::<Node>(root) {
                update_bounds(&mut node);
            }
        });
    }
    let pan_dirty = std::mem::take(&mut canvas.pan_dirty);
    if !redraw && !pan_dirty {
        return;
    }
    if let Ok(mut node) = nodes.get_mut(body)
        && (node.width != px(size.x) || node.height != px(size.y))
    {
        node.width = px(size.x);
        node.height = px(size.y);
    }
    canvas.dirty = false;
    let layout = Layout::for_view(size, canvas.model(), canvas.overview);
    let visible = canvas
        .model()
        .tiles
        .iter()
        .enumerate()
        .filter(|(index, _)| layout.visible(&canvas.model().tiles, *index))
        .map(|(index, tile)| (index, tile.path.clone()))
        .collect::<Vec<_>>();
    if canvas.contents.len() > CACHED_TILES {
        let visible_paths = visible
            .iter()
            .map(|(_, path)| path.as_str())
            .collect::<HashSet<_>>();
        let focus = canvas.focus.clone();
        canvas.contents.retain(|path, _| {
            visible_paths.contains(path.as_str()) || focus.as_ref() == Some(path)
        });
    }
    let previews = if canvas.overview {
        visible
            .iter()
            .filter(|(index, _)| {
                let (_, extent) = layout.rect(&canvas.model().tiles, *index);
                extent.x >= 200. && extent.y >= 110.
            })
            .cloned()
            .collect::<Vec<_>>()
    } else if layout.zoom >= 0.42 {
        visible.clone()
    } else {
        Vec::new()
    };
    #[cfg(not(target_arch = "wasm32"))]
    worker.schedule(&canvas, &repo.0, &previews);
    #[cfg(target_arch = "wasm32")]
    for (_, path) in &previews {
        canvas
            .contents
            .entry(path.clone())
            .or_default()
            .refresh(&repo.0, path);
    }
    if let Some(anchor) = canvas.pending_reveal.as_ref()
        && let Some(content) = canvas.contents.get(&anchor.path)
        && !content.pending()
        && canvas
            .model()
            .tiles
            .iter()
            .find(|tile| tile.path == anchor.path)
            .is_some_and(|tile| content.matches(tile.file(&repo.0)))
    {
        let row = content.row_at_anchor(anchor.line, anchor.before);
        let path = canvas.pending_reveal.take().unwrap().path;
        canvas.contents.get_mut(&path).unwrap().expose(row);
        if let Some(tile) = canvas
            .model_mut()
            .tiles
            .iter_mut()
            .find(|tile| tile.path == path)
        {
            tile.viewport.row = row;
            tile.positioned = true;
            canvas.edit();
            canvas.dirty = false;
        }
    }
    let surface = if let Some(surface) = rendered.surface.filter(|surface| surface.body == body) {
        surface
    } else {
        let header = commands.spawn((ChildOf(body), Node::default())).id();
        let board = commands
            .spawn((
                ChildOf(body),
                Node {
                    width: percent(100.),
                    height: px(layout.viewport.y),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .id();
        let overlay = commands
            .spawn((
                ChildOf(body),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100.),
                    height: percent(100.),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        let surface = Surface {
            body,
            header,
            board,
            overlay,
        };
        *rendered = RenderedTiles {
            surface: Some(surface),
            ..default()
        };
        surface
    };
    if let Ok(mut board) = nodes.get_mut(surface.board)
        && board.height != px(layout.viewport.y)
    {
        board.height = px(layout.viewport.y);
    }
    if redraw || rendered.header.is_none() {
        let header = Header {
            width: size.x,
            name: canvas.model().name.clone(),
            zoom: layout.zoom,
            overview: canvas.overview,
            files: canvas.model().tiles.len(),
            has_selection: canvas.selection.is_some(),
            status: canvas.status.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            agent,
        };
        if rendered.header.as_ref() != Some(&header) {
            commands
                .entity(surface.header)
                .despawn_children()
                .with_children(|parent| {
                    toolbar(
                        parent,
                        &canvas,
                        &layout,
                        #[cfg(not(target_arch = "wasm32"))]
                        header.agent.as_ref(),
                    );
                });
            rendered.header = Some(header);
        }
    }
    let has_dialog = !matches!(canvas.dialog, Dialog::None);
    if (redraw && has_dialog) || rendered.dialog != has_dialog {
        commands
            .entity(surface.overlay)
            .despawn_children()
            .with_children(|parent| {
                if let Dialog::Document(document) = &canvas.dialog {
                    let (field, feedback) = document_panel(parent, document, &repo.0, size);
                    canvas.document_input = field;
                    canvas.document_feedback = feedback;
                } else if has_dialog {
                    dialog(parent, &canvas, &repo.0, size);
                }
            });
        rendered.dialog = has_dialog;
    }
    if canvas.model().tiles.is_empty() && rendered.empty.is_none() {
        let empty = commands
            .spawn((
                ChildOf(surface.board),
                label("No files on this canvas", 15., MUTED),
                Node {
                    margin: UiRect::all(px(18.)),
                    ..default()
                },
            ))
            .id();
        rendered.empty = Some(empty);
    } else if !canvas.model().tiles.is_empty()
        && let Some(empty) = rendered.empty.take()
    {
        commands.entity(empty).despawn();
    }
    sync_panes(
        &mut commands,
        &mut transforms,
        &mut rendered.tiles,
        surface.board,
        &canvas,
        &layout,
        &repo.0,
        &visible,
    );
    #[cfg(target_arch = "wasm32")]
    if geometry::inspection_enabled() {
        publish_view(&canvas, &layout);
    }
}

fn sync_panes(
    commands: &mut Commands,
    transforms: &mut Query<&mut UiTransform>,
    panes: &mut HashMap<String, Pane>,
    board: Entity,
    canvas: &Canvas,
    layout: &Layout,
    repo: &Repository,
    visible: &[(usize, String)],
) {
    let visible_paths: HashSet<_> = visible.iter().map(|(_, path)| path.as_str()).collect();
    panes.retain(|path, pane| {
        let keep = visible_paths.contains(path.as_str());
        if !keep {
            commands.entity(pane.entity).despawn();
        }
        keep
    });
    commands.entity(board).with_children(|parent| {
        for (index, path) in visible {
            let tile = &canvas.model().tiles[*index];
            let file = tile.file(repo);
            let content = canvas
                .contents
                .get(path)
                .filter(|content| content.matches(file));
            let (point, extent) = layout.rect(&canvas.model().tiles, *index);
            let change = file.map(|file| &file.change);
            let drawn = PaneKey {
                extent,
                zoom: layout.zoom,
                overview: canvas.overview,
                content: content.map_or(0, |content| content.revision),
                viewport: tile.viewport,
                positioned: tile.positioned,
                focus: canvas.focus.as_ref() == Some(path),
                selection: canvas
                    .selection
                    .as_ref()
                    .filter(|selection| selection.anchor.path == *path)
                    .map(|s| s.anchor.clone()),
                change: change.cloned(),
            };
            if let Some(existing) = panes.get(path) {
                if existing.drawn == drawn {
                    if let Ok(mut transform) = transforms.get_mut(existing.entity) {
                        let translation = Val2::new(px(point.x), px(point.y));
                        if transform.translation != translation {
                            transform.translation = translation;
                        }
                    }
                    continue;
                }
                parent.commands().entity(existing.entity).despawn();
            }
            let entity = panel(parent, canvas, layout, *index, tile, content, change);
            panes.insert(path.clone(), Pane { entity, drawn });
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn publish_view(canvas: &Canvas, layout: &Layout) {
    if let Some(document) = web_sys::window().and_then(|window| window.document())
        && let Some(element) = document.get_element_by_id("repo-city")
    {
        let paths = canvas
            .model()
            .tiles
            .iter()
            .map(|tile| tile.path.as_str())
            .collect::<Vec<_>>();
        let _ = element.set_attribute("data-diff-canvas", &paths.join("\n"));
        let _ = element.set_attribute(
            "data-diff-canvas-files",
            &serde_json::to_string(&paths).unwrap_or_default(),
        );
        let _ = element.set_attribute("data-diff-canvas-status", &canvas.status);
        let _ = element.toggle_attribute_with_force(
            "data-diff-canvas-editor",
            matches!(canvas.dialog, Dialog::Document(_)),
        );
        let _ = element.set_attribute(
            "data-diff-canvas-layout",
            &serde_json::json!({
                "pan": layout.pan.to_array(),
                "zoom": layout.zoom,
                "overview": canvas.overview,
                "viewport": layout.viewport.to_array(),
                "tiles": (0..paths.len()).map(|index| {
                    let (point, extent) = layout.rect(&canvas.model().tiles, index);
                    [point.x, point.y + layout.top, extent.x, extent.y]
                }).collect::<Vec<_>>(),
            })
            .to_string(),
        );
    }
}

fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: bevy::text::FontSource::list([Handle::<Font>::default(), MATH_FONT, EMOJI_FONT]),
            ..TextFont::from_font_size(size)
        },
        TextColor(color),
        TextLayout::linebreak(LineBreak::NoWrap),
        Pickable::IGNORE,
    )
}

fn container(direction: FlexDirection, gap: f32) -> Node {
    Node {
        flex_direction: direction,
        column_gap: px(gap),
        row_gap: px(gap),
        align_items: if direction == FlexDirection::Row {
            AlignItems::Center
        } else {
            AlignItems::Stretch
        },
        ..default()
    }
}

fn button(parent: &mut ChildSpawnerCommands, text: impl Into<String>, action: Action) {
    parent
        .spawn((
            bevy::ui_widgets::Button,
            Node {
                min_height: px(28.),
                padding: UiRect::axes(px(7.), px(3.)),
                ..default()
            },
            BackgroundColor(CHROME),
            action,
        ))
        .observe(clicked)
        .with_child(label(text, 13., INK));
}

fn clicked(mut event: On<PointerClick>, actions: Query<&Action>, mut canvas: ResMut<Canvas>) {
    if event.button == PointerButton::Primary
        && canvas.target
        && canvas.elapsed >= FULL
        && let Ok(action) = actions.get(event.entity)
    {
        event.propagate(false);
        canvas.actions.push(action.clone());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(PartialEq, Eq)]
pub(super) struct AgentLabels {
    voice: Option<&'static str>,
    follow: &'static str,
}

#[cfg(not(target_arch = "wasm32"))]
impl AgentLabels {
    fn new(session: &agent_session::Session) -> Self {
        Self {
            voice: session.canvas_voice_label(),
            follow: match (session.follow_key(), session.follows_agent()) {
                (KeyCode::F8, true) => "F8 Follow on",
                (KeyCode::F8, false) => "F8 Follow off",
                (_, true) => "F7 Follow on",
                (_, false) => "F7 Follow off",
            },
        }
    }
}

fn toolbar(
    parent: &mut ChildSpawnerCommands,
    canvas: &Canvas,
    layout: &Layout,
    #[cfg(not(target_arch = "wasm32"))] agent: Option<&AgentLabels>,
) {
    let wide = layout.viewport.x >= 1060.;
    let direction = if wide {
        FlexDirection::Row
    } else {
        FlexDirection::Column
    };
    parent
        .spawn((
            Node {
                width: percent(100.),
                height: px(layout.top),
                flex_shrink: 0.,
                padding: UiRect::axes(px(GAP), px(7.)),
                overflow: Overflow::clip(),
                ..container(direction, 5.)
            },
            BackgroundColor(SURFACE),
        ))
        .with_children(|parent| {
            parent
                .spawn(container(FlexDirection::Row, 6.))
                .with_children(|parent| {
                    button(parent, "Close", Action::Close);
                    button(parent, "Canvases", Action::Canvases);
                    button(parent, "Add files", Action::Choose);
                    parent
                        .spawn((
                            Node {
                                width: px(if wide {
                                    100.
                                } else {
                                    (layout.viewport.x - 238.).max(40.)
                                }),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_child(label(canvas.model().name.clone(), 15., INK));
                });
            parent
                .spawn(container(FlexDirection::Row, 5.))
                .with_children(|parent| {
                    button(
                        parent,
                        if canvas.overview {
                            "Reading view"
                        } else {
                            "Show all"
                        },
                        Action::Overview,
                    );
                    parent.spawn(label(format!("{:.0}%", layout.zoom * 100.), 12., MUTED));
                    if canvas.selection.is_some() && !canvas.overview {
                        button(parent, "Comment", Action::Comment);
                        button(parent, "Edit lines", Action::EditLines);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(agent) = agent {
                        if let Some(voice) = agent.voice {
                            button(parent, voice, Action::AgentVoice);
                        }
                        button(parent, agent.follow, Action::AgentFollow);
                    }
                    if wide || !canvas.status.is_empty() {
                        parent.spawn(label(
                            if canvas.status.is_empty() {
                                format!("{} files", canvas.model().tiles.len())
                            } else {
                                canvas.status.clone()
                            },
                            12.,
                            MUTED,
                        ));
                    }
                });
        });
}

fn panel(
    parent: &mut ChildSpawnerCommands,
    canvas: &Canvas,
    layout: &Layout,
    index: usize,
    tile: &model::Tile,
    content: Option<&Content>,
    change: Option<&Change>,
) -> Entity {
    let (point, extent) = layout.rect(&canvas.model().tiles, index);
    let zoom = layout.zoom;
    let selected = canvas.focus.as_deref() == Some(tile.path.as_str());
    if canvas.overview || zoom < 0.42 || extent.y < TILE_HEADER + TILE_FOOTER + 18. {
        let (folder, name) = tile.path.rsplit_once('/').unwrap_or(("", &tile.path));
        let directory = folder.rsplit('/').next().unwrap_or(folder);
        let (accent, kind) = match change {
            Some(Change::Added) => (Color::srgb(0.34, 0.75, 0.56), "Added"),
            Some(Change::Removed) => (Color::srgb(0.88, 0.49, 0.57), "Removed"),
            Some(Change::Unchanged) => (MUTED, "Source"),
            Some(_) => (Color::srgb(0.48, 0.68, 0.92), "Changed"),
            None => (MUTED, "Unavailable"),
        };
        return parent
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0.),
                    top: px(0.),
                    width: px(extent.x),
                    height: px(extent.y),
                    overflow: Overflow::clip(),
                    padding: UiRect::all(px((8. * zoom).min(10.))),
                    border: UiRect::left(px((3. * zoom).max(1.))),
                    ..container(FlexDirection::Column, 3.)
                },
                UiTransform::from_xy(px(point.x), px(point.y)),
                BackgroundColor(if selected {
                    Color::srgb(0.19, 0.32, 0.49)
                } else {
                    CHROME
                }),
                BorderColor::all(accent),
                Action::Tile(tile.path.clone()),
            ))
            .observe(clicked)
            .with_children(|card| {
                card.spawn(label(name, (16. * zoom).clamp(9., 18.), INK));
                if extent.y >= 40. {
                    let detail = if directory.is_empty() {
                        kind.to_owned()
                    } else {
                        format!("{kind} · {directory}")
                    };
                    card.spawn(label(detail, (11. * zoom).clamp(6., 13.), accent));
                }
                if extent.x >= 200. && extent.y >= 110. {
                    if let Some(content) = content {
                        let count = ((extent.y - 44.) / (17. * zoom)).floor().max(1.) as usize;
                        let start = content.first_visible(tile);
                        for line in content.lines(start, count.min(40)) {
                            row(card, line, 0, zoom, false);
                        }
                        if let Rows::Error(error) = &content.rows {
                            card.spawn(label(error, (12. * zoom).clamp(7., 13.), MUTED));
                        }
                    } else {
                        card.spawn(label("Loading source…", (12. * zoom).clamp(7., 13.), MUTED));
                    }
                }
            })
            .id();
    }
    let empty = Content::default();
    let content = content.unwrap_or(&empty);
    let top = content.first_visible(tile);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.),
                top: px(0.),
                width: px(extent.x),
                height: px(extent.y),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1.)),
                ..container(FlexDirection::Column, 0.)
            },
            UiTransform::from_xy(px(point.x), px(point.y)),
            BorderColor::all(if selected {
                Color::srgb(0.45, 0.70, 0.99)
            } else {
                CHROME
            }),
            BackgroundColor(SURFACE),
            Action::Tile(tile.path.clone()),
        ))
        .observe(clicked)
        .with_children(|parent| {
            let mode = match &content.rows {
                Rows::Diff(diff) if diff.final_newline_changed => {
                    format!("Diff · {} hunks · final newline changed", diff.hunks.len())
                }
                Rows::Diff(diff) => format!("Diff · {} hunks", diff.hunks.len()),
                Rows::Source(_) => "Source".into(),
                _ => "Source unavailable".into(),
            };
            parent
                .spawn((
                    Node {
                        width: percent(100.),
                        height: px(TILE_HEADER),
                        flex_shrink: 0.,
                        padding: UiRect::axes(px(8.), px(6.)),
                        overflow: Overflow::clip(),
                        ..container(FlexDirection::Column, 2.)
                    },
                    BackgroundColor(CHROME),
                ))
                .with_children(|parent| {
                    parent.spawn(label(tile.path.clone(), (14. * zoom).max(11.), INK));
                    parent.spawn(label(
                        format!(
                            "{mode} · {}/{}",
                            if content.len() == 0 { 0 } else { top + 1 },
                            content.len()
                        ),
                        11.,
                        MUTED,
                    ));
                });
            parent
                .spawn(Node {
                    width: percent(100.),
                    flex_grow: 1.,
                    min_height: px(0.),
                    overflow: Overflow::clip(),
                    ..container(FlexDirection::Column, 0.)
                })
                .with_children(|parent| {
                    let count = ((extent.y - TILE_HEADER - TILE_FOOTER - 2.) / (17. * zoom))
                        .ceil()
                        .max(1.) as usize;
                    let lines = content.visible_rows(top, count);
                    if lines.is_empty() {
                        let status = match &content.rows {
                            Rows::Diff(_) => "No text changes",
                            Rows::Source(_) => "Empty source",
                            Rows::Error(error) => error,
                            Rows::Missing if change.is_none() => {
                                "File unavailable in this snapshot"
                            }
                            Rows::Missing => "Loading source…",
                        };
                        parent.spawn(label(status, 13., MUTED));
                    }
                    for display in lines {
                        let line = match display {
                            content::Visible::Code(line) => line,
                            content::Visible::Gap(range) => {
                                context_button(parent, &tile.path, range, zoom);
                                continue;
                            }
                        };
                        let selected = canvas.selection.as_ref().is_some_and(|selection| {
                            let anchor = &selection.anchor;
                            anchor.path == tile.path
                                && anchor.before == line.before
                                && (anchor.start..=anchor.end).contains(&(line.line + 1))
                        });
                        row(parent, line, tile.viewport.column, zoom, selected);
                    }
                });
            parent
                .spawn(Node {
                    height: px(TILE_FOOTER),
                    flex_shrink: 0.,
                    overflow: Overflow::clip(),
                    ..container(FlexDirection::Row, 2.)
                })
                .with_children(|parent| {
                    if matches!(&content.rows, Rows::Diff(diff) if diff.hunks.len() > 1) {
                        button(parent, "‹ hunk", Action::Hunk(tile.path.clone(), false));
                        button(parent, "hunk ›", Action::Hunk(tile.path.clone(), true));
                    }
                });
            for position in [
                Node {
                    right: px(0.),
                    top: percent(50.),
                    width: px(3.),
                    height: px(24.),
                    ..default()
                },
                Node {
                    left: percent(50.),
                    bottom: px(0.),
                    width: px(24.),
                    height: px(3.),
                    ..default()
                },
            ] {
                parent.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        ..position
                    },
                    BackgroundColor(MUTED.with_alpha(0.55)),
                    Pickable::IGNORE,
                ));
            }
        })
        .id()
}

fn context_button(
    parent: &mut ChildSpawnerCommands,
    path: &str,
    range: std::ops::Range<usize>,
    zoom: f32,
) {
    let caption = format!("Show {} unchanged lines", range.len());
    parent
        .spawn((
            bevy::ui_widgets::Button,
            Node {
                width: percent(100.),
                height: px(17. * zoom),
                min_height: px(17. * zoom),
                padding: UiRect::horizontal(px(6.)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(CHROME),
            Action::ExpandContext(path.into(), range),
        ))
        .observe(clicked)
        .with_child(label(caption, 12. * zoom, MUTED));
}

fn row(
    parent: &mut ChildSpawnerCommands,
    line: content::Line<'_>,
    column: usize,
    zoom: f32,
    selected: bool,
) {
    let (background, ink) = match line.mark {
        '+' => (Color::srgb(0.08, 0.20, 0.16), Color::srgb(0.72, 0.94, 0.80)),
        '-' => (Color::srgb(0.24, 0.10, 0.15), Color::srgb(1., 0.78, 0.83)),
        _ => (SURFACE, INK),
    };
    let background = if selected {
        Color::srgb(0.19, 0.32, 0.49)
    } else {
        background
    };
    let number = line.line + 1;
    let source = line.text;
    let slice = source
        .chars()
        .skip(column)
        .take(360)
        .collect::<String>()
        .replace('\t', "    ");
    parent
        .spawn((
            Node {
                width: percent(100.),
                height: px(17. * zoom),
                min_height: px(17. * zoom),
                padding: UiRect::horizontal(px(6.)),
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
            BackgroundColor(background),
        ))
        .with_child(label(
            format!("{number:>5} {}  {slice}", line.mark),
            13. * zoom,
            ink,
        ));
}

fn document_panel(
    parent: &mut ChildSpawnerCommands,
    document: &Document,
    repo: &Repository,
    size: Vec2,
) -> (Option<Entity>, Option<Entity>) {
    use bevy::input_focus::{FocusCause, InputFocus};
    let mut field = None;
    let mut feedback = None;
    let mut modal = parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.),
            height: percent(100.),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(CANVAS.with_alpha(0.77)),
        GlobalZIndex(1_150),
    ));
    #[cfg(target_arch = "wasm32")]
    modal.insert(geometry::DialogControls);
    modal.with_children(|overlay| {
        overlay
            .spawn((
                Node {
                    width: px((size.x - 24.).min(1050.)),
                    height: px(if matches!(document, Document::Edit(..)) {
                        (size.y - 32.).min(650.)
                    } else {
                        (size.y - 24.).min(250.)
                    }),
                    padding: UiRect::all(px(14.)),
                    ..container(FlexDirection::Column, 8.)
                },
                BackgroundColor(SURFACE),
            ))
            .with_children(|panel| match document {
                Document::Comment(selection, text) => {
                    panel.spawn(label(
                        format!(
                            "{} · lines {}–{}",
                            selection.anchor.path, selection.anchor.start, selection.anchor.end
                        ),
                        15.,
                        INK,
                    ));
                    field = Some(native_panel::input(panel, "Comment", text, 0, true));
                    feedback = Some(panel.spawn(label("", 13., MUTED)).id());
                    panel
                        .spawn(container(FlexDirection::Row, 6.))
                        .with_children(|bar| {
                            button(bar, "Save comment", Action::SaveComment);
                            button(bar, "Cancel", Action::Cancel);
                        });
                }
                Document::Edit(identity, source) => {
                    panel
                        .spawn(container(FlexDirection::Row, 8.))
                        .with_children(|bar| {
                            button(bar, "Done", Action::Cancel);
                            bar.spawn(label(
                                format!(
                                    "{} · lines {}–{}",
                                    identity.path,
                                    identity.range.start + 1,
                                    identity.range.end
                                ),
                                15.,
                                INK,
                            ));
                        });
                    #[cfg(not(target_arch = "wasm32"))]
                    let apply = (repo.is_worktree() && !identity.before).then(|| {
                        source_editor::ApplyTarget {
                            root: repo.root.clone(),
                            path: identity.path.clone(),
                            expected: source.clone(),
                            bytes: 0..0,
                        }
                    });
                    #[cfg(target_arch = "wasm32")]
                    let apply = {
                        let _ = repo;
                        None
                    };
                    field = Some(source_editor::spawn(panel, identity.clone(), source, apply));
                }
            });
    });
    if let Some(field) = field {
        parent.commands().queue(move |world: &mut World| {
            world
                .resource_mut::<InputFocus>()
                .set(field, FocusCause::Pressed);
        });
    }
    (field, feedback)
}

fn dialog(parent: &mut ChildSpawnerCommands, canvas: &Canvas, repo: &Repository, size: Vec2) {
    let mut modal = parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.),
            top: px(0.),
            width: percent(100.),
            height: percent(100.),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.75)),
        Pickable::default(),
    ));
    #[cfg(target_arch = "wasm32")]
    modal.insert(geometry::DialogControls);
    modal.with_children(|parent| {
        parent
            .spawn((
                Node {
                    width: px((size.x - 24.).clamp(240., 560.)),
                    max_height: px((size.y - 24.).max(100.)),
                    padding: UiRect::all(px(12.)),
                    border: UiRect::all(px(1.)),
                    overflow: Overflow::clip(),
                    ..container(FlexDirection::Column, 6.)
                },
                BorderColor::all(MUTED),
                BackgroundColor(SURFACE),
            ))
            .with_children(|parent| {
                match &canvas.dialog {
                    Dialog::Canvases(paths) => {
                        let title = if paths.is_some() {
                            "Add to canvas"
                        } else {
                            "Open canvas"
                        };
                        parent.spawn(label(title, 17., INK));
                        for (index, saved) in
                            canvas.book.as_ref().unwrap().canvases().iter().enumerate()
                        {
                            button(
                                parent,
                                format!("{} ({})", saved.name, saved.tiles.len()),
                                Action::Use(index),
                            );
                        }
                        button(parent, "New canvas…", Action::New);
                    }
                    Dialog::New(_) => {
                        parent.spawn(label("New canvas", 17., INK));
                        parent.spawn(label(format!("Name  {}▏", canvas.name), 14., INK));
                        button(parent, "Create", Action::Create);
                    }
                    Dialog::Paths {
                        query,
                        selected,
                        offset,
                    } => {
                        let page = ((size.y - 215.) / 34.).floor().clamp(2., 14.) as usize;
                        let lower = query.to_lowercase();
                        let matches = repo
                            .files
                            .iter()
                            .filter(|file| file.path.to_lowercase().contains(&lower));
                        parent.spawn(label("Add files", 17., INK));
                        parent.spawn(label(format!("Search  {query}▏"), 14., INK));
                        for file in matches.clone().skip(*offset).take(page) {
                            let mark = if selected.contains(&file.path) {
                                "✓"
                            } else {
                                "□"
                            };
                            button(
                                parent,
                                format!("{mark} {}", file.path),
                                Action::Toggle(file.path.clone()),
                            );
                        }
                        parent
                            .spawn(Node {
                                min_height: px(28.),
                                ..container(FlexDirection::Row, 4.)
                            })
                            .with_children(|parent| {
                                if *offset > 0 {
                                    button(
                                        parent,
                                        "Previous",
                                        Action::SourcePage(-(page as isize)),
                                    );
                                }
                                if matches.skip(offset + page).next().is_some() {
                                    button(parent, "Next", Action::SourcePage(page as isize));
                                }
                                if !selected.is_empty() {
                                    button(
                                        parent,
                                        if selected.len() == 1 {
                                            "Add 1 file".into()
                                        } else {
                                            format!("Add {} files", selected.len())
                                        },
                                        Action::Add,
                                    );
                                }
                            });
                    }
                    Dialog::None | Dialog::Document(_) => {}
                }
                if matches!(canvas.dialog, Dialog::New(_)) || !canvas.status.is_empty() {
                    parent.spawn((
                        Node {
                            height: px(17.),
                            ..default()
                        },
                        label(canvas.status.clone(), 13., MUTED),
                    ));
                }
                button(parent, "Cancel", Action::Cancel);
            });
    });
}

pub(super) fn world_camera(
    canvas: Res<Canvas>,
    mut player: Query<(Entity, &mut Camera), PlayerCamera>,
    mut suspended: Local<Option<(Entity, bool)>>,
) {
    if canvas.hides_tablet() && canvas.camera.is_some() {
        if suspended.is_none()
            && let Ok((entity, mut camera)) = player.single_mut()
        {
            *suspended = Some((entity, camera.is_active));
            camera.is_active = false;
        }
    } else if let Some((entity, was_active)) = suspended.take()
        && let Ok((_, mut camera)) = player.get_mut(entity)
    {
        camera.is_active = was_active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_can_pan_past_origin_and_empty_space_in_every_direction() {
        let model = model::Canvas {
            name: "Changes".into(),
            tiles: Vec::new(),
            pan: [-300., -200.],
            zoom: 1.,
            layout_width: None,
        };
        let layout = Layout::new(Vec2::new(1280., 800.), &model);
        assert_eq!(layout.pan, Vec2::new(-300., -200.));
        assert_eq!(Layout::clamp(Vec2::new(600., 800.)), Vec2::new(600., 800.));
        assert_eq!(Layout::clamp(Vec2::ZERO), Vec2::ZERO);
        assert_eq!(layout.tile_at(Vec2::new(10., 80.), &model.tiles), None);
    }
}
