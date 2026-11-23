use super::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use sketch::{Format, Sketch};

#[derive(Default)]
pub(super) struct Readme {
    file: Option<(usize, String, String)>,
    loaded: Option<Option<Sketch>>,
}

impl Readme {
    pub(super) fn select(&mut self, repo: &Repository) {
        let file = repo.files.iter().enumerate().find_map(|(index, file)| {
            let blob = file.after.as_ref()?;
            file.path
                .eq_ignore_ascii_case("README.md")
                .then(|| (index, file.path.clone(), blob.id.clone()))
        });
        if self.file.as_ref().map(|(_, path, blob)| (path, blob))
            != file.as_ref().map(|(_, path, blob)| (path, blob))
        {
            self.loaded = None;
        }
        self.file = file;
    }

    pub(super) fn get(&mut self, repo: &Repository) -> Option<Sketch> {
        let (index, path, _) = self.file.as_ref()?;
        if self.loaded.is_none() {
            match repo.source(*index, false) {
                Ok(mut source) => {
                    source.truncate(source.floor_char_boundary(16_384));
                    self.loaded = Some(Some(Sketch {
                        title: path.clone(),
                        source,
                        format: Format::Markdown,
                    }));
                }
                Err(error) if loading(&error) => {}
                Err(_) => self.loaded = Some(None),
            }
        }
        self.loaded.as_ref().cloned().flatten()
    }
}

fn loading(error: &str) -> bool {
    cfg!(target_arch = "wasm32") && error.starts_with("Loading source from GitHub")
}

#[derive(Debug, PartialEq, Eq)]
enum Block {
    Paragraph(String),
    Heading(String, HeadingLevel),
    Code(String),
    Mermaid(String),
    Picture { href: String, alt: String },
}

fn blocks(source: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut image = None;
    let mut mermaid = false;
    let mut heading = None;
    let flush = |result: &mut Vec<Block>, text: &mut String| {
        if !text.trim().is_empty() {
            result.push(Block::Paragraph(std::mem::take(text)));
        } else {
            text.clear();
        }
    };
    for event in markdown_preview::events(source) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut result, &mut text);
                heading = Some(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading.take() {
                    result.push(Block::Heading(std::mem::take(&mut text), level));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut result, &mut text);
                mermaid =
                    matches!(kind, CodeBlockKind::Fenced(language) if language.trim() == "mermaid");
            }
            Event::End(TagEnd::CodeBlock) => {
                let source = std::mem::take(&mut text);
                result.push(if mermaid {
                    Block::Mermaid(source)
                } else {
                    Block::Code(source)
                });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush(&mut result, &mut text);
                image = Some(dest_url.into_string());
            }
            Event::End(TagEnd::Image) => {
                if let Some(href) = image.take() {
                    result.push(Block::Picture {
                        href,
                        alt: std::mem::take(&mut text),
                    });
                }
            }
            Event::Start(Tag::Item) => {
                flush(&mut result, &mut text);
                text.push_str("• ");
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph | TagEnd::TableRow) => {
                flush(&mut result, &mut text)
            }
            Event::End(TagEnd::TableCell) => text.push_str("  |  "),
            Event::Text(body) | Event::Code(body) => text.push_str(&body),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            _ => {}
        }
    }
    flush(&mut result, &mut text);
    result
}

pub(super) fn layout(repo: &Repository, document: Option<&Sketch>) -> (Page, bool) {
    let mut page = Page::new(document.map_or("Documents", |document| &document.title));
    let Some(document) = document else {
        page.text(
            "Open a checkout with a README or stage an MCP sketch.",
            24.,
            BODY,
            false,
        );
        return (page, false);
    };
    let body = if document.format == Format::Mermaid {
        vec![Block::Mermaid(document.source.clone())]
    } else {
        blocks(&document.source)
    };
    for block in body {
        match block {
            Block::Heading(text, level) => {
                let size = if level == HeadingLevel::H1 { 37. } else { 30. };
                page.text(&text, size, INK, false);
            }
            Block::Paragraph(text) => page.text(&text, 23., BODY, false),
            Block::Code(text) => page.text(text.trim_end_matches('\n'), 21., BODY, true),
            Block::Mermaid(source) => {
                let sketch = Sketch {
                    title: "Mermaid".into(),
                    source,
                    format: Format::Mermaid,
                };
                match sketch.image() {
                    Ok(image) => page.picture(image, 420),
                    Err(_) => {
                        page.text("Unable to display this Mermaid diagram.", 23., BODY, false)
                    }
                }
            }
            Block::Picture { href, alt } => match picture(repo, &href) {
                Ok(image) => page.picture(image, 250),
                Err(error) if loading(&error) => return (page, true),
                Err(_) if !alt.trim().is_empty() => page.text(&alt, 21., BODY, false),
                Err(_) => {}
            },
        }
        page.gap(11.);
    }
    (page, false)
}

fn picture(repo: &Repository, href: &str) -> Result<Image, String> {
    if href.contains(':') || href.starts_with("//") {
        return Err("Only snapshot images are shown.".into());
    }
    let path = markdown_preview::relative_file("README.md", href).ok_or("Invalid image path.")?;
    let index = repo
        .files
        .iter()
        .position(|file| file.path == path)
        .ok_or("Image is absent from this snapshot.")?;
    image_preview::decode_named(&path, &repo.asset_bytes(index, false)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_retains_markdown_headings_diagrams_and_snapshot_image_sources() {
        let readme = "# Release\n\nRead the **notes**.\n\n![Preview](docs/scene.png)\n\n```mermaid\nflowchart LR\nA --> B\n```";
        assert_eq!(
            blocks(readme),
            vec![
                Block::Heading("Release".into(), HeadingLevel::H1),
                Block::Paragraph("Read the notes.".into()),
                Block::Picture {
                    href: "docs/scene.png".into(),
                    alt: "Preview".into()
                },
                Block::Mermaid("flowchart LR\nA --> B\n".into()),
            ]
        );
    }
}
