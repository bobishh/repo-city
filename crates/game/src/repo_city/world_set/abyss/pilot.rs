//! The engineer and the pressure sphere with its twin service lamps.
use super::*;
use bevy::light::NotShadowCaster;
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Component)]
pub(super) struct Eyelid {
    open_y: f32,
    radius: f32,
}

#[derive(Component)]
pub(super) struct Spanner;

#[derive(Clone, Copy)]
enum Form {
    Oval,
    Drum,
    Taper,
    Rim,
    Arch,
}

struct Model<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    shapes: [Handle<Mesh>; 5],
}

impl Model<'_, '_, '_> {
    fn piece(
        &mut self,
        parent: Entity,
        shape: Form,
        material: &Handle<StandardMaterial>,
        center: Vec3,
        radii: Vec3,
        rotation: Quat,
    ) -> Entity {
        self.commands
            .spawn((
                ChildOf(parent),
                Mesh3d(self.shapes[shape as usize].clone()),
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                Transform::from_translation(center)
                    .with_scale(radii)
                    .with_rotation(rotation),
            ))
            .id()
    }

    fn oval(
        &mut self,
        parent: Entity,
        material: &Handle<StandardMaterial>,
        center: [f32; 3],
        radii: [f32; 3],
    ) -> Entity {
        self.piece(
            parent,
            Form::Oval,
            material,
            center.into(),
            radii.into(),
            Quat::IDENTITY,
        )
    }

    fn bar(
        &mut self,
        parent: Entity,
        material: &Handle<StandardMaterial>,
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    ) {
        let (a, b) = (Vec3::from(a), Vec3::from(b));
        let axis = b - a;
        self.piece(
            parent,
            Form::Drum,
            material,
            (a + b) * 0.5,
            Vec3::new(radius, axis.length(), radius),
            Quat::from_rotation_arc(Vec3::Y, axis.normalize()),
        );
    }

    fn tuft(
        &mut self,
        parent: Entity,
        material: &Handle<StandardMaterial>,
        base: [f32; 3],
        tip: [f32; 3],
        radius: f32,
    ) {
        let (base, tip) = (Vec3::from(base), Vec3::from(tip));
        let axis = tip - base;
        self.piece(
            parent,
            Form::Taper,
            material,
            (base + tip) * 0.5,
            Vec3::new(radius, axis.length(), radius),
            Quat::from_rotation_arc(Vec3::Y, axis.normalize()),
        );
    }

    fn joint(&mut self, parent: Entity, name: &'static str, pose: Transform) -> Entity {
        self.commands
            .spawn((
                Name::new(name),
                ChildOf(parent),
                pose,
                Visibility::Inherited,
            ))
            .id()
    }
}

struct Finishes {
    skin: Handle<StandardMaterial>,
    cheek: Handle<StandardMaterial>,
    ear: Handle<StandardMaterial>,
    plum: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    iris: Handle<StandardMaterial>,
    mint: Handle<StandardMaterial>,
    lilac: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    amber: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
    glint: Handle<StandardMaterial>,
    acrylic: Handle<StandardMaterial>,
}

