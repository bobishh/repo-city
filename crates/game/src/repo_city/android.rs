//! Articulated android, rendered by the district mirror's small camera.
use super::*;
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster};

pub(super) const LAYER: usize = 7;
pub(super) const TABLET_LAYER: usize = 8;
pub(super) const TABLET_CASE: Vec2 = Vec2::new(1.15, 0.77);
pub(super) const TABLET_SCREEN: Vec2 = Vec2::new(1.03, 0.65);
pub(super) const TABLET_SIZE: UVec2 = UVec2::new(1440, 909);
const SPRAYER_TIP: f32 = 0.23;

fn sprayer_view(raised: f32, can: bool) -> Transform {
    let ease = raised * raised * (3. - 2. * raised);
    let (x, y, scale) = if can {
        (0.24, -0.48 + 0.22 * ease, 0.61)
    } else {
        (0.31, -0.39 + 0.05 * ease, 0.34)
    };
    Transform::from_xyz(x, y, -0.76)
        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI) * Quat::from_rotation_z(-0.18))
        .with_scale(Vec3::splat(scale))
}

#[derive(Component, Clone, Copy)]
pub(super) enum Joint {
    Body,
    Head,
    Shoulder(f32),
    Elbow(f32),
    Hip(f32),
    Knee(f32),
    Thruster(f32),
}

#[derive(Clone, Copy)]
enum Paint {
    Metal,
    Dark,
    Shirt,
    Sole,
    Lamp,
    Blue,
    Jet,
    JetCore,
}

#[derive(Clone)]
struct Workshop {
    block: Handle<Mesh>,
    ball: Handle<Mesh>,
    pin: Handle<Mesh>,
    paint: [Handle<StandardMaterial>; 8],
    layer: usize,
}

impl Workshop {
    fn piece(
        &self,
        commands: &mut Commands,
        parent: Entity,
        mesh: &Handle<Mesh>,
        paint: Paint,
        position: [f32; 3],
        size: [f32; 3],
        rotation: Quat,
    ) {
        commands.spawn((
            ChildOf(parent),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(self.paint[paint as usize].clone()),
            Transform::from_translation(position.into())
                .with_scale(size.into())
                .with_rotation(rotation),
            RenderLayers::layer(self.layer),
            NotShadowCaster,
        ));
    }

    fn block(
        &self,
        commands: &mut Commands,
        parent: Entity,
        paint: Paint,
        position: [f32; 3],
        size: [f32; 3],
    ) {
        self.piece(
            commands,
            parent,
            &self.block,
            paint,
            position,
            size,
            Quat::IDENTITY,
        );
    }

    fn joint(
        &self,
        commands: &mut Commands,
        parent: Entity,
        place: [f32; 3],
        joint: Joint,
    ) -> Entity {
        commands
            .spawn((
                ChildOf(parent),
                Transform::from_translation(place.into()),
                Visibility::default(),
                joint,
            ))
            .id()
    }

    fn can(&self, commands: &mut Commands, parent: Entity, pigment: &Handle<StandardMaterial>) {
        let mesh = |commands: &mut Commands,
                    paint: Handle<StandardMaterial>,
                    position: Vec3,
                    size: Vec3| {
            commands.spawn((
                ChildOf(parent),
                Mesh3d(self.pin.clone()),
                MeshMaterial3d(paint),
                Transform::from_translation(position).with_scale(size),
                RenderLayers::layer(self.layer),
                NotShadowCaster,
            ));
        };
        mesh(
            commands,
            pigment.clone(),
            Vec3::new(0., -0.061, 0.),
            Vec3::new(0.145, 0.235, 0.145),
        );
        mesh(
            commands,
            self.paint[Paint::Metal as usize].clone(),
            Vec3::Y * 0.06,
            Vec3::new(0.145, 0.024, 0.145),
        );
        mesh(
            commands,
            self.paint[Paint::Dark as usize].clone(),
            Vec3::Y * 0.091,
            Vec3::new(0.082, 0.042, 0.082),
        );
        self.block(
            commands,
            parent,
            Paint::Lamp,
            [0., 0.088, 0.045],
            [0.04, 0.018, 0.02],
        );
    }

