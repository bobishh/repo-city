//! A held inspection view leaves the pilot's navigation camera and reading position in place.
use super::*;

#[derive(Component)]
pub(crate) struct ExteriorCamera;

#[derive(Resource)]
pub(crate) struct Exterior {
    craft: Entity,
    showing: bool,
}

pub(crate) fn piloting(view: Option<Res<Exterior>>) -> bool {
    view.is_none_or(|view| !view.showing)
}

pub(super) fn install(app: &mut App) {
    let inspect = (select_view, pilot::animate)
        .chain()
        .before(file_facade::lower_unchanged);
    #[cfg(target_arch = "wasm32")]
    let inspect = inspect.after(browser_touch::update);
    app.add_systems(PostStartup, launch)
        .add_systems(Update, inspect);
}

fn launch(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    set: Res<WorldSet>,
) {
    let craft = pilot::spawn(&mut commands, &mut meshes, &mut materials);
    commands.spawn((
        ChildOf(craft),
        PointLight {
            color: Color::srgb(0.89, 0.85, 1.),
            intensity: 35_000.,
            range: 7.,
            ..default()
        },
        Transform::from_xyz(-2., 2., -2.7),
    ));
    commands.spawn((
        ExteriorCamera,
        Camera3d::default(),
        Camera {
            is_active: false,
            order: 1,
            ..default()
        },
        if mobile_scene() {
            Msaa::Off
        } else {
            Msaa::Sample4
        },
        DistanceFog {
            color: color(set.background),
            falloff: FogFalloff::Exponential { density: 0.014 },
            ..default()
        },
        Transform::default(),
    ));
    commands.insert_resource(Exterior {
        craft,
        showing: false,
    });
}

fn select_view(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    repo: Res<Repo>,
    navigation: Res<city_navigation::Navigation>,
    review: Res<review_notes::Review>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    search: Res<listing::find::Search>,
    #[cfg(not(target_arch = "wasm32"))] repository: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] session: Option<Res<agent_session::Session>>,
    mut view: ResMut<Exterior>,
    mut thrust: ResMut<Thrusters>,
    mut eye: Single<(&mut Camera, &Transform), PlayerCamera>,
    mut outside: Single<(&mut Camera, &mut Transform), (With<ExteriorCamera>, With<Camera3d>)>,
    mut bodies: Query<(&mut Transform, &mut Visibility), Without<Camera3d>>,
) {
    let show = keys.pressed(KeyCode::KeyI)
        && !keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ])
        && !repo.0.files.is_empty()
        && windows.single().is_ok_and(|window| window.focused)
        && !navigation.guides_camera()
        && !review.blocked
        && !review.tablet_open
        && !picker.is_open()
        && !menu.blocks_controls()
        && !search.blocks_controls();
    #[cfg(not(target_arch = "wasm32"))]
    let show = show && !repository.blocks_controls() && city_accepts_input(session);
    if show == view.showing {
        return;
    }
    let Ok((mut pose, mut visible)) = bodies.get_mut(view.craft) else {
        return;
    };
    let (main, ship) = &mut *eye;
    let (external, portrait) = &mut *outside;
    if show {
        thrust.stop();
        *pose = **ship;
        pose.translation = ship.transform_point(-scene::PILOT_EYE);
        let target = pose.transform_point(Vec3::new(0., 0.12, 0.));
        let mut position = pose.transform_point(Vec3::new(2.8, 1.35, -4.8));
        position.y = position.y.max(0.7);
        **portrait = Transform::from_translation(position).looking_at(target, Vec3::Y);
    }
    main.is_active = !show;
    external.is_active = show;
    *visible = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    view.showing = show;
    #[cfg(target_arch = "wasm32")]
    if let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("repo-city"))
    {
        if show {
            let _ = canvas.set_attribute("data-abyss-exterior", "");
        } else {
            let _ = canvas.remove_attribute("data-abyss-exterior");
        }
    }
}
