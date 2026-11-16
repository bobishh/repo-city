use bevy::{
    input::keyboard::KeyboardInput,
    picking::hover::Hovered,
    ui_widgets::{Activate, Button, observe},
};

use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) const SHORTCUT: &str = "P";
#[cfg(not(target_arch = "wasm32"))]
pub(super) const SHORTCUT: &str = "Cmd+P";

const PAGE_SIZE: usize = 7;

#[derive(Resource, Default)]
pub(super) struct FilePicker {
    open: bool,
    suppress_input: bool,
    query: String,
    results: Vec<usize>,
    selected: usize,
    pub(super) destination: Option<usize>,
}

impl FilePicker {
    pub(super) fn is_open(&self) -> bool {
        self.open || self.suppress_input
    }
    fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }
    fn choose(&mut self, result: usize) {
        if let Some(&index) = self.results.get(result) {
            self.destination = Some(index);
            self.open = false;
        }
    }
}

fn rank(path: &str, query: &str) -> Option<usize> {
    let path = path.to_lowercase();
    let name = path.rsplit('/').next().unwrap_or(&path);
    if query.is_empty() || name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.contains(query) {
        return Some(2);
    }
    if path.contains(query) {
        return Some(3);
    }
    let mut chars = path.chars();
    query
        .chars()
        .all(|needle| chars.any(|c| c == needle))
        .then_some(4)
}

#[derive(Component)]
pub(super) struct PickerQuery;
#[derive(Component)]
pub(super) struct PickerHint;

#[derive(Component)]
pub(super) struct PickerPanel;
#[derive(Component)]
pub(super) struct FileRow(usize);
#[derive(Component)]
pub(super) struct FileButton;
#[derive(Component)]
pub(super) struct FileControls;

pub(super) fn spawn(commands: &mut Commands, font: &TextFont, ink: Color) {
    commands
        .spawn((
            FileControls,
            Node {
                position_type: PositionType::Absolute,
                top: px(if cfg!(target_arch = "wasm32") {
                    125.
                } else {
                    77.
                }),
                left: px(if cfg!(target_arch = "wasm32") {
                    24.
                } else {
                    190.
                }),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.),
                ..default()
            },
        ))
        .with_children(|ui| {
            ui.spawn((
                button(
                    &format!("Files  {SHORTCUT}"),
                    font,
                    ink,
                    AlignSelf::FlexStart,
                ),
                observe(|_: On<Activate>, mut picker: ResMut<FilePicker>| picker.toggle()),
            ));
            ui.spawn((
                PickerPanel,
                Node {
                    display: Display::None,
                    width: px(570.),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3.),
                    padding: UiRect::all(px(6.)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.025, 0.055, 0.07, 0.98)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    PickerQuery,
                    Text::new(""),
                    font.clone(),
                    TextColor(ink),
                    BackgroundColor(Color::srgb(0.10, 0.19, 0.22)),
                    Node {
                        padding: UiRect::all(px(10.)),
                        ..default()
                    },
                ));
                for row in 0..PAGE_SIZE {
                    panel.spawn((
                        button("", font, ink, AlignSelf::Stretch),
                        FileRow(row),
                        observe(
                            |event: On<Activate>,
                             rows: Query<&FileRow>,
                             mut picker: ResMut<FilePicker>| {
                                if let Ok(row) = rows.get(event.entity) {
                                    let first = picker.selected / PAGE_SIZE * PAGE_SIZE;
                                    picker.choose(first + row.0);
                                }
                            },
                        ),
                    ));
                }
                panel.spawn((
                    PickerHint,
                    Text::new(""),
                    font.clone(),
                    TextColor(ink),
                    Node {
                        padding: UiRect::all(px(7.)),
                        ..default()
                    },
                ));
            });
        });
}

pub(super) fn button(
    title: &str,
    font: &TextFont,
    ink: Color,
    align_self: AlignSelf,
) -> impl Bundle {
    (
        Button,
        FileButton,
        Hovered::default(),
        Node {
            min_height: px(29.),
            align_self,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(10.), px(4.)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.055, 0.13, 0.15)),
        children![(Text::new(title), font.clone(), TextColor(ink))],
    )
}

