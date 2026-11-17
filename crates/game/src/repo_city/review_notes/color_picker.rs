use bevy::{
    color::Hsva,
    picking::Pickable,
    text::EditableText,
    ui::{BackgroundGradient, ColorStop, LinearGradient, RelativeCursorPosition},
};

use super::*;

#[derive(Component)]
pub(crate) struct Picker {
    hex: Entity,
    hue: Entity,
    field: Entity,
    hue_marker: Entity,
    field_marker: Entity,
    active: Option<Entity>,
    hsv: Hsva,
    rgb: [u8; 3],
}

pub(super) fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if text.len() != 6 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let [_, r, g, b] = u32::from_str_radix(text, 16).ok()?.to_be_bytes();
    Some([r, g, b])
}

fn rgb(hsv: Hsva) -> [u8; 3] {
    let color = Srgba::from(hsv);
    [color.red, color.green, color.blue].map(|v| (v * 255.).round() as u8)
}

fn marker(parent: &mut ChildSpawnerCommands, full_height: bool) -> Entity {
    parent
        .spawn((
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                width: px(if full_height { 4. } else { 9. }),
                height: if full_height { percent(100.) } else { px(9.) },
                border: UiRect::all(px(1.)),
                ..default()
            },
            BorderColor::all(Color::BLACK),
            BackgroundColor(Color::WHITE),
        ))
        .id()
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands, hex: Entity, value: [u8; 3]) {
    let [r, g, b] = value;
    let hsv = Hsva::from(Srgba::rgb_u8(r, g, b));
    let mut hue_marker = Entity::PLACEHOLDER;
    let mut field_marker = Entity::PLACEHOLDER;
    let field = parent
        .spawn((
            Name::new("Saturation and brightness"),
            RelativeCursorPosition::default(),
            Node {
                height: px(110.),
                min_height: px(52.),
                ..default()
            },
            BackgroundColor(Color::hsv(hsv.hue, 1., 1.)),
        ))
        .with_children(|pad| {
            pad.spawn((
                Pickable::IGNORE,
                Node {
                    width: percent(100.),
                    height: percent(100.),
                    ..default()
                },
                BackgroundGradient(vec![
                    LinearGradient::to_right(vec![
                        ColorStop::new(Color::WHITE, percent(0.)),
                        ColorStop::new(Color::NONE, percent(100.)),
                    ])
                    .into(),
                    LinearGradient::to_bottom(vec![
                        ColorStop::new(Color::NONE, percent(0.)),
                        ColorStop::new(Color::BLACK, percent(100.)),
                    ])
                    .into(),
                ]),
            ));
            field_marker = marker(pad, false);
        })
        .id();
    let hue = parent
        .spawn((
            Name::new("Hue"),
            RelativeCursorPosition::default(),
            Node {
                height: px(20.),
                min_height: px(16.),
                ..default()
            },
            BackgroundGradient(vec![
                LinearGradient::to_right(
                    (0..=6)
                        .map(|stop| {
                            ColorStop::new(
                                Color::hsv(stop as f32 * 60., 1., 1.),
                                percent(stop as f32 * 100. / 6.),
                            )
                        })
                        .collect(),
                )
                .into(),
            ]),
        ))
        .with_children(|track| hue_marker = marker(track, true))
        .id();
    parent.spawn(Picker {
        hex,
        hue,
        field,
        hue_marker,
        field_marker,
        active: None,
        hsv,
        rgb: value,
    });
}

pub(crate) fn update(
    mouse: Res<ButtonInput<MouseButton>>,
    mut pickers: Query<&mut Picker>,
    cursor: Query<&RelativeCursorPosition>,
    mut fields: Query<&mut EditableText>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut markers: Query<&mut Node>,
) {
    let Ok(mut picker) = pickers.single_mut() else {
        return;
    };
    let Ok(mut hex) = fields.get_mut(picker.hex) else {
        return;
    };
    if let Some(value) = parse_hex(&hex.value().to_string())
        && value != picker.rgb
    {
        let [r, g, b] = value;
        picker.hsv = Hsva::from(Srgba::rgb_u8(r, g, b));
        picker.rgb = value;
    }
    if mouse.just_pressed(MouseButton::Left) {
        picker.active = [picker.hue, picker.field].into_iter().find(|&part| {
            cursor
                .get(part)
                .is_ok_and(RelativeCursorPosition::cursor_over)
        });
    }
    if !mouse.pressed(MouseButton::Left) {
        picker.active = None;
    }
    if let Some(part) = picker.active
        && let Ok(point) = cursor.get(part)
        && let Some(point) = point.normalized
    {
        let point = (point + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE);
        if part == picker.hue {
            picker.hsv.hue = point.x * 360.;
        } else {
            picker.hsv.saturation = point.x;
            picker.hsv.value = 1. - point.y;
        }
        let value = rgb(picker.hsv);
        if value != picker.rgb || parse_hex(&hex.value().to_string()).is_none() {
            picker.rgb = value;
            let [r, g, b] = value;
            hex.editor_mut()
                .set_text(&format!("#{r:02x}{g:02x}{b:02x}"));
        }
    }
    if let Ok(mut pad) = backgrounds.get_mut(picker.field) {
        pad.set_if_neq(BackgroundColor(Color::hsv(picker.hsv.hue, 1., 1.)));
    }
    if let Ok(mut mark) = markers.get_mut(picker.hue_marker) {
        mark.left = percent(picker.hsv.hue / 3.6);
    }
    if let Ok(mut mark) = markers.get_mut(picker.field_marker) {
        mark.left = percent(picker.hsv.saturation * 100.);
        mark.top = percent((1. - picker.hsv.value) * 100.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_picker_use_the_same_srgb_color() {
        assert_eq!(parse_hex("#FF7e8d"), Some([255, 126, 141]));
        assert_eq!(parse_hex("#wrong!"), None);
        assert_eq!(rgb(Hsva::new(120., 1., 1., 1.)), [0, 255, 0]);
        assert_eq!(rgb(Hsva::new(120., 0., 1., 1.)), [255; 3]);
    }
}
