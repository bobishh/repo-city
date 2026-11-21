//! Shared preview selection; unrecognized names use the bounded text reader.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Markdown,
    Text,
    Image,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FacadeKind {
    Code,
    License,
    Text,
    Markdown,
    Media,
    Asset,
}

pub(super) fn facade_kind(path: &str) -> FacadeKind {
    let path_name = path.to_ascii_lowercase();
    let path_parts = Path::new(&path_name);
    let name = path_parts
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let extension = path_parts
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    if matches!(format(path), Some(Format::Image | Format::Video))
        || matches!(
            extension,
            "mp3" | "ogg" | "oga" | "opus" | "wav" | "flac" | "aac" | "m4a"
        )
    {
        return FacadeKind::Media;
    }
    let stem = path_parts
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if path_name
        .split('/')
        .any(|component| component == "licenses")
        || stem
            .split(['-', '_'])
            .any(|part| matches!(part, "license" | "licence" | "copying"))
    {
        return FacadeKind::License;
    }
    if format(path) == Some(Format::Markdown) {
        return FacadeKind::Markdown;
    }
    if matches!(extension, "txt" | "rst" | "adoc" | "tex")
        || matches!(name, "readme" | "notice" | "changelog" | "authors")
    {
        return FacadeKind::Text;
    }
    if has_text_size_hint(path) {
        FacadeKind::Code
    } else if extension.is_empty() {
        FacadeKind::Text
    } else {
        FacadeKind::Asset
    }
}

pub(super) fn video_extension(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "mp4" => "mp4",
        "m4v" => "m4v",
        "webm" => "webm",
        "mov" => "mov",
        "ogv" => "ogv",
        _ => return None,
    })
}

/// File names that can use a text size estimate before the content is loaded.
pub(super) fn has_text_size_hint(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let extension = Path::new(&name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    matches!(
        extension,
        "rs" | "go"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "cxx"
            | "hpp"
            | "cs"
            | "swift"
            | "kt"
            | "kts"
            | "java"
            | "scala"
            | "py"
            | "rb"
            | "rake"
            | "php"
            | "lua"
            | "ex"
            | "exs"
            | "elm"
            | "erl"
            | "hrl"
            | "hs"
            | "ml"
            | "mli"
            | "fs"
            | "lean"
            | "zig"
            | "dart"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "ps1"
            | "js"
            | "mjs"
            | "cjs"
            | "jsx"
            | "ts"
            | "mts"
            | "cts"
            | "tsx"
            | "vue"
            | "svelte"
            | "astro"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "wgsl"
            | "wesl"
            | "glsl"
            | "vert"
            | "frag"
            | "sql"
            | "graphql"
            | "proto"
            | "toml"
            | "ini"
            | "conf"
            | "cfg"
            | "json"
            | "jsonc"
            | "jsonl"
            | "yaml"
            | "yml"
            | "xml"
            | "csv"
            | "tsv"
            | "lock"
            | "env"
            | "diff"
            | "patch"
            | "md"
            | "markdown"
            | "mdown"
            | "mkd"
            | "txt"
            | "rst"
            | "adoc"
            | "tex"
    ) || matches!(
        name.as_str(),
        "dockerfile"
            | "containerfile"
            | "makefile"
            | "justfile"
            | "rakefile"
            | "gemfile"
            | "procfile"
            | "readme"
            | "license"
            | "licence"
            | "copying"
            | "notice"
            | "changelog"
            | "authors"
            | ".env"
            | ".gitignore"
            | ".gitattributes"
            | ".gitmodules"
            | ".dockerignore"
            | ".editorconfig"
    ) || name.starts_with("license-")
        || name.starts_with("licence-")
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
    if extension == "svg" || image_format(path).is_some() {
        return Some(Format::Image);
    }
    if video_extension(path).is_some() {
        return Some(Format::Video);
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

    #[test]
    fn facade_categories_keep_documents_and_media_distinct() {
        for (path, expected) in [
            ("src/main.rs", FacadeKind::Code),
            ("docs/README.MD", FacadeKind::Markdown),
            ("legal/UPSTREAM-LICENSE", FacadeKind::License),
            ("licenses/MPL-2.0.txt", FacadeKind::License),
            ("notes/plain.txt", FacadeKind::Text),
            ("cover.svg", FacadeKind::Media),
            ("sound/theme.ogg", FacadeKind::Media),
            ("demo.WEBM", FacadeKind::Media),
            ("fonts/title.ttf", FacadeKind::Asset),
        ] {
            assert_eq!(facade_kind(path), expected, "{path}");
        }
        assert_eq!(format("demo.WEBM"), Some(Format::Video));
    }
}
