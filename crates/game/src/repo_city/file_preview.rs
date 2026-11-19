//! Shared preview selection; unrecognized names use the bounded text reader.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Markdown,
    Text,
    Image,
}

pub(super) fn format(path: &str) -> Option<Format> {
    let file = Path::new(path).file_name()?.to_str()?.to_ascii_lowercase();
    let extension = Path::new(&file)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if ["md", "markdown", "mdown", "mkd"].contains(&extension) {
        return Some(Format::Markdown);
    }
    if image_format(path).is_some() {
        return Some(Format::Image);
    }
    // The bounded source reader validates UTF-8 and rejects binary data.
    // Text previews must not depend on a language or filename registry.
    Some(Format::Text)
}

/// Extension hints are used only when the raster format has no reliable magic, such as TGA.
pub(super) fn image_format(path: &str) -> Option<image::ImageFormat> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "webp" => image::ImageFormat::WebP,
        "gif" => image::ImageFormat::Gif,
        "bmp" => image::ImageFormat::Bmp,
        "ico" => image::ImageFormat::Ico,
        "tif" | "tiff" => image::ImageFormat::Tiff,
        "tga" => image::ImageFormat::Tga,
        "pnm" | "pbm" | "pgm" | "ppm" | "pam" => image::ImageFormat::Pnm,
        "qoi" => image::ImageFormat::Qoi,
        "dds" => image::ImageFormat::Dds,
        "hdr" | "rgbe" => image::ImageFormat::Hdr,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_names_and_unknown_languages_use_text_while_images_keep_their_decoder() {
        for name in [
            "notes.TXT",
            "LICENSE-MIT",
            "skills/differential-review/UPSTREAM-LICENSE",
            "licenses/THIRD_PARTY_NOTICES",
            "proof.lean",
            "Campaign.elm",
            "document.custom-extension",
            ".gitignore",
            "settings.toml",
            "source.mjs",
            "schema.json",
            "Makefile",
            "manual.rst",
        ] {
            assert_eq!(format(name), Some(Format::Text), "{name}");
        }
        assert_eq!(format("README.MD"), Some(Format::Markdown));
        for name in [
            "picture.PNG",
            "photo.jpeg",
            "texture.webp",
            "animated.GIF",
            "bitmap.bmp",
            "icon.ico",
            "scan.tif",
            "scan.tiff",
            "texture.TGA",
            "raster.pnm",
            "raster.ppm",
            "raster.pgm",
            "raster.pbm",
            "raster.pam",
            "raster.qoi",
            "texture.dds",
            "environment.hdr",
            "environment.rgbe",
        ] {
            assert_eq!(format(name), Some(Format::Image));
        }
        for name in ["", "/"] {
            assert_eq!(format(name), None);
        }
    }
}
