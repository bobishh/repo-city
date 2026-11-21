use bevy::input_focus::tab_navigation::{TabGroup, TabNavigationPlugin};

use super::super::*;
use super::ADAPTERS;

#[derive(Resource)]
struct Launcher {
    args: Vec<String>,
    selection: Option<Choice>,
}

#[derive(Clone, Copy, Component)]
struct Choice(&'static str);

impl native_panel::Actions<Choice> for Launcher {
    fn activate(&mut self, choice: Choice) {
        self.selection = Some(choice);
    }
}

#[derive(Component)]
struct LaunchError;

pub(crate) fn run(args: Vec<String>) {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Repo City · Choose world".into(),
                resolution: (610, 372).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TabNavigationPlugin)
        .insert_resource(ClearColor(Color::srgb(0.05, 0.075, 0.09)))
        .insert_resource(Launcher {
            args,
            selection: None,
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                native_panel::keyboard_navigation,
                native_panel::focus_outline,
                launch,
            )
                .chain(),
        )
        .run();
}

fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    load_ui_font(&mut fonts);
    commands.spawn(Camera2d);
    commands
        .spawn((
            TabGroup::modal(),
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(13),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Choose world"),
                TextFont::from_font_size(24.),
                TextColor(native_panel::INK),
            ));
            for adapter in ADAPTERS {
                let set = WorldSet::load(Some(adapter.id)).expect("bundled world descriptor");
                native_panel::button::<_, Launcher>(
                    panel,
                    &set.name,
                    color(set.accent),
                    Choice(adapter.id),
                );
            }
            panel.spawn((
                LaunchError,
                Text::new(""),
                TextFont::from_font_size(15.),
                TextColor(Color::srgb(1., 0.58, 0.43)),
                Node {
                    min_height: px(24),
                    ..default()
                },
            ));
        });
}

fn launch(
    mut launcher: ResMut<Launcher>,
    mut errors: Query<&mut Text, With<LaunchError>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        commands.write_message(AppExit::Success);
        return;
    }
    let Some(choice) = launcher.selection.take() else {
        return;
    };
    let result = std::env::current_exe().and_then(|executable| {
        std::process::Command::new(executable)
            .args(world_args(&launcher.args, choice.0))
            .spawn()
    });
    match result {
        Ok(_) => {
            commands.write_message(AppExit::Success);
        }
        Err(error) => {
            if let Ok(mut text) = errors.single_mut() {
                text.0 = format!("Could not open the selected world. {error}");
            }
        }
    }
}

fn world_args(args: &[String], id: &str) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.len() + 2);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--world-picker" => {}
            "--world-set" => {
                args.next();
            }
            _ => forwarded.push(arg.clone()),
        }
    }
    forwarded.extend(["--world-set".to_owned(), id.to_owned()]);
    forwarded
}

#[test]
fn choosing_a_world_keeps_the_same_repository_revision_and_file() {
    let args = [
        "--repo",
        "/my checkout",
        "--rev",
        "HEAD",
        "--world-picker",
        "--world-set",
        "code-city",
        "--file",
        "a.rs",
    ]
    .map(str::to_owned);
    assert_eq!(
        world_args(&args, "greek-library"),
        [
            "--repo",
            "/my checkout",
            "--rev",
            "HEAD",
            "--file",
            "a.rs",
            "--world-set",
            "greek-library"
        ]
    );
}