pub(super) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    mut picker: ResMut<FilePicker>,
    repo: Res<Repo>,
    mut pilot: Query<&mut Pilot>,
    mut panels: Query<&mut Node, (With<PickerPanel>, Without<FileRow>)>,
    mut rows: Query<
        (
            &FileRow,
            &Children,
            &mut Node,
            &mut BackgroundColor,
            &Hovered,
        ),
        Without<PickerPanel>,
    >,
    mut labels: Query<(&mut Text, Has<PickerQuery>, Has<PickerHint>)>,
) {
    picker.suppress_input = false;
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let shortcut = keys.just_pressed(KeyCode::KeyP)
        && if cfg!(target_arch = "wasm32") {
            !command && !picker.open
        } else {
            command
        };
    if shortcut {
        picker.toggle();
        // An earlier key in the same frame belongs to the view before opening.
        let _ = events
            .read()
            .find(|event| event.state.is_pressed() && event.key_code == KeyCode::KeyP);
    }
    if !picker.open {
        events.clear();
    }
    if picker.open {
        if let Ok(mut pilot) = pilot.single_mut() {
            pilot.captured = false;
        }
        let old_query = picker.query.clone();
        let mut accept = false;
        let mut shift = 0isize;
        for event in events.read().filter(|event| event.state.is_pressed()) {
            match event.key_code {
                KeyCode::Escape => {
                    picker.open = false;
                    picker.suppress_input = true;
                    break;
                }
                KeyCode::ArrowDown => shift += 1,
                KeyCode::ArrowUp => shift -= 1,
                KeyCode::PageDown => shift += PAGE_SIZE as isize,
                KeyCode::PageUp => shift -= PAGE_SIZE as isize,
                KeyCode::Enter | KeyCode::NumpadEnter => accept = true,
                KeyCode::KeyA if command => picker.query.clear(),
                KeyCode::Backspace => {
                    picker.query.pop();
                }
                _ if !command && let Some(value) = &event.text => {
                    let remaining = 256usize.saturating_sub(picker.query.chars().count());
                    picker
                        .query
                        .extend(value.chars().filter(|c| !c.is_control()).take(remaining));
                }
                _ => {}
            }
        }
        if picker.query != old_query {
            picker.selected = 0;
        }
        let query = picker.query.trim().to_lowercase();
        let mut matches = repo
            .0
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| rank(&file.path, &query).map(|score| (score, index)))
            .collect::<Vec<_>>();
        matches.sort_unstable();
        picker.results = matches.into_iter().map(|(_, index)| index).collect();
        picker.selected = picker
            .selected
            .saturating_add_signed(shift)
            .min(picker.results.len().saturating_sub(1));
        if accept && picker.open {
            let index = picker.selected;
            picker.choose(index);
        }
    }
    for mut panel in &mut panels {
        let display = if picker.open {
            Display::Flex
        } else {
            Display::None
        };
        if panel.display != display {
            panel.display = display;
        }
    }
    if !picker.open {
        return;
    }
    for (mut text, query, hint) in &mut labels {
        let value = if query {
            if picker.query.is_empty() {
                "Type a file name or path".into()
            } else {
                format!("{}▏", picker.query)
            }
        } else if hint {
            if picker.results.is_empty() {
                "No matching files".into()
            } else {
                format!(
                    "{} / {}   Up / Down select · Enter flies to file",
                    picker.selected + 1,
                    picker.results.len()
                )
            }
        } else {
            continue;
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    let first = picker.selected / PAGE_SIZE * PAGE_SIZE;
    for (row, children, mut node, mut background, hovered) in &mut rows {
        let result = first + row.0;
        let file = picker
            .results
            .get(result)
            .map(|&index| &repo.0.files[index]);
        let display = if file.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let color = if result == picker.selected || hovered.get() {
            Color::srgb(0.13, 0.29, 0.32)
        } else {
            Color::srgb(0.055, 0.13, 0.15)
        };
        if background.0 != color {
            background.0 = color;
        }
        if let Some(file) = file
            && let Ok((mut label, _, _)) = labels.get_mut(children[0])
            && label.0 != file.path
        {
            label.0 = file.path.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_search_prefers_filename_and_accepts_paths_and_subsequences() {
        assert!(rank("src/foo/parser.rs", "parser") < rank("parser/test.rs", "parser"));
        assert_eq!(rank("src/foo/parser.rs", "foo/par"), Some(3));
        assert_eq!(rank("src/foo/parser.rs", "psr"), Some(4));
        assert_eq!(rank("src/foo/parser.rs", "missing"), None);
    }
}