    fn sprayer(&self, commands: &mut Commands, parent: Entity) {
        use Paint::*;
        let along_barrel = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        self.block(
            commands,
            parent,
            Dark,
            [0., -0.076, -0.03],
            [0.072, 0.153, 0.079],
        );
        self.block(
            commands,
            parent,
            Dark,
            [0., 0., -0.023],
            [0.15, 0.112, 0.26],
        );
        self.block(
            commands,
            parent,
            Lamp,
            [0., 0.03, -0.14],
            [0.10, 0.06, 0.06],
        );
        self.block(
            commands,
            parent,
            Metal,
            [0., -0.038, 0.057],
            [0.028, 0.069, 0.026],
        );
        self.piece(
            commands,
            parent,
            &self.pin,
            Dark,
            [0., 0., 0.14],
            [0.09, 0.126, 0.09],
            along_barrel,
        );
        self.piece(
            commands,
            parent,
            &self.pin,
            Metal,
            [0., 0., SPRAYER_TIP - 0.016],
            [0.108, 0.032, 0.108],
            along_barrel,
        );
        self.piece(
            commands,
            parent,
            &self.pin,
            Dark,
            [0., -0.118, -0.101],
            [0.103, 0.116, 0.103],
            Quat::IDENTITY,
        );
        self.block(
            commands,
            parent,
            Lamp,
            [0., -0.15, -0.049],
            [0.038, 0.037, 0.007],
        );
    }
}

#[derive(Component)]
pub(super) struct ViewSprayer(bool);
#[derive(Component)]
pub(super) struct MirrorSprayer(bool);
#[derive(Component)]
pub(super) struct MirrorTablet;

#[derive(Component)]
pub(super) struct ViewTablet;

#[derive(Component)]
pub(super) struct TabletCamera;

#[derive(Resource)]
pub(super) struct CanPigment(Handle<StandardMaterial>);

