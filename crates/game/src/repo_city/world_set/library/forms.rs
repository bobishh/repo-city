use super::*;

#[derive(Resource)]
pub(super) struct Stone {
    heading: Handle<Font>,
}

impl Stone {
    pub(super) fn new(fonts: &mut Assets<Font>) -> Self {
        Self {
            heading: fonts.add(Font::from_bytes(
                include_bytes!("../../../../../../assets/fonts/GFSDidot-Regular.ttf").to_vec(),
            )),
        }
    }

    pub(super) fn heading(&self, size: f32) -> TextFont {
        TextFont {
            font: self.heading.clone().into(),
            ..TextFont::from_font_size(size)
        }
    }
}

type Styled = Or<(
    Changed<BackgroundColor>,
    Changed<BorderColor>,
    Changed<TextColor>,
    Added<native_panel::Heading>,
)>;

pub(super) fn apply(
    stone: Res<Stone>,
    forms: Query<(), With<native_panel::Form>>,
    parents: Query<&ChildOf>,
    mut widgets: Query<
        (
            Entity,
            Option<&mut BackgroundColor>,
            Option<&mut BorderColor>,
            Option<&mut TextColor>,
            Option<&mut TextFont>,
            Has<native_panel::Heading>,
        ),
        (With<Node>, Styled),
    >,
) {
    for (entity, background, border, foreground, font, heading) in &mut widgets {
        if !forms.contains(entity)
            && !parents
                .iter_ancestors(entity)
                .any(|parent| forms.contains(parent))
        {
            continue;
        }
        if let Some(mut background) = background {
            let themed = BackgroundColor(surface(background.0));
            background.set_if_neq(themed);
        }
        if let Some(mut border) = border {
            let themed = BorderColor {
                top: surface(border.top),
                bottom: surface(border.bottom),
                left: surface(border.left),
                right: surface(border.right),
            };
            border.set_if_neq(themed);
        }
        if let Some(mut foreground) = foreground {
            let ink = if heading || foreground.0 == native_panel::INK {
                Color::srgb_u8(250, 230, 195)
            } else if foreground.0 == Color::srgb_u8(205, 227, 228)
                || foreground.0 == Color::srgb_u8(216, 234, 235)
                || foreground.0 == Color::srgb_u8(157, 193, 196)
            {
                Color::srgb_u8(235, 218, 194)
            } else {
                foreground.0
            };
            foreground.set_if_neq(TextColor(ink));
        }
        if heading && let Some(mut font) = font {
            let themed = stone.heading.clone().into();
            if font.font != themed {
                font.font = themed;
            }
        }
    }
}

fn surface(color: Color) -> Color {
    if color == Color::srgb_u8(117, 225, 205) {
        return Color::srgb_u8(224, 184, 125);
    }
    let [r, g, b, alpha] = Srgba::from(color).to_f32_array();
    if alpha > 0. && g > r + 0.025 && b > r + 0.025 && g.max(b) < 0.72 {
        let level = (0.2 * r + 0.68 * g + 0.12 * b).clamp(0., 0.65);
        Color::srgba(
            level * 1.20 + 0.025,
            level * 0.92 + 0.015,
            level * 0.67 + 0.012,
            alpha,
        )
    } else {
        color
    }
}
