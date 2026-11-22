use super::*;
use bevy::input::mouse::MouseButtonInput;
use city_menu::Action;

#[derive(Default)]
pub(super) struct Gesture {
    drag: Option<f32>,
    aim: Option<aim::Target>,
    note: Option<String>,
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    wheel: Res<AccumulatedMouseScroll>,
    mut clicks: MessageReader<MouseButtonInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut gesture: Local<Gesture>,
    mut menu: ResMut<city_menu::Menu>,
    mut library: ResMut<Library>,
    (aim, nearest, mut map): (Res<aim::Aim>, Res<Nearest>, ResMut<map::Map>),
    (repo, explorer, mut review, mut filter): (
        Res<Repo>,
        Res<Explorer>,
        ResMut<review_notes::Review>,
        ResMut<file_facade::ChangesOnly>,
    ),
    mut view: city_menu::MenuView,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] mut preview: ResMut<markdown_preview::Request>,
) {
    menu.begin_frame();
    let blocked = repo.0.files.is_empty()
        || map.open()
        || review.blocked
        || review.tablet_open
        || library.input.is_some();
    #[cfg(not(target_arch = "wasm32"))]
    let blocked = blocked || picker.blocks_controls();
    if let Some(drag) = &mut gesture.drag {
        *drag += motion.delta.length();
    }
    let mut click = false;
    for event in clicks
        .read()
        .filter(|event| event.button == MouseButton::Right)
    {
        if event.state == ButtonState::Pressed {
            gesture.drag = (!blocked).then_some(0.);
        } else {
            click |= gesture.drag.take().is_some_and(|distance| distance < 6.);
        }
    }
    let toggle = click || city_menu::keyboard_menu(&keys);
    let screen = windows.single().ok().map(|window| {
        let size = Vec2::new(window.width(), window.height());
        nearest
            .screen
            .unwrap_or((window.cursor_position().unwrap_or(size / 2.), size))
    });

    if blocked
        || repo.is_changed()
        || (menu.is_open() && (toggle || keys.just_pressed(KeyCode::Escape)))
    {
        menu.close();
    } else if toggle && let Some((point, _)) = screen {
        gesture.aim = aim.0;
        gesture.note.clone_from(&review.aim_note);
        menu.open(point);
    }
    let viewport = screen.map_or(Vec2::ZERO, |(_, size)| size);
    if !menu.is_open() {
        view.draw(&menu, &[], viewport, filter.enabled);
        return;
    }

    let selected = match gesture.aim {
        Some(aim::Target::File(index) | aim::Target::Line { file: index, .. }) => Some(index),
        _ if library.reader.is_some() => Some(explorer.selected),
        _ => None,
    }
    .and_then(|index| repo.0.files.get(index).map(|file| (index, file)));
    let folder = match gesture.aim {
        Some(aim::Target::Cabinet(index)) => Some(index),
        _ => selected.and_then(|(_, file)| {
            library
                .catalog
                .folder(file.directory())
                .map(|(index, _, _)| index)
        }),
    };
    let available: Vec<_> = [
        (Action::Approach, selected.is_some()),
        (Action::Diff, selected.is_some()),
        (Action::Blame, selected.is_some()),
        (Action::Find, true),
        (Action::Paints, review.can_paint),
        (Action::Notes, true),
        (Action::FindText, library.reader.is_some()),
        (Action::ChangesOnly, filter.available),
        (Action::Folder, folder.is_some()),
        (Action::Preview, selected.is_some()),
    ]
    .into_iter()
    .filter_map(|(action, shown)| shown.then_some(action))
    .collect();

    if menu.is_open() {
        menu.select(&wheel, &keys, available.len());
        let shortcut = [
            (KeyCode::KeyE, Action::Approach),
            (KeyCode::KeyG, Action::Diff),
            (KeyCode::KeyB, Action::Blame),
            (KeyCode::KeyU, Action::Paints),
            (KeyCode::KeyJ, Action::Notes),
            (KeyCode::Slash, Action::FindText),
            (KeyCode::KeyF, Action::ChangesOnly),
            (KeyCode::KeyM, Action::Folder),
            (KeyCode::KeyP, Action::Preview),
        ]
        .into_iter()
        .find_map(|(key, action)| keys.just_pressed(key).then_some(action));
        let choice = shortcut.or_else(|| {
            if mouse.just_pressed(MouseButton::Left) {
                view.hovered()
            } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
                menu.selected_action(&available)
            } else {
                None
            }
        });
        if let Some(action) = choice.filter(|action| available.contains(action)) {
            match action {
                Action::Find => {
                    library.catalog.query.clear();
                    library
                        .catalog
                        .collect(&repo.0, Some(explorer.selected), &filter);
                    library.input = Some(Input::Catalog);
                }
                Action::FindText => library.input = Some(Input::Text(String::new())),
                Action::Paints => review.request = Some(review_notes::Panel::Paints),
                Action::Notes => {
                    review.aim_note.clone_from(&gesture.note);
                    review.request = Some(review_notes::Panel::Aimed);
                }
                Action::ChangesOnly => filter.toggle(),
                Action::Folder => map.show_folder(folder.expect("available folder")),
                Action::Preview => {
                    let (index, file) = selected.expect("available file");
                    let same = index == explorer.selected && library.reader.is_some();
                    let before = if same {
                        explorer.before
                    } else {
                        file.after.is_none()
                    };
                    let range = library
                        .reader
                        .as_ref()
                        .filter(|_| same)
                        .map_or(0..SCROLL_ROWS, |reader| {
                            reader.page(explorer.highlighted.len())
                        });
                    #[cfg(target_arch = "wasm32")]
                    markdown_preview::open_browser(
                        &file.path,
                        before,
                        range.start,
                        range.end,
                        false,
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        preview.0 = Some((index, before, range, false));
                    }
                }
                Action::Approach | Action::Diff | Action::Blame => {
                    let (index, file) = selected.expect("available file");
                    let (before, line) = match gesture.aim {
                        Some(aim::Target::Line { before, number, .. }) => (before, number),
                        _ => (file.after.is_none(), 1),
                    };
                    library.destination = Some(Destination {
                        index,
                        before,
                        line,
                        end: None,
                        face: match action {
                            Action::Diff => listing::Face::Diff,
                            Action::Blame => listing::Face::Blame,
                            _ => listing::Face::Source,
                        },
                    });
                }
            }
            menu.close();
        } else if mouse.just_pressed(MouseButton::Left) {
            menu.close();
        }
    }
    view.draw(&menu, &available, viewport, filter.enabled);
}
