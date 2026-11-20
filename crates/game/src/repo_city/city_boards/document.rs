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

pub(super) fn render(
    set: &WorldSet,
    repo: &Repository,
    document: Option<&Sketch>,
) -> (Image, bool) {
    let mut canvas = canvas(
        set,
        document.map_or("Documents", |document| &document.title),
    );
    let Some(document) = document else {
        canvas.text(
            "Open a checkout with a README or stage an MCP sketch.",
            34.,
            144.,
            24.,
            |_| BODY,
        );
        return (canvas.image(), false);
    };
    let mut pictures = Vec::new();
    let mut waiting = false;
    let mut top = 108.;
    let body = if document.format == Format::Mermaid {
        vec![Block::Mermaid(document.source.clone())]
    } else {
        blocks(&document.source)
    };
    for block in body {
        if top >= HEIGHT as f32 - 35. {
            break;
        }
        match block {
            Block::Heading(text, level) => {
                let size = if level == HeadingLevel::H1 { 37. } else { 30. };
                top = lines(&mut canvas, &text, top, size, INK);
            }
            Block::Paragraph(text) => top = lines(&mut canvas, &text, top, 23., BODY),
            Block::Code(text) => {
                for line in text.lines() {
                    top = lines(&mut canvas, line, top, 21., BODY);
                }
            }
            Block::Mermaid(source) => {
                let sketch = Sketch {
                    title: "Mermaid".into(),
                    source,
                    format: Format::Mermaid,
                };
                match sketch.image() {
                    Ok(image) => top = fit(&mut pictures, image, top, 420),
                    Err(_) => {
                        top = lines(
                            &mut canvas,
                            "Unable to display this Mermaid diagram.",
                            top,
                            23.,
                            BODY,
                        )
                    }
                }
            }
            Block::Picture { href, alt } => match picture(repo, &href) {
                Ok(image) => top = fit(&mut pictures, image, top, 250),
                Err(error) if loading(&error) => {
                    waiting = true;
                    break;
                }
                Err(_) if !alt.trim().is_empty() => top = lines(&mut canvas, &alt, top, 21., BODY),
                Err(_) => {}
            },
        }
        top += 11.;
    }

    let mut result = canvas.image();
    if !pictures.is_empty()
        && let Some(bytes) = result.data.take()
    {
        let mut background =
            ::image::RgbaImage::from_raw(WIDTH, HEIGHT, bytes).expect("board dimensions");
        for (picture, top) in pictures {
            let x = i64::from((WIDTH - picture.width()) / 2);
            ::image::imageops::overlay(&mut background, &picture, x, top);
        }
        result.data = Some(background.into_raw());
    }
    (result, waiting)
}

fn lines(
    canvas: &mut code_texture::Canvas,
    text: &str,
    mut top: f32,
    size: f32,
    ink: [f32; 3],
) -> f32 {
    let capacity = (956. / (size * 0.62)) as usize;
    let mut row = String::new();
    for word in text.split_whitespace() {
        if !row.is_empty() && row.chars().count() + word.chars().count() + 1 > capacity {
            top += size + 5.;
            if top >= HEIGHT as f32 - 12. {
                return top;
            }
            canvas.text(&row, 34., top, size, |_| ink);
            row.clear();
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.extend(word.chars().take(capacity));
    }
    if !row.is_empty() {
        top += size + 5.;
        if top < HEIGHT as f32 - 12. {
            canvas.text(&row, 34., top, size, |_| ink);
        }
    }
    top
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

fn fit(
    pictures: &mut Vec<(::image::RgbaImage, i64)>,
    image: Image,
    top: f32,
    height_limit: u32,
) -> f32 {
    let (width, height) = (image.width(), image.height());
    let available = ((HEIGHT as f32 - top - 17.) as u32).min(height_limit);
    if available < 48 {
        return top;
    }
    let Some(image) = image
        .data
        .and_then(|bytes| ::image::RgbaImage::from_raw(width, height, bytes))
    else {
        return top;
    };
    let scale = (956. / width as f32)
        .min(available as f32 / height as f32)
        .min(1.5);
    let resized = ::image::imageops::resize(
        &image,
        (width as f32 * scale).round().max(1.) as u32,
        (height as f32 * scale).round().max(1.) as u32,
        ::image::imageops::FilterType::Triangle,
    );
    let ending = top + resized.height() as f32;
    pictures.push((resized, (top + 6.) as i64));
    ending
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
