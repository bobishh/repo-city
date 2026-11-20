//! Native voice activity glyph. It reports session state without capturing input.
use super::*;

#[derive(Component)]
pub(super) struct Indicator;
#[derive(Component)]
pub(super) struct Caption;
#[derive(Component)]
pub(super) struct Segment(usize);
#[derive(Component)]
pub(super) struct Bar(usize);
#[derive(Component)]
pub(super) struct Microphone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Off,
    Connecting,
    Waiting,
    Listening,
    Speaking,
    Failed,
}
fn phase(session: &Session) -> Phase {
    if session.voice_error.is_some() {
        Phase::Failed
    } else if !session.voice_active() {
        Phase::Off
    } else if session.voice_waiting {
        Phase::Waiting
    } else if !session.voice_connected {
        Phase::Connecting
    } else if session.is_speaking() {
        Phase::Speaking
    } else {
        Phase::Listening
    }
}

pub(super) fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Indicator,
            Node {
                position_type: PositionType::Absolute,
                top: px(20.),
                left: percent(44.),
                width: px(240.),
                height: px(48.),
                display: Display::None,
                align_items: AlignItems::Center,
                column_gap: px(10.),
                ..default()
            },
            GlobalZIndex(30),
            bevy::picking::Pickable::IGNORE,
        ))
        .with_children(|body| {
            body.spawn(Node {
                width: px(46.),
                height: px(46.),
                flex_shrink: 0.,
                ..default()
            })
            .with_children(|glyph| {
                for index in 0..12 {
                    let angle = index as f32 * std::f32::consts::TAU / 12.;
                    glyph.spawn((
                        Segment(index),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(21. + angle.sin() * 19.),
                            top: px(21. - angle.cos() * 19.),
                            width: px(3.),
                            height: px(3.),
                            ..default()
                        },
                        BackgroundColor(native_panel::INK),
                    ));
                }
                // A microphone drawn from rectangles, independent of font coverage.
                for (x, y, width, height) in [
                    (19., 11., 8., 14.),
                    (15., 20., 2., 9.),
                    (29., 20., 2., 9.),
                    (15., 28., 16., 2.),
                    (22., 30., 2., 5.),
                    (18., 35., 10., 2.),
                ] {
                    glyph.spawn((
                        Microphone,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(x),
                            top: px(y),
                            width: px(width),
                            height: px(height),
                            ..default()
                        },
                        BackgroundColor(native_panel::INK),
                    ));
                }
                for index in 0..5 {
                    glyph.spawn((
                        Bar(index),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(11. + index as f32 * 5.),
                            top: px(20.),
                            width: px(3.),
                            height: px(6.),
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(native_panel::INK),
                    ));
                }
            });
            body.spawn((
                Caption,
                Text::new(""),
                TextFont::from_font_size(13.),
                TextColor(native_panel::INK),
            ));
        });
}

pub(super) fn update(
    time: Res<Time>,
    session: Res<Session>,
    mut roots: Query<&mut Node, (With<Indicator>, Without<Bar>, Without<Microphone>)>,
    mut captions: Query<&mut Text, With<Caption>>,
    mut segments: Query<(&Segment, &mut BackgroundColor)>,
    mut microphones: Query<
        (&mut Node, &mut BackgroundColor),
        (
            With<Microphone>,
            Without<Indicator>,
            Without<Bar>,
            Without<Segment>,
        ),
    >,
    mut bars: Query<
        (&Bar, &mut Node, &mut BackgroundColor),
        (Without<Indicator>, Without<Microphone>, Without<Segment>),
    >,
) {
    let phase = phase(&session);
    for mut root in &mut roots {
        root.display = if phase == Phase::Off {
            Display::None
        } else {
            Display::Flex
        };
    }
    let (label, color) = match phase {
        Phase::Off => ("", native_panel::INK),
        Phase::Connecting => ("Connecting voice… · F5", Color::srgb_u8(235, 181, 94)),
        Phase::Waiting => (
            "Waiting for current turn · F5",
            Color::srgb_u8(235, 181, 94),
        ),
        Phase::Listening => ("Listening · F5", Color::srgb_u8(100, 225, 205)),
        Phase::Speaking => ("Agent speaking · F5", Color::srgb_u8(174, 149, 239)),
        Phase::Failed => ("Voice failed · Agent", Color::srgb_u8(240, 113, 108)),
    };
    for mut caption in &mut captions {
        caption.set_if_neq(Text::new(label));
    }
    let now = time.elapsed_secs();
    let head = (now * 9.) as usize % 12;
    for (segment, mut ink) in &mut segments {
        let strength = if phase == Phase::Connecting {
            0.18 + 0.82 * ((segment.0 + 12 - head) % 12) as f32 / 11.
        } else {
            0.55 + 0.25 * (now * 2.).sin()
        };
        ink.0 = color.with_alpha(strength);
    }
    for (mut node, mut ink) in &mut microphones {
        node.display = if phase == Phase::Speaking {
            Display::None
        } else {
            Display::Flex
        };
        ink.0 = color;
    }
    for (bar, mut node, mut ink) in &mut bars {
        node.display = if phase == Phase::Speaking {
            Display::Flex
        } else {
            Display::None
        };
        let height = 7. + 15. * (now * 7. + bar.0 as f32 * 1.5).sin().abs();
        node.height = px(height);
        node.top = px(23. - height / 2.);
        ink.0 = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hud_reports_connecting_before_audio_ready_and_keeps_failure_visible() {
        let mut session = Session::default();
        assert_eq!(phase(&session), Phase::Off);
        session.projection.voice = Some("voice".into());
        assert_eq!(phase(&session), Phase::Connecting);
        session.voice_waiting = true;
        assert_eq!(phase(&session), Phase::Waiting);
        session.voice_waiting = false;
        session.voice_connected = true;
        assert_eq!(phase(&session), Phase::Listening);
        session.speaking.store(true, Ordering::Relaxed);
        assert_eq!(phase(&session), Phase::Speaking);
        session.projection.voice = None;
        session.voice_error = Some("Connection timed out".into());
        assert_eq!(phase(&session), Phase::Failed);
    }
}
