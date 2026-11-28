//! The sonar chart illuminates a local patch at either map scale.
use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::UiTargetCamera,
};

const TEXTURE_SIZE: u32 = 256;

#[derive(Resource, PartialEq, Default)]
struct ChartLamp {
    center: Vec2,
    viewport: Vec2,
}

impl ChartLamp {
    fn at(viewport: Vec2, cursor: Option<Vec2>) -> Self {
        Self {
            center: cursor.unwrap_or(viewport * 0.5),
            viewport,
        }
    }

    fn strength(&self, point: Vec2) -> f32 {
        let distance = self.center.distance(point) / self.viewport.min_element().max(1.);
        let falloff = ((distance - 0.26) / (0.47 - 0.26)).clamp(0., 1.);
        1. - falloff * falloff * (3. - 2. * falloff)
    }

    fn image_rect(&self) -> Rect {
        let scale = TEXTURE_SIZE as f32 / self.viewport.min_element().max(1.);
        let min = Vec2::splat(TEXTURE_SIZE as f32 * 0.5) - self.center * scale;
        Rect::from_corners(min, min + self.viewport * scale)
    }
}

#[derive(Component)]
struct Water;

pub(super) fn install(app: &mut App) {
    app.init_resource::<ChartLamp>()
        .add_systems(PostStartup, spawn)
        .add_systems(Update, position.before(targeting))
        .add_systems(
            Update,
            shade.after(city_navigation::navigate).after(position),
        );
}

fn spawn(
    mut commands: Commands,
    eye: Single<Entity, PlayerCamera>,
    mut images: ResMut<Assets<Image>>,
) {
    let lamp = ChartLamp::at(Vec2::splat(TEXTURE_SIZE as f32), None);
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let lit = lamp.strength(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            pixels.extend([4, 15, 21, ((1. - lit) * 255.).round() as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    commands.spawn((
        Water,
        UiTargetCamera(*eye),
        GlobalZIndex(-100),
        ImageNode::new(images.add(image)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: percent(100.),
            height: percent(100.),
            left: px(0.),
            top: px(0.),
            ..default()
        },
        bevy::picking::Pickable::IGNORE,
    ));
}

fn position(windows: Query<&Window, With<PrimaryWindow>>, mut lamp: ResMut<ChartLamp>) {
    if let Ok(window) = windows.single() {
        lamp.set_if_neq(ChartLamp::at(
            Vec2::new(window.width(), window.height()),
            window.cursor_position(),
        ));
    }
}

fn shade(
    lamp: Res<ChartLamp>,
    navigation: Option<Res<city_navigation::Navigation>>,
    review: Option<Res<review_notes::Review>>,
    water: Single<(&mut Node, &mut ImageNode), With<Water>>,
) {
    let (mut node, mut image) = water.into_inner();
    let shown = navigation.is_some_and(|n| n.overview()) && !review.is_some_and(|r| r.tablet_open);
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
    if shown && lamp.viewport.min_element() > 0. {
        let rect = Some(lamp.image_rect());
        if image.rect != rect {
            image.rect = rect;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn district_and_project_charts_light_the_pointer_and_keep_distant_water_dark() {
        for size in [Vec2::new(1440., 900.), Vec2::new(420., 920.)] {
            let lamp = ChartLamp::at(size, Some(size * Vec2::new(0.3, 0.4)));
            let half = size.min_element();
            assert_eq!(lamp.strength(lamp.center), 1.);
            assert_eq!(lamp.strength(lamp.center + Vec2::Y * half * 0.2), 1.);
            let edge = lamp.strength(lamp.center + Vec2::X * half * 0.36);
            assert!((0.2..0.8).contains(&edge));
            assert_eq!(lamp.strength(lamp.center + Vec2::X * half * 0.65), 0.);
            assert_eq!(ChartLamp::at(size, None).center, size * 0.5);
        }
    }
}