pub(super) fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    pilot: Entity,
) {
    use Paint::*;
    let colors = [
        [0.71, 0.59, 0.45],
        [0.10, 0.13, 0.19],
        [0.87, 0.23, 0.40],
        [0.91, 0.81, 0.62],
        [1.0, 0.81, 0.27],
        [0.24, 0.62, 0.65],
        [0.21, 0.65, 1.0],
        [0.74, 0.96, 1.0],
    ];
    let w = Workshop {
        block: meshes.add(shapes::rounded_box(Vec3::ONE, 0.10)),
        ball: meshes.add(Sphere::new(0.5).mesh().uv(12, 8)),
        pin: meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(12)),
        paint: std::array::from_fn(|index| {
            let [r, g, b] = colors[index];
            let outer_jet = index == Jet as usize;
            materials.add(StandardMaterial {
                base_color: Color::srgba(r, g, b, if outer_jet { 0.48 } else { 1. }),
                unlit: index >= Jet as usize,
                alpha_mode: if outer_jet {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                metallic: 0.14,
                perceptual_roughness: 0.69,
                emissive: LinearRgba::new(r * 0.13, g * 0.13, b * 0.13, 1.),
                ..default()
            })
        }),
        layer: LAYER,
    };
    let pigment = materials.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    commands.insert_resource(CanPigment(pigment.clone()));
    let plume = meshes.add(Cone::new(0.5, 1.).mesh().resolution(12));
    let body = commands
        .spawn((Joint::Body, Transform::default(), Visibility::default()))
        .id();
    w.block(commands, body, Dark, [0., -0.41, 0.0], [0.68, 0.34, 0.46]);
    w.block(commands, body, Metal, [0., 0.29, 0.0], [0.58, 1.16, 0.43]);
    w.block(commands, body, Dark, [0., -0.21, 0.038], [0.72, 0.14, 0.48]);
    w.block(commands, body, Sole, [0., -0.21, 0.30], [0.16, 0.11, 0.045]);
    w.block(commands, body, Metal, [0., 1.04, 0.02], [0.15, 0.43, 0.16]);
    for side in [-1., 1.] {
        w.piece(
            commands,
            body,
            &w.block,
            Shirt,
            [side * 0.25, 0.33, 0.11],
            [0.36, 1.03, 0.51],
            Quat::from_rotation_z(side * 0.06),
        );
        for (dx, y, angle) in [(0.20, 0.62, 0.58), (0.31, 0.38, -0.80), (0.16, 0.12, 0.46)] {
            w.piece(
                commands,
                body,
                &w.block,
                Dark,
                [side * dx, y, 0.371],
                [0.15, 0.045, 0.03],
                Quat::from_rotation_z(angle * side),
            );
        }
    }
    let tablet = commands
        .spawn((
            ChildOf(body),
            MirrorTablet,
            Transform::from_xyz(0., 0.25, 1.02).with_rotation(Quat::from_rotation_x(-0.22)),
            Visibility::Hidden,
        ))
        .id();
    tablet_case(&w, commands, tablet);
    let head = w.joint(commands, body, [0.02, 1.18, 0.02], Joint::Head);
    w.block(commands, head, Metal, [0., 0.47, 0.], [0.69, 0.88, 0.51]);
    w.block(commands, head, Dark, [0., 0.53, 0.268], [0.64, 0.27, 0.11]);
    w.block(
        commands,
        head,
        Blue,
        [-0.32, 0.54, 0.081],
        [0.12, 0.29, 0.28],
    );
    w.block(
        commands,
        head,
        Lamp,
        [-0.14, 0.55, 0.328],
        [0.16, 0.083, 0.033],
    );
    w.block(
        commands,
        head,
        Lamp,
        [0.13, 0.53, 0.328],
        [0.14, 0.060, 0.033],
    );
    w.piece(
        commands,
        head,
        &w.pin,
        Metal,
        [-0.33, 0.96, 0.],
        [0.045, 0.42, 0.045],
        Quat::from_rotation_z(0.38),
    );
    w.piece(
        commands,
        head,
        &w.ball,
        Lamp,
        [-0.41, 1.16, 0.],
        [0.10; 3],
        Quat::IDENTITY,
    );
    for side in [-1., 1.] {
        let arm = w.joint(
            commands,
            body,
            [side * 0.52, 0.74, 0.],
            Joint::Shoulder(side),
        );
        w.block(
            commands,
            arm,
            Shirt,
            [side * 0.045, -0.13, 0.],
            [0.34, 0.34, 0.42],
        );
        w.block(commands, arm, Metal, [0., -0.49, 0.], [0.13, 0.55, 0.15]);
        let elbow = w.joint(commands, arm, [0., -0.75, 0.], Joint::Elbow(side));
        w.piece(
            commands,
            elbow,
            &w.ball,
            Dark,
            [0.; 3],
            [0.23; 3],
            Quat::IDENTITY,
        );
        w.block(commands, elbow, Metal, [0., -0.31, 0.], [0.11, 0.59, 0.13]);
        w.block(
            commands,
            elbow,
            Metal,
            [0., -0.64, 0.02],
            [0.22, 0.22, 0.14],
        );
        for finger in 0..if side < 0. { 2 } else { 3 } {
            w.block(
                commands,
                elbow,
                Sole,
                [-0.085 + finger as f32 * 0.077, -0.78, 0.041],
                [0.05, 0.13, 0.048],
            );
        }
        if side > 0. {
            let grip = commands
                .spawn((
                    ChildOf(elbow),
                    Transform::from_xyz(0.025, -0.62, 0.12)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::splat(1.8)),
                    Visibility::default(),
                ))
                .id();
            let water = commands
                .spawn((
                    ChildOf(grip),
                    Transform::default(),
                    Visibility::Hidden,
                    MirrorSprayer(false),
                ))
                .id();
            let can = commands
                .spawn((
                    ChildOf(grip),
                    Transform::default(),
                    Visibility::Hidden,
                    MirrorSprayer(true),
                ))
                .id();
            w.sprayer(commands, water);
            w.can(commands, can, &pigment);
        }

        let hip = w.joint(commands, body, [side * 0.22, -0.51, 0.], Joint::Hip(side));
        w.block(commands, hip, Metal, [0., -0.36, 0.], [0.15, 0.73, 0.21]);
        let knee = w.joint(commands, hip, [0., -0.74, 0.], Joint::Knee(side));
        w.piece(
            commands,
            knee,
            &w.ball,
            Blue,
            [0.; 3],
            [0.26; 3],
            Quat::IDENTITY,
        );
        w.block(commands, knee, Metal, [0., -0.38, 0.], [0.105, 0.69, 0.13]);
        w.block(commands, knee, Sole, [0., -0.76, 0.12], [0.29, 0.22, 0.62]);
        w.block(commands, knee, Blue, [0., -0.84, 0.12], [0.30, 0.068, 0.63]);
        w.block(
            commands,
            knee,
            Shirt,
            [0., -0.65, 0.22],
            [0.25, 0.035, 0.07],
        );
        w.piece(
            commands,
            knee,
            &w.pin,
            Dark,
            [0., -0.89, -0.03],
            [0.25, 0.16, 0.25],
            Quat::IDENTITY,
        );
        w.piece(
            commands,
            knee,
            &w.pin,
            Metal,
            [0., -0.957, -0.03],
            [0.28, 0.045, 0.28],
            Quat::IDENTITY,
        );
        let jet = w.joint(commands, knee, [0., -0.98, -0.03], Joint::Thruster(side));
        let downward = Quat::from_rotation_z(std::f32::consts::PI);
        w.piece(
            commands,
            jet,
            &plume,
            Jet,
            [0., -0.27, 0.],
            [0.23, 0.54, 0.23],
            downward,
        );
        w.piece(
            commands,
            jet,
            &plume,
            JetCore,
            [0., -0.15, 0.],
            [0.16, 0.30, 0.16],
            downward,
        );
    }
    let view = commands
        .spawn((
            ChildOf(pilot),
            ViewSprayer(false),
            sprayer_view(0., false),
            Visibility::Hidden,
        ))
        .id();
    let first_person = Workshop { layer: 0, ..w };
    let tablet = commands
        .spawn((
            ChildOf(pilot),
            ViewTablet,
            Transform::from_xyz(0., -0.9, -0.95),
            Visibility::Hidden,
        ))
        .id();
    {
        let case = Workshop {
            layer: TABLET_LAYER,
            ..first_person.clone()
        };
        let size = TABLET_CASE;
        case.block(commands, tablet, Dark, [0.; 3], [size.x, size.y, 0.105]);
        case.block(
            commands,
            tablet,
            Dark,
            [0., 0., 0.025],
            [size.x * 0.995, size.y * 0.99, 0.050],
        );
        for side in [-1., 1.] {
            tablet_grip(&case, commands, tablet, side);
        }
        tablet_display::spawn(
            commands,
            pilot,
            tablet,
            images,
            meshes,
            materials,
            tablet_display::Case::Metal,
        );
    }
    let can = commands
        .spawn((
            ChildOf(pilot),
            ViewSprayer(true),
            sprayer_view(0., true),
            Visibility::Hidden,
        ))
        .id();
    first_person.can(commands, can, &pigment);
    first_person.block(
        commands,
        can,
        Metal,
        [-0.04, -0.105, -0.025],
        [0.09, 0.085, 0.095],
    );
    first_person.sprayer(commands, view);
    first_person.block(
        commands,
        view,
        Metal,
        [-0.045, -0.115, -0.032],
        [0.09, 0.104, 0.104],
    );
    first_person.block(
        commands,
        view,
        Dark,
        [-0.054, -0.28, -0.076],
        [0.092, 0.26, 0.091],
    );
    for finger in 0..3 {
        first_person.block(
            commands,
            view,
            Sole,
            [0.019, -0.08 - finger as f32 * 0.028, 0.012],
            [0.078, 0.018, 0.028],
        );
    }
}

