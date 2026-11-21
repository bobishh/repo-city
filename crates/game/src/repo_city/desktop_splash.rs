use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    ui::UiSystems,
};

pub(super) struct Plugin;

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn)
            .add_systems(PostUpdate, layout.before(UiSystems::Layout));
    }
}

#[derive(Component)]
struct Cover {
    aspect: f32,
    focus: Vec2,
    library: bool,
}

#[derive(Component)]
pub(super) struct CityOverlay;

#[derive(Component)]
struct CoverCredit;

const CAVEAT: Handle<Font> = bevy::asset::uuid_handle!("3194a8cc-e336-433d-aaf4-d056ceed4871");

pub(super) fn wordmark_font() -> TextFont {
    TextFont::from_font_size(34.)
        .with_font(CAVEAT)
        .with_font_weight(bevy::text::FontWeight::BOLD)
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
    set: Res<WorldSet>,
) {
    fonts
        .insert(
            CAVEAT.id(),
            Font::from_bytes(include_bytes!("../../../../assets/fonts/Caveat-Latin.ttf").to_vec()),
        )
        .expect("Repo City wordmark font");
    let library = set.is_library();
    let bytes: &[u8] = if library {
        include_bytes!("../../../../assets/greek-library-cover.webp")
    } else {
        include_bytes!("../../../../assets/repo-city-tablet.webp")
    };
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("webp"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("Repo City desktop splash");
    let aspect = image.width() as f32 / image.height() as f32;
    commands.spawn((
        Cover {
            aspect,
            focus: Vec2::new(if library { 0.42 } else { 0.67 }, 0.47),
            library,
        },
        ImageNode::new(images.add(image)),
        Node::default(),
        GlobalZIndex(-1),
        bevy::picking::Pickable::IGNORE,
    ));
    if library {
        commands.spawn((
            CoverCredit,
            Text::new("© OpenStreetMap contributors · ODbL"),
            TextFont::from_font_size(11.),
            TextColor(Color::WHITE),
            BackgroundColor(Color::srgba(0.13, 0.09, 0.06, 0.85)),
            Node {
                position_type: PositionType::Absolute,
                right: px(10.),
                bottom: px(9.),
                padding: UiRect::axes(px(5.), px(3.)),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));
    }
}

fn layout(
    repo: Res<Repo>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buildings: Query<(), With<Facade>>,
    mut cover: Query<(&Cover, &mut Node)>,
    mut overlays: Query<&mut Node, (With<CityOverlay>, Without<Cover>)>,
    mut credit: Query<&mut Node, (With<CoverCredit>, Without<Cover>, Without<CityOverlay>)>,
    mut city_seen: Local<bool>,
) {
    let (Ok(window), Ok((cover, mut node))) = (windows.single(), cover.single_mut()) else {
        return;
    };
    let visible = repo.0.files.is_empty() || (!cover.library && !*city_seen);
    *city_seen |= !buildings.is_empty();
    let viewport = Vec2::new(window.width(), window.height());
    let (size, offset) = fit(viewport, cover.aspect, cover.focus);
    for mut overlay in &mut overlays {
        let display = if visible {
            Display::None
        } else {
            Display::Flex
        };
        if overlay.display != display {
            overlay.display = display;
        }
    }
    if let Ok(mut credit) = credit.single_mut() {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if credit.display != display {
            credit.display = display;
        }
    }
    node.set_if_neq(Node {
        display: if visible {
            Display::Flex
        } else {
            Display::None
        },
        position_type: PositionType::Absolute,
        left: px(offset.x),
        top: px(offset.y),
        width: px(size.x),
        height: px(size.y),
        ..default()
    });
}

fn fit(viewport: Vec2, aspect: f32, focus: Vec2) -> (Vec2, Vec2) {
    let width = viewport.x.max(viewport.y * aspect);
    let size = Vec2::new(width, width / aspect);
    let offset =
        (viewport * 0.5 - size * focus).clamp((viewport - size).min(Vec2::ZERO), Vec2::ZERO);
    (size, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_wide_and_tall_windows_without_stretching_the_robot() {
        let aspect = 1672. / 941.;
        for viewport in [
            Vec2::new(1440., 900.),
            Vec2::new(646., 878.),
            Vec2::new(1200., 600.),
        ] {
            let (size, offset) = fit(viewport, aspect, Vec2::new(0.67, 0.47));
            assert!(offset.cmple(Vec2::ZERO).all());
            assert!((offset + size).cmpge(viewport - Vec2::splat(0.001)).all());
            assert!((size.x / size.y - aspect).abs() < 0.001);
            for point in [Vec2::new(0.55, 0.64), Vec2::new(0.71, 0.34)] {
                let visible = offset + size * point;
                assert!(visible.cmpge(Vec2::ZERO).all() && visible.cmple(viewport).all());
            }
        }
    }
}
