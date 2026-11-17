//! Bounded snapshot image loading for VibePad; decoding happens off the render thread.
use std::io::Cursor;

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
};

use super::*;

const MAX_DIMENSION: u32 = 8192;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Resource, Default)]
pub(super) struct Request(pub Option<(usize, bool)>);

pub(super) fn supported(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        ["png", "jpg", "jpeg", "webp"]
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

pub(super) struct Preview {
    pub path: String,
    pending: Option<native_github::Job<Image>>,
}

impl Preview {
    pub(super) fn open(repo: &Repository, index: usize, before: bool) -> Result<Self, String> {
        let file = repo.files.get(index).ok_or("Unknown file")?.clone();
        if !supported(&file.path) {
            return Err("Image preview supports PNG, JPEG, and WebP.".into());
        }
        let path = file.path.clone();
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
                decode(&snapshot.asset_bytes(0, before)?)
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

fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() > repo_world::ASSET_BYTES_LIMIT {
        return Err("Image exceeds the 16 MiB preview limit.".into());
    }
    let format = image::guess_format(bytes)
        .map_err(|_| "Image data is invalid or unsupported.".to_owned())?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => return Err("Image preview supports PNG, JPEG, and WebP.".into()),
    };
    // Bevy's from_buffer disables allocation limits, so inspect dimensions before decoding pixels.
    let mut header = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    header.limits(limits);
    let (width, height) = header
        .into_dimensions()
        .map_err(|error| format!("Could not read image dimensions: {error}"))?;
    validate_dimensions(width, height)?;
    Image::from_buffer(
        bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .map_err(|error| format!("Could not decode image: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
