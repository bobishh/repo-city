//! One extension policy for file previews in the desktop and browser tablets.
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
    if [
        "txt",
        "text",
        "log",
        "csv",
        "tsv",
        "rst",
        "adoc",
        "asciidoc",
        "org",
        "rdoc",
        "tex",
        "json",
        "jsonc",
        "json5",
        "yaml",
        "yml",
        "toml",
        "xml",
        "html",
        "htm",
        "svg",
        "css",
        "scss",
        "sass",
        "less",
        "js",
        "mjs",
        "cjs",
        "jsx",
        "ts",
        "tsx",
        "rs",
        "go",
        "py",
        "rb",
        "rake",
        "gemspec",
        "clj",
        "cljs",
        "cljc",
        "edn",
        "c",
        "cc",
        "cpp",
        "cxx",
        "h",
        "hpp",
        "sh",
        "bash",
        "zsh",
        "fish",
        "ps1",
        "bat",
        "cmd",
        "sql",
        "graphql",
        "proto",
        "ini",
        "cfg",
        "conf",
        "config",
        "env",
        "lock",
        "properties",
        "svelte",
        "vue",
        "swift",
        "kt",
        "kts",
        "java",
        "cs",
        "fs",
        "ex",
        "exs",
        "erl",
        "hrl",
        "hs",
        "lua",
        "nix",
        "cmake",
        "gradle",
        "scala",
        "dart",
        "diff",
        "patch",
    ]
    .contains(&extension)
        || [
            "readme",
            "license",
            "licence",
            "copying",
            "notice",
            "authors",
            "contributors",
            "changelog",
            "changes",
            "makefile",
            "dockerfile",
            "containerfile",
            "gemfile",
            "rakefile",
            "procfile",
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".editorconfig",
            ".env",
        ]
        .contains(&file.as_str())
        || file.starts_with("license-")
        || file.starts_with("licence-")
    {
        Some(Format::Text)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documents_code_and_images_share_a_case_insensitive_extension_policy() {
        for name in [
            "notes.TXT",
            "LICENSE-MIT",
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
        for name in ["app.exe", "data.sqlite3", "archive.zip", "README.md.bak"] {
            assert_eq!(format(name), None);
        }
    }
}