fn tablet_case(w: &Workshop, commands: &mut Commands, tablet: Entity) {
    use Paint::*;
    w.block(commands, tablet, Dark, [0.; 3], [1.15, 0.71, 0.085]);
    w.block(commands, tablet, Blue, [0., 0., 0.044], [1.07, 0.63, 0.012]);
    w.block(commands, tablet, Dark, [0., 0., 0.052], [0.99, 0.55, 0.012]);
    w.block(
        commands,
        tablet,
        Lamp,
        [0.49, -0.30, -0.048],
        [0.042, 0.015, 0.01],
    );
}

fn tablet_grip(w: &Workshop, commands: &mut Commands, tablet: Entity, side: f32) {
    use Paint::*;
    let pivot = commands
        .spawn((
            ChildOf(tablet),
            Transform::from_xyz(side * 0.579, -0.238, -0.015)
                .with_rotation(Quat::from_rotation_z(side * -0.16)),
            Visibility::Inherited,
        ))
        .id();
    w.block(
        commands,
        pivot,
        Dark,
        [side * 0.009, -0.166, 0.063],
        [0.088, 0.143, 0.084],
    );
    w.block(
        commands,
        pivot,
        Metal,
        [side * 0.012, -0.089, 0.045],
        [0.074, 0.082, 0.077],
    );
    w.piece(
        commands,
        pivot,
        &w.ball,
        Dark,
        [0., -0.027, 0.029],
        [0.128, 0.127, 0.105],
        Quat::IDENTITY,
    );
    // Fingers wrap behind the case; the thumb supports the front bezel.
    for (y, reach) in [
        (0.046, 0.091),
        (0.010, 0.112),
        (-0.026, 0.103),
        (-0.057, 0.076),
    ] {
        w.piece(
            commands,
            pivot,
            &w.ball,
            Dark,
            [side * 0.012, y, -0.024],
            [0.052, 0.032, 0.046],
            Quat::IDENTITY,
        );
        w.piece(
            commands,
            pivot,
            &w.block,
            Metal,
            [-side * (reach * 0.5 - 0.012), y, -0.067],
            [reach, 0.027, 0.035],
            Quat::from_rotation_z(side * 0.09),
        );
    }
    w.piece(
        commands,
        pivot,
        &w.ball,
        Dark,
        [-side * 0.014, -0.046, 0.055],
        [0.074, 0.068, 0.066],
        Quat::IDENTITY,
    );
    w.piece(
        commands,
        pivot,
        &w.block,
        Metal,
        [-side * 0.044, -0.019, 0.09],
        [0.085, 0.035, 0.04],
        Quat::from_rotation_z(-side * 0.61),
    );
}

