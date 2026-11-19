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
    if ["png", "jpg", "jpeg", "webp"].contains(&extension) {
        return Some(Format::Image);
    }
    // The bounded source reader validates UTF-8 and rejects binary data.
    // Text previews must not depend on a language or filename registry.
    Some(Format::Text)
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
        for name in ["picture.PNG", "photo.jpeg", "texture.webp"] {
            assert_eq!(format(name), Some(Format::Image));
        }
        for name in ["", "/"] {
            assert_eq!(format(name), None);
        }
    }
}
