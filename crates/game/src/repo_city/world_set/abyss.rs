//! A submersible surveys file stations with a forward-facing lamp.
use super::super::*;

mod buoys;
pub(crate) mod mirror;
mod pilot;
pub(crate) mod scene;

pub(crate) const LAMP_RANGE: f32 = 72.;
pub(crate) const LAMP_INNER_ANGLE: f32 = 0.27;
pub(crate) const LAMP_OUTER_ANGLE: f32 = 0.45;

pub(super) fn install(app: &mut App, args: &[String]) {
    let world = app.world().resource::<WorldSet>().id.clone();
    app.world_mut()
        .resource_mut::<progress::Progress>()
        .separate_camera(world);
    app.init_resource::<Thrusters>()
        .add_systems(PostUpdate, style_forms);
    install_city(app, args);
    scene::install(app);
    buoys::install(app);
    mirror::install(app);
    #[cfg(not(target_arch = "wasm32"))]
    open_initial_line(app, args);
}

fn style_forms(
    mut forms: Query<(&mut BackgroundColor, &mut BorderColor), Added<native_panel::Form>>,
    mut headings: Query<&mut TextColor, Added<native_panel::Heading>>,
) {
    for (mut surface, mut border) in &mut forms {
        surface.0 = Color::srgb_u8(19, 23, 52);
        *border = BorderColor::all(Color::srgb_u8(102, 104, 147));
    }
    for mut heading in &mut headings {
        heading.0 = Color::srgb_u8(240, 241, 255);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_initial_line(app: &mut App, args: &[String]) {
    let value = |key: &str| {
        args.windows(2)
            .find(|pair| pair[0] == key)
            .map(|pair| &pair[1])
    };
    let (Some(path), Some(line)) = (
        value("--file"),
        value("--line").and_then(|line| line.parse::<usize>().ok()),
    ) else {
        return;
    };
    let path = path.clone();
    let align = move |repo: Res<Repo>,
                      explorer: Res<Explorer>,
                      listing: Res<listing::Listing>,
                      mut camera: Query<&mut Transform, PlayerCamera>| {
        let Some(file) = repo
            .0
            .files
            .get(explorer.selected)
            .filter(|file| file.path == path)
        else {
            return;
        };
        if let Some(pose) = listing.line_view(&explorer, file, listing::Face::Source, line)
            && let Ok(mut camera) = camera.single_mut()
        {
            *camera = pose;
        }
    };
    app.add_systems(
        Update,
        align
            .run_if(run_once)
            .after(spawn_city)
            .before(file_facade::lower_unchanged),
    );
}

pub(crate) fn lamp_strength(camera: &Transform, point: Vec3) -> f32 {
    let offset = point - camera.translation;
    let distance = offset.length();
    if !(0.001..LAMP_RANGE).contains(&distance) {
        return 0.;
    }
    let cosine = camera.forward().dot(offset / distance);
    let outer = LAMP_OUTER_ANGLE.cos();
    let cone = ((cosine - outer) / (LAMP_INNER_ANGLE.cos() - outer)).clamp(0., 1.);
    let range = 1. - (distance / LAMP_RANGE).powi(4);
    (cone * range).powi(2)
}

pub(crate) fn arrival(repo: &Repository, selected: usize) -> Transform {
    let file = &repo.files[selected];
    let target = Vec3::new(
        file.position[0],
        listing::block_size(file).y.min(9.) * 0.45,
        file.position[1],
    );
    // Prefer a longer approach in open water. In dense districts launch in the nearest clear aisle.
    let offsets = [
        Vec3::new(16., 13., 39.),
        Vec3::new(-16., 13., 39.),
        Vec3::new(12., 8., 17.),
    ];
    for offset in offsets {
        let eye = target + offset;
        let distance = offset.length();
        let direction = -offset / distance;
        let occluded = repo.files.iter().enumerate().any(|(i, other)| {
            if i == selected
                || (Vec2::from_array(other.position) - eye.xz()).length_squared()
                    > (distance + listing::WIDTH).powi(2)
            {
                return false;
            }
            let size = listing::block_size(other);
            let center = Vec3::new(other.position[0], size.y * 0.5, other.position[1]);
            ray_box(eye, direction, center - size * 0.5, center + size * 0.5)
                .is_some_and(|d| d < distance)
        });
        if !occluded {
            return Transform::from_translation(eye).looking_at(target, Vec3::Y);
        }
    }
    Transform::from_translation(target + Vec3::new(4., 4., 11.)).looking_at(target, Vec3::Y)
}

#[derive(Default, Resource)]
pub(crate) struct Thrusters {
    velocity: Vec3,
}

impl Thrusters {
    pub(crate) fn stop(&mut self) {
        self.velocity = Vec3::ZERO;
    }

    // Integrate linear drag analytically so a dive covers the same distance at any frame rate.
    fn step(&mut self, target: Vec3, dt: f32) -> Vec3 {
        let previous = self.velocity;
        let response = -(-4. * dt).exp_m1();
        self.velocity += (target - previous) * response;
        target * dt + (previous - target) * (response / 4.)
    }

    pub(crate) fn steer(&mut self, camera: &mut Transform, direction: Vec3, boost: bool, dt: f32) {
        let target = direction.clamp_length_max(1.) * if boost { 32. } else { 10. };
        camera.translation += self.step(target, dt);
        if camera.translation.y <= CAMERA_FLOOR {
            camera.translation.y = CAMERA_FLOOR;
            self.velocity.y = self.velocity.y.max(0.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn launching_at_a_removed_file_places_the_camera_on_the_requested_source_line_once() {
        let source = "fn reef() {}\n".repeat(90);
        let mut repo = repository_picker::empty();
        let file = repo_world::File {
            path: "src/reef.rs".into(),
            before: Some(repo_world::Blob {
                id: "a".repeat(40),
                size: source.len(),
                mode: "100644".into(),
            }),
            after: None,
            change: Change::Removed,
            position: [4., 16.],
        };
        let mut explorer = Explorer::new(0);
        explorer.before = true;
        explorer.set_highlighted(syntax::highlight(&file.path, &source));
        let listing = listing::Listing::default();
        let expected = listing
            .line_view(&explorer, &file, listing::Face::Source, 52)
            .unwrap();
        repo.files.push(file);
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(explorer)
            .insert_resource(listing);
        let eye = app
            .world_mut()
            .spawn((Camera3d::default(), Pilot::default(), Transform::default()))
            .id();
        let args = ["--file", "src/reef.rs", "--line", "52"].map(str::to_owned);
        open_initial_line(&mut app, &args);
        app.update();
        assert_eq!(app.world().get::<Transform>(eye), Some(&expected));
        let departed = Transform::from_xyz(92., 1., 26.);
        *app.world_mut().get_mut::<Transform>(eye).unwrap() = departed;
        app.update();
        assert_eq!(app.world().get::<Transform>(eye), Some(&departed));
    }

    #[test]
    fn the_bathyscaphe_accelerates_and_coasts_the_same_distance_at_different_frame_rates() {
        fn dive(fps: u32) -> (Vec3, Vec3) {
            let mut sub = Thrusters::default();
            let mut distance = Vec3::ZERO;
            for target in [Vec3::Z * 10., Vec3::ZERO] {
                for _ in 0..fps {
                    distance += sub.step(target, 1. / fps as f32);
                }
                if target != Vec3::ZERO {
                    assert!(distance.z > 7. && distance.z < 8.);
                }
            }
            (distance, sub.velocity)
        }
        let (travel, drift) = dive(30);
        let (smooth_travel, smooth_drift) = dive(144);
        assert!(travel.distance(smooth_travel) < 0.001);
        assert!(drift.distance(smooth_drift) < 0.001);
        assert!((travel.z - 9.955).abs() < 0.002);
        assert!((drift.z - 0.180).abs() < 0.002);
    }

    #[test]
    fn a_station_fades_at_the_lamp_edge_and_disappears_behind_the_pilot_or_beyond_range() {
        let eye = Transform::from_xyz(18., 6., 3.).looking_to(Vec3::X, Vec3::Y);
        let sample = |angle: f32, distance: f32| {
            lamp_strength(
                &eye,
                eye.translation + Quat::from_rotation_y(angle) * Vec3::X * distance,
            )
        };
        assert!(sample(0., 10.) > 0.9);
        assert!((0.01..0.9).contains(&sample((LAMP_INNER_ANGLE + LAMP_OUTER_ANGLE) * 0.5, 10.)));
        assert_eq!(sample(LAMP_OUTER_ANGLE + 0.01, 10.), 0.);
        assert_eq!(sample(std::f32::consts::PI, 10.), 0.);
        assert_eq!(sample(0., LAMP_RANGE + 1.), 0.);
    }
}
