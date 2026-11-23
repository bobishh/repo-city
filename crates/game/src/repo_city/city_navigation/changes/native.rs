use super::*;

#[derive(Component)]
pub(crate) enum Part {
    Panel,
    Caption,
    Track,
    Dot(usize),
}

pub(crate) fn spawn(commands: &mut Commands, font: TextFont, ink: Color) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::ZERO,
                bottom: px(66.),
                width: percent(100.),
                justify_content: JustifyContent::Center,
                ..default()
            },
            desktop_splash::CityOverlay,
            bevy::picking::Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Part::Panel,
                    Node {
                        display: Display::None,
                        width: percent(94.),
                        row_gap: px(9.),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    bevy::picking::Pickable::IGNORE,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Part::Caption,
                        Text::new(""),
                        TextLayout {
                            justify: Justify::Center,
                            ..default()
                        }
                        .with_no_wrap(),
                        font,
                        TextColor(ink),
                        bevy::ui::widget::TextShadow {
                            offset: Vec2::ONE,
                            color: Color::BLACK,
                        },
                        Node {
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        bevy::picking::Pickable::IGNORE,
                    ));
                    panel
                        .spawn((
                            Part::Track,
                            Node {
                                column_gap: px(3.),
                                width: percent(100.),
                                justify_content: JustifyContent::SpaceBetween,
                                ..default()
                            },
                            bevy::picking::Pickable::IGNORE,
                        ))
                        .with_children(|track| {
                            track.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    top: px(2.5),
                                    width: percent(100.),
                                    height: px(1.),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb_u8(117, 134, 149)),
                                bevy::picking::Pickable::IGNORE,
                            ));
                            for position in 0..MAX_VISIBLE_STOPS {
                                track.spawn((
                                    Part::Dot(position),
                                    Node {
                                        height: px(6.),
                                        flex_basis: px(6.),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    UiTransform::from_rotation(Rot2::degrees(45.)),
                                    bevy::picking::Pickable::IGNORE,
                                ));
                            }
                        });
                });
        });
}

pub(crate) fn draw(
    repo: Res<Repo>,
    navigation: Res<Navigation>,
    progress: Res<progress::Progress>,
    time: Res<Time>,
    review: Res<review_notes::Review>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut parts: Query<(
        &Part,
        &mut Node,
        Option<&mut Text>,
        Option<&mut BackgroundColor>,
        Option<&mut UiTransform>,
    )>,
) {
    let width = windows.single().map_or(1440., Window::width);
    let route = (!review.blocked)
        .then(|| navigation.change_route_hud(&repo.0, &progress, time.elapsed_secs(), width))
        .flatten();
    for (part, mut node, text, background, transform) in &mut parts {
        let display = match (part, &route) {
            (_, None) => Display::None,
            (Part::Track, Some(route)) if route.stops.is_empty() => Display::None,
            (Part::Dot(position), Some(route)) => {
                let Some(stop) = route.stops.get(*position) else {
                    if node.display != Display::None {
                        node.display = Display::None;
                    }
                    continue;
                };
                if let Some(mut background) = background {
                    background.set_if_neq(BackgroundColor(match stop {
                        StopState::Pending => Color::srgb_u8(137, 153, 176),
                        StopState::Visited => Color::srgb_u8(91, 225, 187),
                        StopState::Current => Color::srgb_u8(255, 191, 104),
                    }));
                }
                if let Some(mut transform) = transform {
                    let size = Vec2::splat(if *stop == StopState::Current {
                        1.45
                    } else {
                        1.
                    });
                    if transform.scale != size {
                        transform.scale = size;
                    }
                }
                Display::Flex
            }
            (Part::Caption, Some(route)) => {
                if let Some(mut text) = text {
                    if text.0 != route.caption {
                        text.0.clone_from(&route.caption);
                    }
                }
                Display::Flex
            }
            (Part::Panel | Part::Track, Some(_)) => Display::Flex,
        };
        if node.display != display {
            node.display = display;
        }
    }
}
