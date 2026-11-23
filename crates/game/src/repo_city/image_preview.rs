//! Bounded snapshot image loading for VibePad; decoding happens off the render thread.
use std::{
    io::Cursor,
    sync::{Arc, OnceLock},
};

use bevy::{asset::RenderAssetUsages, image::ImageSampler};

use super::*;

const MAX_DIMENSION: u32 = 8192;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const FORMATS: &str = "PNG, JPEG, WebP, GIF, BMP, ICO, TIFF, TGA, PNM, QOI, DDS, HDR and SVG";

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default)]
pub(super) struct Request(pub Option<(usize, bool)>);

pub(super) fn supported(path: &str) -> bool {
    file_preview::format(path) == Some(file_preview::Format::Image)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct Preview {
    pub path: String,
    pending: Option<native_github::Job<Image>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Preview {
    pub(super) fn open(repo: &Repository, index: usize, before: bool) -> Result<Self, String> {
        let file = repo.files.get(index).ok_or("Unknown file")?.clone();
        if !supported(&file.path) {
            return Err(format!("Image preview supports {FORMATS}."));
        }
        let path = file.path.clone();
        let image_path = path.clone();
        let snapshot = Repository {
            root: repo.root.clone(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            files: vec![file],
            districts: Vec::new(),
        };
        Ok(Self {
            path,
            pending: Some(native_github::Job::spawn(move || {
                decode_named(&image_path, &snapshot.asset_bytes(0, before)?)
            })),
        })
    }

    /// Hand the decoded image to Assets<Image> on the render thread exactly once.
    pub(super) fn take(&mut self) -> Option<Result<Image, String>> {
        let result = self.pending.as_ref()?.take()?;
        self.pending = None;
        Some(result)
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(
            "Image exceeds preview limits (8192 pixels per side, 16 megapixels total).".into(),
        );
    }
    Ok(())
}

pub(super) fn decode(bytes: &[u8]) -> Result<Image, String> {
    decode_named("", bytes)
}

pub(super) fn decode_named(path: &str, bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() > repo_world::ASSET_BYTES_LIMIT {
        return Err("Image exceeds the 16 MiB preview limit.".into());
    }
    if file_facade::extension(path).eq_ignore_ascii_case("svg") {
        return decode_svg(bytes);
    }
    let format = image::guess_format(bytes)
        .ok()
        .or_else(|| {
            // TGA does not have a dependable magic signature. Never guess it for other files.
            file_preview::image_format(path).filter(|format| *format == image::ImageFormat::Tga)
        })
        .ok_or_else(|| "Image data is invalid or unsupported.".to_owned())?;
    if !matches!(
        format,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
            | image::ImageFormat::Bmp
            | image::ImageFormat::Ico
            | image::ImageFormat::Tiff
            | image::ImageFormat::Tga
            | image::ImageFormat::Pnm
            | image::ImageFormat::Qoi
            | image::ImageFormat::Dds
            | image::ImageFormat::Hdr
    ) {
        return Err(format!("Image preview supports {FORMATS}."));
    }
    // The shared image decoder has the same codecs and allocation limits on every target.
    let mut header = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    header.limits(limits.clone());
    let (width, height) = header
        .into_dimensions()
        .map_err(|error| format!("Could not read image dimensions: {error}"))?;
    validate_dimensions(width, height)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("Could not decode image: {error}"))?;
    Ok(texture(decoded.into_rgba8()))
}

fn texture(rgba: image::RgbaImage) -> Image {
    // The in-game tablet samples filterable 8-bit RGBA on both render backends.
    let mut image = Image::from_dynamic(
        image::DynamicImage::ImageRgba8(rgba),
        true,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

fn decode_svg(bytes: &[u8]) -> Result<Image, String> {
    use resvg::{tiny_skia, usvg};
    const MAX_SIDE: u32 = 2048;
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fontdb = FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_font_data(
            include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf").to_vec(),
        );
        fonts.set_serif_family("Fira Mono");
        fonts.set_sans_serif_family("Fira Mono");
        fonts.set_monospace_family("Fira Mono");
        Arc::new(fonts)
    });
    let options = usvg::Options {
        font_family: "Fira Mono".into(),
        fontdb: fontdb.clone(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options)
        .map_err(|error| format!("Could not read SVG: {error}"))?;
    let size = tree.size();
    let scale = (MAX_SIDE as f32 / size.width().max(size.height())).min(1.);
    let extent = |side: f32| ((side * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let (width, height) = (extent(size.width()), extent(size.height()));
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or("Could not allocate SVG preview.")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let bytes = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    let rgba = image::RgbaImage::from_raw(width, height, bytes)
        .ok_or("Invalid SVG preview dimensions.")?;
    Ok(texture(rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_preview_renders_vector_pixels_and_scales_large_viewports() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="50000" viewBox="0 0 2 1"><rect width="2" height="1" fill="red" fill-opacity="0.5"/></svg>"#;
        assert!(supported("diagram.SVG"));
        let preview = decode_named("diagram.SVG", svg).unwrap();
        assert_eq!(preview.size(), UVec2::new(2048, 1024));
        let pixels = preview.data.unwrap();
        assert_eq!(&pixels[..3], &[255, 0, 0]);
        assert!((127..=128).contains(&pixels[3]));
        assert!(decode_named("broken.svg", b"not SVG").is_err());
    }

    struct Fixture(std::path::PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tablet_image_app(bytes: &[u8], label: &str) -> (App, Fixture) {
        let root = Fixture(
            std::env::temp_dir().join(format!("vibe-pad-image-ui-{label}-{}", std::process::id())),
        );
        std::fs::create_dir_all(&root.0).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&root.0)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "-q"]);
        std::fs::write(root.0.join("picture.png"), bytes).unwrap();
        let blob = git(&["hash-object", "-w", "picture.png"]);
        let mut repo = repository_picker::empty();
        repo.root = root.0.clone();
        repo.revision = "image-ui-test".into();
        repo.files = vec![repo_world::File {
            path: "picture.png".into(),
            before: None,
            after: Some(repo_world::Blob {
                id: blob,
                size: bytes.len(),
                mode: "100644".into(),
            }),
            change: Change::Added,
            position: [0., 0.],
        }];
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<review_tablet::Tablet>()
            .init_resource::<guide::Tour>()
            .init_resource::<reading::Settings>()
            .init_resource::<review_notes::Review>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<city_menu::Menu>()
            .init_resource::<live_worktree::Live>()
            .init_resource::<progress::Progress>()
            .init_resource::<listing::find::Search>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .init_resource::<Assets<Image>>()
            .insert_resource(Time::<()>::default())
            .insert_resource(Repo(repo))
            .insert_resource(repository_picker::RepositoryPicker::new(String::new()))
            .insert_resource(Request(Some((0, false))))
            .add_systems(Update, review_tablet::update);
        (app, root)
    }

    fn dds_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        for (at, value) in [
            (4, 124u32),
            (8, 0x81007),
            (12, height),
            (16, width),
            (20, width * height / 2),
            (76, 32),
            (80, 4),
            (108, 0x1000),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(b"DXT1");
        let color = ((26u16 << 11) | (10u16 << 5) | 21u16).to_le_bytes();
        for _ in 0..(width / 4) * (height / 4) {
            bytes.extend_from_slice(&[color[0], color[1], 0, 0, 0, 0, 0, 0]);
        }
        bytes
    }

    #[test]
    fn shared_codecs_render_extended_formats_with_filterable_eight_bit_pixels() {
        // Given supported snapshot rasters, desktop and WASM share the same bounded decoder.
        let original = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            4,
            4,
            image::Rgb([210, 40, 170]),
        ));
        for (name, format) in [
            ("sample.gif", image::ImageFormat::Gif),
            ("sample.bmp", image::ImageFormat::Bmp),
            ("sample.ico", image::ImageFormat::Ico),
            ("sample.tiff", image::ImageFormat::Tiff),
            ("sample.TGA", image::ImageFormat::Tga),
            ("sample.pnm", image::ImageFormat::Pnm),
            ("sample.qoi", image::ImageFormat::Qoi),
            ("sample.hdr", image::ImageFormat::Hdr),
        ] {
            let original = if format == image::ImageFormat::Hdr {
                image::DynamicImage::ImageRgb32F(original.to_rgb32f())
            } else if format == image::ImageFormat::Ico {
                image::DynamicImage::ImageRgba8(original.to_rgba8())
            } else {
                original.clone()
            };
            let mut bytes = Cursor::new(Vec::new());
            original.write_to(&mut bytes, format).unwrap();
            let decoded = decode_named(name, bytes.get_ref())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(decoded.size(), UVec2::new(4, 4), "{name}");
            assert_eq!(
                decoded.texture_descriptor.format,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                "{name}"
            );
            let pixels = decoded.data.unwrap();
            assert!(
                pixels[0] > 150 && pixels[1] < 100 && pixels[2] > 100,
                "{name}"
            );
        }
        let image = decode_named("sample.dds", &dds_bytes(4, 4)).unwrap();
        assert_eq!(image.size(), UVec2::new(4, 4));
        assert!(image.data.unwrap()[0] > 150);
    }

    #[test]
    fn animated_gif_preview_uses_only_the_first_frame() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for rgba in [[210, 40, 170, 255], [20, 220, 30, 255]] {
                encoder
                    .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                        4,
                        4,
                        image::Rgba(rgba),
                    )))
                    .unwrap();
            }
        }
        let image = decode_named("animated.gif", bytes.get_ref()).unwrap();
        assert_eq!(&image.data.unwrap()[..4], &[210, 40, 170, 255]);
    }

    #[test]
    fn new_codecs_keep_dimension_and_invalid_data_guards() {
        let mut tga = vec![0; 18];
        tga[2] = 2;
        tga[12..14].copy_from_slice(&10000u16.to_le_bytes());
        tga[14..16].copy_from_slice(&1u16.to_le_bytes());
        tga[16] = 24;
        assert!(decode_named("too-wide.tga", &tga).is_err());
        assert!(decode_named("invalid.tiff", b"bad data").is_err());
        assert!(decode_named("invalid.gif", b"GIF89a").is_err());
    }

    #[test]
    fn tablet_image_preview_shows_pixels_or_an_editable_navigation_safe_error() {
        // Given a PNG or invalid PNG data, requesting preview opens the same tablet page.
        for (name, bytes, valid) in [
            (
                "valid",
                include_bytes!("../../../../web/assets/apple-touch-icon.png").as_slice(),
                true,
            ),
            ("invalid", b"invalid PNG data".as_slice(), false),
        ] {
            let (mut app, _fixture) = tablet_image_app(bytes, name);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                app.update();
                let world = app.world_mut();
                let pixels = world.query::<&ImageNode>().iter(world).count() == 1;
                let failed = world
                    .query::<&Text>()
                    .iter(world)
                    .any(|text| text.0.contains("Image data is invalid or unsupported"));
                if pixels || failed {
                    assert_eq!(pixels, valid);
                    assert_eq!(failed, !valid);
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Image preview did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(
                app.world().resource::<review_tablet::Tablet>().page_name(),
                Some("image")
            );
            assert!(app.world().resource::<review_notes::Review>().tablet_open);
            // Both outcomes keep the regular Tab dismissal; failures never capture the cursor.
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Tab);
            app.update();
            assert!(!app.world().resource::<review_tablet::Tablet>().is_open());
        }
    }

    #[test]
    fn png_preview_decodes_actual_pixels_and_rejects_invalid_data() {
        let image = decode(include_bytes!(
            "../../../../web/assets/apple-touch-icon.png"
        ))
        .unwrap();
        assert_eq!(image.size(), UVec2::new(180, 180));
        assert!(image.data.as_ref().is_some_and(|pixels| !pixels.is_empty()));
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn preview_rejects_excessive_pixel_allocations_before_decode() {
        assert!(validate_dimensions(5120, 2880).is_ok());
        assert!(validate_dimensions(8192, 8192).is_err());
        assert!(validate_dimensions(1, 100_000).is_err());
        assert!(validate_dimensions(0, 100).is_err());
    }

    #[test]
    fn jpeg_and_webp_codecs_decode_into_the_same_preview_surface() {
        let original = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            3,
            image::Rgb([80, 140, 200]),
        ));
        for format in [image::ImageFormat::Jpeg, image::ImageFormat::WebP] {
            let mut bytes = Cursor::new(Vec::new());
            original.write_to(&mut bytes, format).unwrap();
            assert_eq!(decode(bytes.get_ref()).unwrap().size(), UVec2::new(2, 3));
        }
    }
}