impl Finishes {
    fn new(assets: &mut Assets<StandardMaterial>) -> Self {
        let mut finish = |rgb: [u8; 3], metal: f32, roughness: f32, emission: f32| {
            let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
            assets.add(StandardMaterial {
                base_color: color,
                emissive: color.to_linear() * emission,
                metallic: metal,
                perceptual_roughness: roughness,
                clearcoat: if metal > 0.2 { 0.62 } else { 0.16 },
                clearcoat_perceptual_roughness: 0.21,
                fog_enabled: false,
                ..default()
            })
        };
        let mut p = Self {
            skin: finish([93, 113, 247], 0.02, 0.46, 0.34),
            cheek: finish([140, 154, 255], 0.04, 0.41, 0.34),
            ear: finish([149, 154, 245], 0.02, 0.51, 0.30),
            plum: finish([42, 28, 70], 0.11, 0.36, 0.21),
            white: finish([246, 240, 208], 0.04, 0.25, 0.44),
            iris: finish([43, 180, 165], 0.17, 0.22, 0.45),
            mint: finish([133, 226, 204], 0.40, 0.25, 0.34),
            lilac: finish([182, 151, 227], 0.43, 0.27, 0.32),
            brass: finish([215, 162, 92], 0.60, 0.31, 0.36),
            steel: finish([193, 212, 226], 0.68, 0.24, 0.43),
            amber: finish([247, 161, 73], 0.12, 0.35, 0.43),
            lamp: finish([252, 235, 166], 0.08, 0.18, 1.8),
            glint: finish([241, 255, 243], 0.03, 0.19, 1.1),
            acrylic: default(),
        };
        p.acrylic = assets.add(StandardMaterial {
            base_color: Color::srgba(0.67, 0.88, 0.93, 0.055),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.23,
            clearcoat: 0.44,
            fog_enabled: false,
            ..default()
        });
        p
    }
}

pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let root = commands
        .spawn((
            Name::new("Survey bathyscaphe"),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    let mut model = Model {
        commands,
        shapes: [
            meshes.add(Sphere::new(1.).mesh().uv(24, 16)),
            meshes.add(Cylinder::new(1., 1.).mesh().resolution(24)),
            meshes.add(Cone::new(1., 1.).mesh().resolution(14)),
            meshes.add(
                Torus::new(0.87, 1.)
                    .mesh()
                    .major_resolution(48)
                    .minor_resolution(8),
            ),
            meshes.add(
                Torus::new(0.975, 1.)
                    .mesh()
                    .major_resolution(32)
                    .minor_resolution(6)
                    .angle_range(0.0..=std::f32::consts::PI),
            ),
        ],
    };
    let p = Finishes::new(materials);
    vessel(&mut model, root, &p);
    engineer(&mut model, root, &p);
    // Two small portrait fills keep the face legible against an unlit trench.
    for (position, color, intensity) in [
        (
            Vec3::new(-1.4, 1.9, -2.),
            Color::srgb(1., 0.85, 0.76),
            17000.,
        ),
        (
            Vec3::new(1.5, 0.9, 0.8),
            Color::srgb(0.60, 0.83, 1.),
            11000.,
        ),
    ] {
        model.commands.spawn((
            ChildOf(root),
            Transform::from_translation(position),
            PointLight {
                color,
                intensity,
                range: 4.2,
                radius: 0.35,
                ..default()
            },
        ));
    }
    root
}

fn vessel(m: &mut Model, root: Entity, p: &Finishes) {
    let front = Quat::from_rotation_x(FRAC_PI_2);
    m.oval(root, &p.lilac, [0., -0.40, 0.13], [1.02, 0.49, 1.35]);
    m.oval(root, &p.mint, [0., -0.53, -0.19], [0.98, 0.32, 1.11]);
    m.piece(
        root,
        Form::Rim,
        &p.brass,
        Vec3::new(0., -0.17, 0.),
        Vec3::new(0.86, 0.60, 0.85),
        Quat::IDENTITY,
    );
    m.oval(root, &p.plum, [0., -0.14, 0.], [0.79, 0.11, 0.78]);
    m.oval(root, &p.acrylic, [0., 0.24, 0.06], [0.78, 0.81, 0.70]);
    // Rear acrylic seam and two lacquer glints leave the engineer's entire face unobstructed.
    m.piece(
        root,
        Form::Arch,
        &p.brass,
        Vec3::new(0., -0.14, 0.36),
        Vec3::new(0.79, 0.65, 1.13),
        front.inverse(),
    );
    m.piece(
        root,
        Form::Oval,
        &p.glint,
        Vec3::new(0.62, 0.74, -0.12),
        Vec3::new(0.016, 0.12, 0.013),
        Quat::from_rotation_z(0.61),
    );
    for side in [-1., 1.] {
        let lens = scene::lens_center(side);
        m.bar(
            root,
            &p.steel,
            [side * 0.73, -0.4, -0.42],
            [lens.x, lens.y, -0.55],
            0.13,
        );
        m.piece(
            root,
            Form::Drum,
            &p.lilac,
            lens + Vec3::Z * 0.40,
            Vec3::new(0.455, 0.80, 0.455),
            front,
        );
        m.oval(root, &p.mint, [lens.x, lens.y, -0.25], [0.45, 0.45, 0.31]);
        for (z, radius, material) in [
            (0.33, 0.465, &p.mint),
            (0.08, 0.47, &p.plum),
            (0.032, 0.469, &p.brass),
        ] {
            m.piece(
                root,
                Form::Rim,
                material,
                lens + Vec3::Z * z,
                Vec3::splat(radius),
                front,
            );
        }
        let optic = m.piece(
            root,
            Form::Oval,
            &p.lamp,
            lens,
            Vec3::new(0.408, 0.408, 0.095),
            Quat::IDENTITY,
        );
        m.commands.entity(optic).insert(Name::new("Fresnel lens"));
        for i in 1..=3 {
            let radius = i as f32 * 0.09 + 0.055;
            let z = -0.095 * (1. - (radius / 0.408).powi(2)).sqrt();
            m.piece(
                root,
                Form::Rim,
                &p.glint,
                lens + Vec3::Z * z,
                Vec3::new(radius, 0.045, radius),
                front,
            );
        }
        m.piece(
            root,
            Form::Oval,
            &p.mint,
            Vec3::new(side * 0.82, -0.42, 0.85),
            Vec3::new(0.56, 0.073, 0.33),
            Quat::from_rotation_y(-side * 0.38),
        );
        m.bar(
            root,
            &p.brass,
            [side * 0.54, -0.63, -0.63],
            [side * 0.58, -0.90, -0.52],
            0.045,
        );
        m.bar(
            root,
            &p.brass,
            [side * 0.56, -0.62, 0.49],
            [side * 0.58, -0.90, 0.65],
            0.045,
        );
        m.oval(
            root,
            &p.lilac,
            [side * 0.58, -0.90, 0.10],
            [0.10, 0.07, 0.91],
        );
    }
    m.oval(root, &p.plum, [0., -0.53, -1.19], [0.31, 0.14, 0.075]);
    for x in [-0.18, -0.09, 0., 0.09, 0.18] {
        m.oval(root, &p.brass, [x, -0.53, -1.255], [0.014, 0.088, 0.014]);
    }
    m.piece(
        root,
        Form::Rim,
        &p.brass,
        Vec3::new(0., -0.33, 1.49),
        Vec3::splat(0.34),
        front,
    );
    for i in 0..3 {
        let a = i as f32 * TAU / 3.;
        m.piece(
            root,
            Form::Oval,
            &p.mint,
            Vec3::new(a.cos() * 0.13, -0.33 + a.sin() * 0.13, 1.48),
            Vec3::new(0.19, 0.066, 0.04),
            Quat::from_rotation_z(a + 0.30),
        );
    }
}

fn engineer(m: &mut Model, root: Entity, p: &Finishes) {
    let head = m.joint(root, "Engineer", Transform::default());
    m.oval(root, &p.amber, [0., -0.12, -0.31], [0.37, 0.26, 0.25]);
    for side in [-1., 1.] {
        m.oval(
            root,
            &p.mint,
            [side * 0.28, -0.12, -0.37],
            [0.14, 0.17, 0.14],
        );
    }
    m.piece(
        head,
        Form::Oval,
        &p.skin,
        Vec3::new(0., 0.39, 0.05),
        Vec3::new(0.63, 0.605, 0.53),
        Quat::IDENTITY,
    );
    m.oval(head, &p.skin, [0., 0.125, -0.30], [0.355, 0.19, 0.215]);
    for side in [-1., 1.] {
        m.oval(
            head,
            &p.skin,
            [side * 0.585, 0.37, 0.015],
            [0.105, 0.145, 0.10],
        );
        m.oval(
            head,
            &p.ear,
            [side * 0.671, 0.38, 0.005],
            [0.024, 0.086, 0.062],
        );

        let pos = scene::PILOT_EYE + Vec3::X * (side * 0.224);
        m.piece(
            head,
            Form::Oval,
            &p.white,
            pos,
            Vec3::new(0.193, 0.179, 0.120),
            Quat::IDENTITY,
        );
        let iris = pos - Vec3::Z * 0.109;
        m.piece(
            head,
            Form::Oval,
            &p.iris,
            iris,
            Vec3::new(0.104, 0.112, 0.038),
            Quat::IDENTITY,
        );
        m.piece(
            head,
            Form::Oval,
            &p.plum,
            iris - Vec3::Z * 0.029,
            Vec3::new(0.057, 0.071, 0.027),
            Quat::IDENTITY,
        );
        m.piece(
            head,
            Form::Oval,
            &p.glint,
            iris + Vec3::new(-0.021, 0.029, -0.053),
            Vec3::new(0.017, 0.021, 0.009),
            Quat::IDENTITY,
        );
        let top = pos + Vec3::new(0., 0.165, -0.016);
        let lid = m.piece(
            head,
            Form::Oval,
            &p.skin,
            top,
            Vec3::new(0.171, 0.038, 0.092),
            Quat::IDENTITY,
        );
        m.commands.entity(lid).insert(Eyelid {
            open_y: top.y,
            radius: 0.038,
        });
        m.piece(
            head,
            Form::Oval,
            &p.plum,
            pos + Vec3::new(0., 0.235, 0.03),
            Vec3::new(0.151, 0.026, 0.035),
            Quat::from_rotation_z(-side * 0.075),
        );
    }
    m.oval(head, &p.skin, [0., 0.291, -0.507], [0.07, 0.111, 0.072]);
    m.oval(head, &p.cheek, [0., 0.234, -0.552], [0.091, 0.067, 0.079]);
    // A thin upward curve follows the cheek surface, with a closed smile.
    let smile = |x: f32| [x, 0.11 + x * x * 1.85, -0.519 + x * x * 0.86];
    for i in 0..8 {
        let x = -0.152 + i as f32 * 0.038;
        m.bar(head, &p.plum, smile(x), smile(x + 0.038), 0.010);
    }
    // Short side-parted hair and the engineer's grease pencil.
    m.piece(
        head,
        Form::Oval,
        &p.plum,
        Vec3::new(0., 0.811, 0.105),
        Vec3::new(0.50, 0.194, 0.382),
        Quat::from_rotation_z(0.045),
    );
    m.piece(
        head,
        Form::Oval,
        &p.plum,
        Vec3::new(-0.145, 0.824, -0.10),
        Vec3::new(0.315, 0.115, 0.205),
        Quat::from_rotation_z(0.14),
    );
    m.piece(
        head,
        Form::Oval,
        &p.plum,
        Vec3::new(0.232, 0.818, -0.060),
        Vec3::new(0.217, 0.114, 0.216),
        Quat::from_rotation_z(-0.14),
    );
    m.bar(
        head,
        &p.amber,
        [0.554, 0.50, -0.18],
        [0.62, 0.79, 0.33],
        0.024,
    );
    m.tuft(
        head,
        &p.plum,
        [0.554, 0.50, -0.18],
        [0.547, 0.468, -0.236],
        0.024,
    );
    // Tiny gloved hands control the boat and hold a properly open-ended spanner.
    m.bar(
        root,
        &p.amber,
        [-0.29, -0.10, -0.39],
        [-0.48, -0.05, -0.54],
        0.082,
    );
    m.bar(
        root,
        &p.amber,
        [-0.48, -0.05, -0.54],
        [-0.53, 0.016, -0.63],
        0.07,
    );
    m.bar(
        root,
        &p.amber,
        [0.30, -0.10, -0.38],
        [0.46, -0.06, -0.57],
        0.082,
    );
    m.bar(
        root,
        &p.brass,
        [0.40, -0.31, -0.65],
        [0.48, -0.034, -0.68],
        0.030,
    );
    m.oval(root, &p.plum, [0.48, -0.028, -0.67], [0.085, 0.075, 0.073]);
    m.oval(root, &p.ear, [0.44, -0.06, -0.62], [0.088, 0.064, 0.064]);
    let tool = m.joint(
        root,
        "Inspection spanner",
        Transform::from_xyz(-0.52, 0.01, -0.64).with_rotation(Quat::from_rotation_z(0.48)),
    );
    m.commands.entity(tool).insert(Spanner);
    m.bar(tool, &p.steel, [0., -0.13, 0.], [0., 0.30, 0.], 0.033);
    m.bar(tool, &p.steel, [0., 0.28, 0.], [-0.079, 0.343, 0.], 0.045);
    m.bar(
        tool,
        &p.steel,
        [-0.079, 0.343, 0.],
        [-0.092, 0.419, 0.],
        0.036,
    );
    m.bar(tool, &p.steel, [0., 0.28, 0.], [0.077, 0.343, 0.], 0.045);
    m.bar(
        tool,
        &p.steel,
        [0.077, 0.343, 0.],
        [0.092, 0.393, 0.],
        0.035,
    );
    m.oval(root, &p.ear, [-0.517, 0.022, -0.663], [0.082, 0.073, 0.066]);
    for i in 0..3 {
        m.oval(
            root,
            &p.cheek,
            [-0.552 + i as f32 * 0.028, 0.023 - i as f32 * 0.012, -0.711],
            [0.024, 0.044, 0.025],
        );
    }
}

pub(super) fn animate(
    time: Res<Time>,
    mut lids: Query<(&Eyelid, &InheritedVisibility, &mut Transform)>,
    mut wrench: Query<(&InheritedVisibility, &mut Transform), (With<Spanner>, Without<Eyelid>)>,
) {
    let t = time.elapsed_secs();
    let blink = (1. - ((t % 3.93) - 0.16).abs() / 0.14).clamp(0., 1.);
    for (lid, visible, mut pose) in &mut lids {
        if visible.get() {
            pose.translation.y = lid.open_y - blink * 0.13;
            pose.scale.y = lid.radius + blink * 0.13;
        }
    }
    for (visible, mut pose) in &mut wrench {
        if visible.get() {
            pose.rotation = Quat::from_rotation_z(0.48 + (t * 1.35).sin() * 0.058);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn the_inspection_model_starts_hidden_with_two_large_forward_lenses() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>();
        let world = app.world_mut();
        let root = world
            .run_system_once(
                |mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>| {
                    spawn(&mut commands, &mut meshes, &mut materials)
                },
            )
            .unwrap();
        assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden));
        let lenses: Vec<_> = world
            .query::<(&Name, &Transform, &ChildOf)>()
            .iter(world)
            .filter(|(name, _, _)| name.as_str() == "Fresnel lens")
            .map(|(_, pose, parent)| (*pose, parent.parent()))
            .collect();
        assert_eq!(lenses.len(), 2);
        for (lens, parent) in &lenses {
            assert_eq!(*parent, root);
            assert!(lens.translation.z < scene::PILOT_EYE.z && lens.scale.x >= 0.40);
        }
        assert!(lenses[0].0.translation.x * lenses[1].0.translation.x < -1.);
        assert_eq!(world.query::<&Eyelid>().iter(world).count(), 2);
    }
}