pub(super) fn show_tablet(
    review: Res<review_notes::Review>,
    time: Res<Time>,
    mut amount: Local<f32>,
    mut mirror: Query<&mut Visibility, (With<MirrorTablet>, Without<ViewTablet>)>,
    mut view: Query<(&mut Visibility, &mut Transform), With<ViewTablet>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let direction = if review.tablet_open { 1. } else { -1. };
    *amount = (*amount + direction * time.delta_secs() / 0.38).clamp(0., 1.);
    for mut visibility in &mut mirror {
        visibility.set_if_neq(if review.tablet_open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let t = *amount;
    for (mut visibility, mut transform) in &mut view {
        let shown = t > 0.;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if shown {
            let aspect = window
                .single()
                .map_or(1.6, |window| window.width() / window.height().max(1.));
            transform.set_if_neq(tablet_pose(t, aspect));
        }
    }
}

pub(super) fn tablet_pose(amount: f32, aspect: f32) -> Transform {
    let distance = (TABLET_CASE.x / aspect.max(0.1)).max(TABLET_CASE.y)
        / (2. * std::f32::consts::FRAC_PI_8.tan() * 0.95);
    let remaining = 1. - amount * amount * (3. - 2. * amount);
    Transform::from_xyz(0., -remaining * 0.95, -distance - 0.08).with_rotation(Quat::from_euler(
        EulerRot::XYZ,
        -remaining * 0.6,
        remaining * 0.24,
        remaining * 0.13,
    ))
}

pub(super) fn tablet_corners(pose: &Transform, viewport: Vec2) -> Option<[f32; 8]> {
    let mut corners = [0.; 8];
    let perspective = std::f32::consts::FRAC_PI_8.tan();
    for (index, [x, y]) in [[-1., 1.], [1., 1.], [1., -1.], [-1., -1.]]
        .into_iter()
        .enumerate()
    {
        let point = pose.transform_point(Vec3::new(
            x * TABLET_SCREEN.x / 2.,
            y * TABLET_SCREEN.y / 2. + 0.012,
            0.054,
        ));
        if point.z >= -0.01 {
            return None;
        }
        let scale = viewport.y / (-point.z * perspective * 2.);
        corners[index * 2] = viewport.x / 2. + point.x * scale;
        corners[index * 2 + 1] = viewport.y / 2. - point.y * scale;
    }
    Some(corners)
}

pub(super) fn spray_origin(camera: &Transform, raised: f32, can: bool) -> Vec3 {
    let tip = if can {
        Vec3::new(0., 0.088, 0.055)
    } else {
        Vec3::Z * SPRAYER_TIP
    };
    camera.transform_point(sprayer_view(raised, can).transform_point(tip))
}

pub(super) fn show_sprayer(
    review: Res<review_notes::Review>,
    navigation: Res<city_navigation::Navigation>,
    pigment: Option<Res<CanPigment>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut view: Query<(&ViewSprayer, &mut Transform, &mut Visibility)>,
    mut mirror: Query<(&MirrorSprayer, &mut Visibility), Without<ViewSprayer>>,
) {
    let paint = review.paint();
    let can = paint.is_some();
    let raised = if navigation.guides_camera() || review.tablet_open {
        0.
    } else {
        review.nozzle
    };
    let shown = |is_can| {
        if is_can == can && raised > 0.12 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (tool, mut pose, mut visibility) in &mut view {
        visibility.set_if_neq(shown(tool.0));
        if raised > 0. {
            pose.set_if_neq(sprayer_view(raised, tool.0));
        }
    }
    for (tool, mut visibility) in &mut mirror {
        visibility.set_if_neq(shown(tool.0));
    }
    if let (Some(preset), Some(pigment)) = (paint, pigment) {
        let [r, g, b] = preset.color;
        let color = Color::srgb_u8(r, g, b);
        if materials
            .get(&pigment.0)
            .is_some_and(|value| value.base_color != color)
            && let Some(mut value) = materials.get_mut(&pigment.0)
        {
            value.base_color = color;
        }
    }
}

fn pose(joint: Joint, t: f32, drift: f32, turn: f32, raised: f32) -> Quat {
    let sway = (t * 2.1).sin();
    let swing = (t * 5.).sin() * drift;
    let (pitch, yaw, roll) = match joint {
        Joint::Body => (0.14 * drift, turn * 0.62, sway * 0.025),
        Joint::Head => (0.04 * sway, turn * 0.37, -0.18 + sway * 0.055),
        Joint::Shoulder(side) => (
            0.06 * sway * side + swing * side * 0.32 - if side > 0. { 0.63 * raised } else { 0. },
            0.,
            side * (0.17 + 0.12 * drift),
        ),
        Joint::Elbow(side) => (
            -0.18 - 0.27 * drift + sway * side * 0.055 - if side > 0. { 0.51 * raised } else { 0. },
            0.,
            side * 0.08,
        ),
        Joint::Hip(side) => (0.10 + swing * side * 0.30, 0.04 * side, side * 0.12),
        Joint::Knee(side) => (0.08 + (0.12 + swing * side * 0.14) * drift, 0., 0.),
        Joint::Thruster(_) => return Quat::IDENTITY,
    };
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)
}

pub(super) fn animate(
    joint: Joint,
    transform: &mut Transform,
    t: f32,
    drift: f32,
    turn: f32,
    raised: f32,
    tablet: bool,
) {
    if let Joint::Thruster(side) = joint {
        transform.scale.y = 0.24 + drift * 1.42 + (t * 28. + side * 1.7).sin() * 0.10;
        return;
    }
    transform.rotation = if tablet {
        match joint {
            Joint::Shoulder(side) => {
                Quat::from_euler(EulerRot::YXZ, -side * 0.22, -0.76, side * 0.06)
            }
            Joint::Elbow(side) => Quat::from_euler(EulerRot::YXZ, 0., -0.78, side * 0.20),
            _ => pose(joint, t, drift, turn, 0.),
        }
    } else {
        pose(joint, t, drift, turn, raised)
    };
    if matches!(joint, Joint::Body) {
        transform.translation.y = (t * 2.1).sin() * (0.035 + drift * 0.045);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_tablet_faces_camera_without_perspective_text_distortion() {
        let corners = tablet_corners(&tablet_pose(1., 1.6), Vec2::new(1280., 800.)).unwrap();
        assert!((corners[1] - corners[3]).abs() < 0.001);
        assert!((corners[0] - corners[6]).abs() < 0.001);
        assert!((corners[2] - corners[4]).abs() < 0.001);
        assert!((corners[5] - corners[7]).abs() < 0.001);
        assert_ne!(tablet_pose(0.5, 1.6).rotation, Quat::IDENTITY);
    }

    #[test]
    fn raised_tablet_screen_fits_wide_and_portrait_windows() {
        for size in [
            Vec2::new(1280., 800.),
            Vec2::new(2560., 800.),
            Vec2::new(400., 800.),
        ] {
            let pose = tablet_pose(1., size.x / size.y);
            let corners = tablet_corners(&pose, size).unwrap();
            for point in corners.chunks_exact(2) {
                assert!(point[0] > 0. && point[0] < size.x);
                assert!(point[1] > 0. && point[1] < size.y);
            }
            let lowered = tablet_corners(&tablet_pose(0., size.x / size.y), size).unwrap();
            assert!(
                lowered[1] > corners[1],
                "the screen rises together with its case"
            );
        }
    }

    #[test]
    fn pilot_joints_animate_at_idle_and_answer_motion() {
        assert_ne!(
            pose(Joint::Head, 0., 0., 0., 0.),
            pose(Joint::Head, 0.6, 0., 0., 0.)
        );
        assert_ne!(
            pose(Joint::Knee(1.), 0.4, 1., 0., 0.),
            pose(Joint::Knee(1.), 0.4, 0., 0., 0.)
        );
        assert_ne!(
            pose(Joint::Body, 0.4, 0., 0., 0.),
            pose(Joint::Body, 0.4, 0., 0.5, 0.)
        );
    }
}
