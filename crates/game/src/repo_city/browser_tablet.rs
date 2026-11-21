//! Browser service state, presented with the same GPU widgets and screen as desktop.
use super::*;
use base64::Engine;
use bevy::{input_focus::InputFocus, picking::Pickable, text::EditableText};
use serde::Deserialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tabletView)]
    fn take_view() -> String;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tabletAction)]
    fn send_action(value: &str);
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct View {
    open: bool,
    title: String,
    focus: Option<String>,
    nodes: Vec<NodeView>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct NodeView {
    id: String,
    kind: String,
    text: String,
    name: String,
    value: String,
    checked: bool,
    before: bool,
    disabled: bool,
    selected: bool,
    multiline: bool,
    max_length: Option<usize>,
    rows: usize,
    children: Vec<NodeView>,
    options: Vec<Choice>,
    source: String,
    data: String,
    color: String,
}
#[derive(Clone, Default, Deserialize)]
struct Choice {
    text: String,
    value: String,
}
#[derive(Component)]
pub(super) struct Binding(String);
#[derive(Component)]
pub(super) struct Label(String);
#[derive(Component, Clone, Copy)]
struct Action(u64);
#[derive(Component)]
#[require(tablet_scroll::Area)]
pub(super) struct Body;

#[derive(Resource, Default)]
pub(super) struct Tablet {
    panel: Option<Entity>,
    signature: Vec<(String, String)>,
    nodes: HashMap<String, NodeView>,
    drafts: HashMap<String, String>,
    actions: Vec<u64>,
    ends: HashSet<String>,
    images: HashMap<String, Result<Handle<Image>, String>>,
    image_nodes: HashSet<String>,
}
impl native_panel::Actions<Action> for Tablet {
    fn activate(&mut self, action: Action) {
        self.actions.push(action.0);
    }
}

fn flatten(
    nodes: &[NodeView],
    out: &mut Vec<(String, String)>,
    map: &mut HashMap<String, NodeView>,
) {
    for node in nodes {
        out.push((node.id.clone(), node.kind.clone()));
        map.insert(node.id.clone(), node.clone());
        flatten(&node.children, out, map);
    }
}

fn label(node: &NodeView) -> String {
    match node.kind.as_str() {
        "editor" => node.name.clone(),
        "check" => format!("{}: {}", node.text, if node.checked { "on" } else { "off" }),
        "select" => node
            .options
            .iter()
            .find(|option| option.value == node.value)
            .map_or_else(|| node.value.clone(), |option| option.text.clone()),
        "range" => format!(
            "Volume: {}%",
            node.value.parse::<f32>().unwrap_or(0.).round()
        ),
        _ => node.text.clone(),
    }
}

fn draw(parent: &mut ChildSpawnerCommands, node: &NodeView, order: &mut i32) {
    let ink = review_notes::color_picker::parse_hex(&node.color)
        .map_or(native_panel::INK, |[r, g, b]| Color::srgb_u8(r, g, b));
    let binding = || Binding(node.id.clone());
    match node.kind.as_str() {
        "header" | "tabs" => {
            parent
                .spawn((
                    binding(),
                    Node {
                        width: percent(100.),
                        min_width: px(0.),
                        flex_shrink: 0.,
                        flex_direction: FlexDirection::Row,
                        flex_wrap: if node.kind == "tabs" {
                            FlexWrap::Wrap
                        } else {
                            FlexWrap::NoWrap
                        },
                        align_items: AlignItems::Center,
                        column_gap: px(8.),
                        row_gap: px(8.),
                        ..default()
                    },
                ))
                .with_children(|bar| {
                    for child in &node.children {
                        if node.kind == "header" && child.kind == "heading" {
                            bar.spawn(Node {
                                flex_grow: 1.,
                                flex_basis: px(0.),
                                min_width: px(0.),
                                flex_direction: FlexDirection::Column,
                                ..default()
                            })
                            .with_children(|title| draw(title, child, order));
                        } else {
                            draw(bar, child, order);
                        }
                    }
                });
        }
        "editor" => {
            let identity = match source_editor::DraftIdentity::parse(&node.source, &node.name) {
                Ok(identity) => identity,
                Err(error) => {
                    native_panel::text(parent, &error, native_panel::INK);
                    return;
                }
            };
            let entity = source_editor::spawn(parent, identity, &node.text, None);
            parent.commands().entity(entity).insert((
                binding(),
                bevy::input_focus::tab_navigation::TabIndex(*order),
            ));
            *order += 1;
        }
        "input" | "color" => {
            let input = native_panel::input(parent, "", &node.value, *order, node.multiline);
            if node.max_length.is_some() || node.rows > 4 {
                let (limit, rows) = (node.max_length, node.rows);
                parent.commands().queue(move |world: &mut World| {
                    if let Some(limit) = limit
                        && let Some(mut field) = world.get_mut::<EditableText>(input)
                    {
                        field.max_characters = Some(limit);
                    }
                    if rows > 4
                        && let Some(mut node) = world.get_mut::<Node>(input)
                    {
                        node.height = px((rows as f32 * 20. + 12.).min(240.));
                    }
                });
            }
            parent.commands().entity(input).insert((
                binding(),
                Name::new(if node.text.contains("File name") {
                    "File name or path".to_owned()
                } else {
                    node.text.clone()
                }),
            ));
            if node.disabled {
                parent
                    .commands()
                    .entity(input)
                    .insert(bevy::ui::InteractionDisabled);
            }
            if node.kind == "color" {
                review_notes::color_picker::spawn(
                    parent,
                    input,
                    review_notes::color_picker::parse_hex(&node.value).unwrap_or([151, 199, 201]),
                );
            }
            *order += 1;
        }
        "button" | "check" | "select" | "range" => {
            if node.text.is_empty() && node.kind == "button" {
                return;
            }
            let button = native_panel::button::<Action, Tablet>(
                parent,
                &label(node),
                if node.disabled {
                    Color::srgb_u8(83, 111, 111)
                } else {
                    ink
                },
                Action(node.id.parse().unwrap_or(0)),
            );
            parent.commands().entity(button).insert((
                binding(),
                bevy::input_focus::tab_navigation::TabIndex(*order),
            ));
            let label_id = node.id.clone();
            parent.commands().queue(move |world: &mut World| {
                if let Some(child) = world
                    .get::<Children>(button)
                    .and_then(|children| children.first())
                    .copied()
                {
                    world.entity_mut(child).insert(Label(label_id));
                }
            });
            if node.disabled {
                parent
                    .commands()
                    .entity(button)
                    .insert(bevy::ui::InteractionDisabled);
            }
            if node.selected {
                parent
                    .commands()
                    .entity(button)
                    .insert(BackgroundColor(Color::srgb_u8(20, 78, 77)));
            }
            *order += 1;
        }
        "code" if !node.name.is_empty() => {
            let document = markdown_preview::draw_source(parent, &node.name, &node.text);
            parent.commands().entity(document).insert(binding());
        }
        "text" | "heading" | "code" | "status" => {
            let mut font = native_panel::panel_font();
            font.font_size = (if node.kind == "heading" { 22. } else { 18. }).into();
            parent.spawn((
                binding(),
                Label(node.id.clone()),
                Text::new(&node.text),
                font,
                TextColor(if node.kind == "heading" {
                    ink
                } else {
                    Color::srgb_u8(205, 227, 228)
                }),
                Pickable::IGNORE,
                Node {
                    flex_shrink: 0.,

                    width: if node.kind == "heading" {
                        Val::Auto
                    } else {
                        percent(100.)
                    },
                    margin: UiRect::bottom(px(5.)),
                    ..default()
                },
            ));
        }
        "spacer" => {
            parent.spawn(Node {
                flex_grow: 1.,
                ..default()
            });
        }
        "controls" => {
            parent
                .spawn(Node {
                    display: Display::Grid,
                    width: percent(100.),
                    grid_template_columns: vec![GridTrack::flex(1.), GridTrack::flex(3.)],
                    column_gap: px(18.),
                    row_gap: px(12.),
                    flex_shrink: 0.,
                    ..default()
                })
                .with_children(|grid| {
                    for child in &node.children {
                        draw(grid, child, order);
                    }
                });
        }
        "markdown" => {
            let source = (!node.name.is_empty()).then(|| markdown_preview::SourceFile {
                path: node.name.clone(),
                before: node.before,
            });
            let document =
                markdown_preview::draw(parent, markdown_preview::markdown_html(&node.text), source);
            parent.commands().entity(document).insert(binding());
        }
        "end" => {
            parent.spawn((
                binding(),
                Node {
                    height: px(2.),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
        }
        "image" => {
            parent
                .spawn((binding(), Node::default()))
                .with_children(|body| {
                    native_panel::text(body, &format!("Loading image: {}", node.text), ink);
                });
        }
        _ => {
            let row = node.kind == "row" || node.kind == "columns";
            parent
                .spawn((
                    binding(),
                    Node {
                        width: percent(100.),
                        min_width: px(0.),
                        flex_shrink: 0.,
                        flex_direction: if row {
                            FlexDirection::Row
                        } else {
                            FlexDirection::Column
                        },
                        flex_wrap: if row {
                            FlexWrap::Wrap
                        } else {
                            FlexWrap::NoWrap
                        },
                        column_gap: px(if node.kind == "columns" { 24. } else { 8. }),
                        row_gap: px(if node.kind == "columns" { 20. } else { 8. }),
                        padding: UiRect::all(px(if node.kind == "card" { 18. } else { 0. })),
                        border: UiRect::all(px(if node.kind == "card" { 1. } else { 0. })),
                        ..default()
                    },
                    BackgroundColor(if node.kind == "card" {
                        Color::srgb_u8(10, 32, 40)
                    } else {
                        Color::NONE
                    }),
                    BorderColor::all(Color::srgb_u8(39, 71, 78)),
                ))
                .with_children(|group| {
                    for child in &node.children {
                        if node.kind == "columns" || (node.kind == "row" && child.kind == "column")
                        {
                            group
                                .spawn(Node {
                                    flex_basis: px(if node.kind == "columns" {
                                        400.
                                    } else {
                                        200.
                                    }),
                                    flex_grow: 1.,
                                    min_width: px(if node.kind == "columns" { 260. } else { 140. }),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(20.),
                                    ..default()
                                })
                                .with_children(|column| draw(column, child, order));
                        } else {
                            draw(group, child, order);
                        }
                    }
                });
        }
    }
}

pub(super) fn update(
    mut commands: Commands,
    mut tablet: ResMut<Tablet>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &Binding, &mut EditableText)>,
    mut editors: Query<(&Binding, &mut source_editor::View)>,
    mut texts: Query<(&Label, &mut Text, &mut TextColor)>,
    bindings: Query<(Entity, &Binding)>,
    parents: Query<&ChildOf>,
    mut scrolls: Query<(&mut ScrollPosition, &ComputedNode), With<Body>>,
    scroll: Res<AccumulatedMouseScroll>,
    layout: Query<(&UiGlobalTransform, &ComputedNode)>,
    documents: Query<(&ComputedNode, &Children), With<bevy_markup::prelude::HtmlUi>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut images: ResMut<Assets<Image>>,
    mut touch_scroll: Option<ResMut<tablet_display::TouchScroll>>,
) {
    for (binding, mut editor) in &mut editors {
        if let Some(value) = editor.take_change() {
            send_action(
                &serde_json::json!({"id": binding.0,"kind":"editor","value":value,"range":editor.range()}).to_string(),
            );
        }
    }
    for (_, binding, field) in &mut fields {
        if tablet.nodes.contains_key(&binding.0) {
            let text = field.value().to_string();
            if tablet.drafts.get(&binding.0) != Some(&text) {
                tablet.drafts.insert(binding.0.clone(), text.clone());
                send_action(
                    &serde_json::json!({ "id": binding.0, "kind": "input", "value": text })
                        .to_string(),
                );
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter)
        && let Some(entity) = focus.get()
        && let Ok((_, binding, field)) = fields.get(entity)
        && !field.allow_newlines
    {
        send_action(&serde_json::json!({ "id": binding.0, "kind": "enter" }).to_string());
    }
    for id in std::mem::take(&mut tablet.actions) {
        if let Some(node) = tablet.nodes.get(&id.to_string()) {
            let kind = match node.kind.as_str() {
                "check" | "select" | "range" => node.kind.as_str(),
                _ => "click",
            };
            send_action(&serde_json::json!({ "id": node.id, "kind": kind }).to_string());
        }
    }
    let packet = take_view();
    let view = if packet.is_empty() {
        None
    } else {
        match serde_json::from_str::<View>(&packet) {
            Ok(view) => Some(view),
            Err(error) => {
                error!("Invalid browser tablet view: {error}");
                None
            }
        }
    };
    if let Some(view) = view {
        if !view.open {
            if let Some(panel) = tablet.panel.take() {
                native_panel::close(&mut commands, panel, &mut focus, &parents);
            }
            tablet.signature.clear();
            tablet.nodes.clear();
            tablet.drafts.clear();
            tablet.ends.clear();
            tablet.images.clear();
            tablet.image_nodes.clear();
        } else {
            let mut signature = Vec::new();
            let mut nodes = HashMap::new();
            flatten(&view.nodes, &mut signature, &mut nodes);
            let rebuilt = signature != tablet.signature || tablet.panel.is_none();
            if rebuilt {
                let focused_id = view.focus.clone().or_else(|| {
                    focus
                        .get()
                        .and_then(|entity| bindings.get(entity).ok())
                        .map(|(_, binding)| binding.0.clone())
                });
                let editor = focus
                    .get()
                    .and_then(|entity| fields.get(entity).ok())
                    .filter(|(_, binding, _)| focused_id.as_deref() == Some(binding.0.as_str()))
                    .map(|(_, _, field)| field.clone());
                let panel = tablet
                    .panel
                    .unwrap_or_else(|| native_panel::tablet(&mut commands, ""));
                commands.entity(panel).despawn_children();
                commands.entity(panel).with_children(|root| {
                    let mut order = 0;
                    for node in view
                        .nodes
                        .iter()
                        .filter(|node| matches!(node.kind.as_str(), "header" | "tabs"))
                    {
                        draw(root, node, &mut order);
                    }
                    root.spawn((
                        Body,
                        ScrollPosition::default(),
                        Node {
                            width: percent(100.),
                            flex_grow: 1.,
                            min_height: px(0.),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(14.),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(|body| {
                        for node in view
                            .nodes
                            .iter()
                            .filter(|node| !matches!(node.kind.as_str(), "header" | "tabs"))
                        {
                            draw(body, node, &mut order);
                        }
                    });
                });
                commands.entity(panel).insert(Name::new(view.title));
                tablet.panel = Some(panel);
                tablet.signature = signature;
                tablet.ends.clear();
                tablet.image_nodes.clear();
                commands.queue(move |world: &mut World| {
                    use bevy::input_focus::tab_navigation::TabIndex;
                    let mut query =
                        world
                            .query::<(Entity, &Binding, Option<&EditableText>, Option<&TabIndex>)>(
                            );
                    let restored = focused_id.and_then(|id| {
                        query
                            .iter(world)
                            .find(|(_, binding, _, _)| binding.0 == id)
                            .map(|(entity, _, _, _)| entity)
                    });
                    let selected = restored
                        .or_else(|| {
                            query
                                .iter(world)
                                .filter(|(_, _, field, _)| field.is_some())
                                .min_by_key(|(_, _, _, index)| {
                                    index.map_or(i32::MAX, |index| index.0)
                                })
                                .map(|(entity, _, _, _)| entity)
                        })
                        .or_else(|| {
                            query
                                .iter(world)
                                .filter(|(_, _, _, index)| index.is_some())
                                .min_by_key(|(_, _, _, index)| index.unwrap().0)
                                .map(|(entity, _, _, _)| entity)
                        });
                    if let Some(entity) = selected {
                        if restored == Some(entity)
                            && let Some(editor) = editor
                        {
                            world.entity_mut(entity).insert(editor);
                        }
                        world
                            .resource_mut::<InputFocus>()
                            .set(entity, bevy::input_focus::FocusCause::Navigated);
                    }
                });
            } else {
                if let Some(id) = view.focus
                    && let Some((entity, _)) = bindings.iter().find(|(_, binding)| binding.0 == id)
                {
                    focus.set(entity, bevy::input_focus::FocusCause::Navigated);
                }
                for (label_id, mut text, mut color) in &mut texts {
                    if let Some(node) = nodes.get(&label_id.0) {
                        if ["button", "check", "select", "range"].contains(&node.kind.as_str()) {
                            color.0 = if node.disabled {
                                Color::srgb_u8(83, 111, 111)
                            } else {
                                native_panel::INK
                            };
                        }
                        let next = label(node);
                        if text.0 != next {
                            text.0 = next;
                        }
                    }
                }
                for (entity, binding, mut field) in &mut fields {
                    if focus.get() != Some(entity)
                        && let Some(node) = nodes.get(&binding.0)
                        && field.value().to_string() != node.value
                    {
                        field.editor_mut().set_text(&node.value);
                        tablet.drafts.insert(binding.0.clone(), node.value.clone());
                    }
                }
            }
            let sources: HashSet<_> = nodes
                .values()
                .filter(|node| node.kind == "image")
                .map(|node| node.source.clone())
                .collect();
            tablet.images.retain(|source, _| sources.contains(source));
            for (entity, binding) in bindings.iter().filter(|_| !rebuilt) {
                if let Some(node) = nodes.get(&binding.0)
                    && ["button", "check", "select", "range", "input", "color"]
                        .contains(&node.kind.as_str())
                {
                    commands
                        .entity(entity)
                        .insert(BackgroundColor(if node.selected {
                            Color::srgb_u8(20, 78, 77)
                        } else {
                            Color::srgb(0.055, 0.13, 0.15)
                        }));
                    if node.disabled {
                        commands
                            .entity(entity)
                            .insert(bevy::ui::InteractionDisabled);
                    } else {
                        commands
                            .entity(entity)
                            .remove::<bevy::ui::InteractionDisabled>();
                    }
                }
            }
            tablet.drafts.retain(|id, _| nodes.contains_key(id));
            for node in nodes
                .values()
                .filter(|node| node.kind == "input" || node.kind == "color")
            {
                tablet
                    .drafts
                    .entry(node.id.clone())
                    .or_insert_with(|| node.value.clone());
            }
            tablet.nodes = nodes;
            // Queries still contain the previous children until deferred commands flush.
            // Their bindings can survive a service refresh, but their entities cannot.
            if rebuilt {
                return;
            }
        }
    }
    if tablet.panel.is_none() {
        return;
    }
    for (entity, binding) in &bindings {
        if tablet.image_nodes.contains(&binding.0) {
            continue;
        }
        let Some(node) = tablet
            .nodes
            .get(&binding.0)
            .filter(|node| node.kind == "image" && !node.data.is_empty())
            .cloned()
        else {
            continue;
        };
        if !tablet.images.contains_key(&node.source) {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&node.data)
                .map_err(|error| error.to_string())
                .and_then(|bytes| image_preview::decode_named(&node.text, &bytes))
                .map(|image| images.add(image));
            tablet.images.insert(node.source.clone(), decoded);
        }
        match tablet.images.get(&node.source).unwrap() {
            Ok(handle) => {
                commands.entity(entity).despawn_children().insert((
                    ImageNode::new(handle.clone()),
                    Node {
                        max_width: percent(100.),
                        max_height: px(700.),
                        ..default()
                    },
                ));
            }
            Err(error) => {
                send_action(
                    &serde_json::json!({ "id": binding.0, "kind": "image-error", "value": error })
                        .to_string(),
                );
                commands
                    .entity(entity)
                    .despawn_children()
                    .with_children(|body| {
                        native_panel::text(body, error, native_panel::INK);
                    });
            }
        }
        tablet.image_nodes.insert(binding.0.clone());
    }

    let touch_delta = touch_scroll
        .as_mut()
        .map_or(0., |scroll| std::mem::take(&mut scroll.0));
    for (mut position, node) in &mut scrolls {
        let step = if keys.pressed(KeyCode::PageDown) {
            18.
        } else if keys.pressed(KeyCode::PageUp) {
            -18.
        } else {
            0.
        };
        position.y = (position.y - scroll.delta.y * 24. + step + touch_delta).clamp(
            0.,
            (node.content_size().y - node.size().y).max(0.) * node.inverse_scale_factor(),
        );
    }
    let documents_ready = bindings
        .iter()
        .filter(|(_, binding)| {
            tablet.nodes.get(&binding.0).is_some_and(|node| {
                node.kind == "markdown" || (node.kind == "code" && !node.name.is_empty())
            })
        })
        .all(|(entity, _)| {
            documents
                .get(entity)
                .is_ok_and(|(node, children)| node.size().y > 0. && !children.is_empty())
        });
    if documents_ready && scrolls.single().is_ok() {
        for (entity, binding) in &bindings {
            if tablet
                .nodes
                .get(&binding.0)
                .is_some_and(|node| node.kind == "end")
                && !tablet.ends.contains(&binding.0)
                && let Ok((transform, _)) = layout.get(entity)
            {
                let transform: bevy::math::Affine2 = transform.into();
                let visible = parents
                    .iter_ancestors(entity)
                    .find_map(|parent| {
                        if !scrolls.contains(parent) {
                            return None;
                        }
                        let (area, node) = layout.get(parent).ok()?;
                        let area: bevy::math::Affine2 = area.into();
                        let top = area.translation.y - node.size().y / 2.;
                        Some(
                            transform.translation.y >= top
                                && transform.translation.y <= top + node.size().y,
                        )
                    })
                    .unwrap_or(false);
                if visible {
                    send_action(&serde_json::json!({ "id": binding.0, "kind": "end" }).to_string());
                    tablet.ends.insert(binding.0.clone());
                }
            }
        }
    }
}

/// Screen geometry exposes the current keyboard target to browser inspectors.
pub(super) fn geometry(
    tablet: Res<Tablet>,
    display: Option<Res<tablet_display::Display>>,
    pose: Query<&Transform, With<android::ViewTablet>>,
    window: Query<&Window, With<PrimaryWindow>>,
    controls: Query<(
        Entity,
        &Binding,
        &UiGlobalTransform,
        &ComputedNode,
        Option<&bevy::input_focus::tab_navigation::TabIndex>,
        Option<&source_editor::View>,
    )>,
    focus: Res<InputFocus>,
    entities: Query<&Binding>,
    parents: Query<&ChildOf>,
    clipping: Query<(&UiGlobalTransform, &ComputedNode, &Node)>,
    scroll_parts: Query<(&tablet_scroll::Part, &UiGlobalTransform, &ComputedNode)>,
    scroll_areas: Query<(&ScrollPosition, &ComputedNode), With<tablet_scroll::Area>>,
    mut previous: Local<String>,
) {
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tabletLayout)]
        fn send_layout(value: &str);
    }
    let (Some(display), Ok(pose), Ok(window)) = (display, pose.single(), window.single()) else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let Some(corners) = android::tablet_corners(pose, viewport) else {
        return;
    };
    let project = |point: Vec2| {
        let uv = point / display.size.as_vec2();
        Vec2::new(corners[0], corners[1])
            .lerp(Vec2::new(corners[2], corners[3]), uv.x)
            .lerp(
                Vec2::new(corners[6], corners[7]).lerp(Vec2::new(corners[4], corners[5]), uv.x),
                uv.y,
            )
    };
    let controls: Vec<_> = controls.iter().filter_map(|(entity, binding, transform, node, index, editor)| {
        let view = tablet.nodes.get(&binding.0)?;
        if index.is_none() && !["heading", "text", "code", "status"].contains(&view.kind.as_str()) { return None; }
        let transform: bevy::math::Affine2 = transform.into();
        let min = transform.translation - node.size() / 2.;
        let max = min + node.size();
        let mut clip_min = Vec2::splat(20.);
        let mut clip_max = display.size.as_vec2() - Vec2::splat(20.);
        for ancestor in parents.iter_ancestors(entity) {
            let Ok((transform, area, style)) = clipping.get(ancestor) else { continue; };
            let transform: bevy::math::Affine2 = transform.into();
            let area_min = transform.translation - area.size() / 2.;
            let area_max = area_min + area.size();
            if style.overflow.x != OverflowAxis::Visible {
                clip_min.x = clip_min.x.max(area_min.x);
                clip_max.x = clip_max.x.min(area_max.x);
            }
            if style.overflow.y != OverflowAxis::Visible {
                clip_min.y = clip_min.y.max(area_min.y);
                clip_max.y = clip_max.y.min(area_max.y);
            }
        }
        let visible = min.cmpge(clip_min - Vec2::splat(0.5)).all()
            && max.cmple(clip_max + Vec2::splat(0.5)).all();
        let clip_min = project(clip_min); let clip_max = project(clip_max);
        let min = project(min); let max = project(max);
        Some(serde_json::json!({"id":binding.0,"kind":view.kind,"text":if view.name.is_empty() { label(view) } else { view.name.clone() },"display":label(view),"value":editor.map_or(view.value.as_str(), source_editor::View::value),"status":editor.map(source_editor::View::status_label),"disabled":view.disabled,"visible":visible,"order":index.map_or(-1, |index| index.0),"bounds":[min.x,min.y,max.x-min.x,max.y-min.y],"clip":[clip_min.x,clip_min.y,clip_max.x-clip_min.x,clip_max.y-clip_min.y]}))
    }).collect();
    let focused = focus
        .get()
        .and_then(|entity| entities.get(entity).ok())
        .map(|binding| binding.0.clone());
    let scrollbars: Vec<_> = scroll_parts.iter().filter_map(|(part, transform, node)| {
        let (position, area) = scroll_areas.get(part.area).ok()?;
        let transform: bevy::math::Affine2 = transform.into();
        let min = transform.translation - node.size() / 2.;
        let max = project(min + node.size());
        let min = project(min);
        Some(serde_json::json!({"kind":part.kind,"offset":position.y,"range":tablet_scroll::range(area),"bounds":[min.x,min.y,max.x-min.x,max.y-min.y]}))
    }).collect();
    let packet = serde_json::json!({"open":tablet.panel.is_some(),"viewport":[viewport.x,viewport.y],"controls":controls,"focused":focused,"scrollbars":scrollbars}).to_string();
    if *previous != packet {
        send_layout(&packet);
        *previous = packet;
    }
}
