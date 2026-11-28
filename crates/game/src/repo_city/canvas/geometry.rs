//! Browser coordinates for the canvas controls currently available to the pointer.
use super::*;

#[derive(Component)]
pub(super) struct DialogControls;

pub(super) fn inspection_enabled() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
        .is_some_and(|root| root.has_attribute("data-canvas-inspection"))
}

pub(super) fn publish(
    canvas: Res<Canvas>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Query<
        (Entity, &UiGlobalTransform, &ComputedNode, &Children),
        (With<Action>, With<bevy::ui_widgets::Button>),
    >,
    parents: Query<&ChildOf>,
    dialogs: Query<(), With<DialogControls>>,
    texts: Query<&Text>,
    mut previous: Local<Option<String>>,
) {
    let controls = if canvas.target && canvas.elapsed >= FULL && inspection_enabled() {
        windows.single().ok().map(|window| {
            let viewport = Vec2::new(
                window.physical_width() as f32,
                window.physical_height() as f32,
            );
            let has_dialog = !matches!(canvas.dialog, Dialog::None);
            let controls: Vec<_> = buttons
                .iter()
                .filter(|(entity, _, _, _)| {
                    !has_dialog
                        || parents
                            .iter_ancestors(*entity)
                            .any(|ancestor| dialogs.contains(ancestor))
                })
                .filter_map(|(_, transform, node, children)| {
                    let label = children.iter().find_map(|child| texts.get(child).ok())?;
                    let transform: bevy::math::Affine2 = transform.into();
                    let center = transform.translation;
                    let min = center - node.size() / 2.;
                    let max = center + node.size() / 2.;
                    if node.size().min_element() <= 0.
                        || min.cmplt(Vec2::ZERO).any()
                        || max.cmpgt(viewport).any()
                    {
                        return None;
                    }
                    let position = center / viewport;
                    Some(serde_json::json!({"label": label.0, "x": position.x, "y": position.y}))
                })
                .collect();
            serde_json::to_string(&controls).expect("canvas geometry")
        })
    } else {
        None
    };
    if *previous == controls {
        return;
    }
    if let Some(scene) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("repo-city"))
    {
        if let Some(value) = &controls {
            let _ = scene.set_attribute("data-diff-canvas-controls", value);
        } else {
            let _ = scene.remove_attribute("data-diff-canvas-controls");
        }
    }
    *previous = controls;
}
