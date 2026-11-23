use super::*;

#[derive(Component)]
pub(crate) enum Part {
    Panel,
    Title,
    Path,
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
                        width: px(416.),
                        max_width: percent(92.),
                        padding: UiRect::all(px(12.)),
                        row_gap: px(5.),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.024, 0.066, 0.08, 0.95)),
                    bevy::picking::Pickable::IGNORE,
                ))
                .with_children(|panel| {
                    for (part, size) in [(Part::Title, 14.), (Part::Path, 11.)] {
                        panel.spawn((
                            part,
                            Text::new(""),
                            TextLayout::default().with_no_wrap(),
                            TextFont {
                                font_size: FontSize::Px(size),
                                ..font.clone()
                            },
                            TextColor(ink),
                            Node {
                                overflow: Overflow::clip(),
                                min_height: px(14.),
                                ..default()
                            },
                            bevy::picking::Pickable::IGNORE,
                        ));
                    }
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
                                    top: px(2.),
                                    width: percent(100.),
                                    height: px(1.),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb_u8(49, 70, 78)),
                                bevy::picking::Pickable::IGNORE,
                            ));
                            for position in 0..VISIBLE_STOPS {
                                track.spawn((
                                    Part::Dot(position),
                                    Node {
                                        height: px(5.),
                                        max_width: px(8.),
                                        flex_basis: px(8.),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
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
    mut parts: Query<(
        &Part,
        &mut Node,
        Option<&mut Text>,
        Option<&mut BackgroundColor>,
    )>,
) {
    let route = (!review.blocked)
        .then(|| navigation.change_route_hud(&repo.0, &progress, time.elapsed_secs()))
        .flatten();
    for (part, mut node, text, background) in &mut parts {
        let display = match (part, &route) {
            (_, None) => Display::None,
            (Part::Track, Some(route)) if route.stops.is_empty() => Display::None,
            (Part::Dot(position), Some(route)) => {
                let Some(stop) = route.stops.get(*position) else {
                    node.display = Display::None;
                    continue;
                };
                if let Some(mut background) = background {
                    background.set_if_neq(BackgroundColor(match stop {
                        StopState::Pending => Color::srgb_u8(49, 70, 78),
                        StopState::Visited => Color::srgb_u8(91, 192, 170),
                        StopState::Current => Color::srgb_u8(250, 190, 104),
                    }));
                }
                Display::Flex
            }
            (Part::Title | Part::Path, Some(route)) => {
                if let Some(mut text) = text {
                    let value = if matches!(part, Part::Title) {
                        &route.title
                    } else {
                        &route.path
                    };
                    if text.0 != *value {
                        text.0.clone_from(value);
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
