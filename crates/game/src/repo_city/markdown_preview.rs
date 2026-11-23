//! Markdown previews for source documents and review comments.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::fmt::Write;

use super::*;

pub(super) fn is_markdown(path: &str) -> bool {
    file_preview::format(path) == Some(file_preview::Format::Markdown)
}

// Limit a tablet page's layout work. Every byte of the source remains reachable.
pub(super) const PAGE_BYTES: usize = 48 * 1024;
const PAGE_LINES: usize = 256;

pub(super) fn page_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    let mut start = 0;
    while start < source.len() {
        let limit = source.floor_char_boundary((start + PAGE_BYTES).min(source.len()));
        let end = source[start..limit]
            .match_indices('\n')
            .nth(PAGE_LINES - 1)
            .map_or(limit, |(line, _)| start + line + 1);
        let boundary = if end < source.len() {
            let page = &source[start..end];
            page.rfind("\n\n")
                .map(|index| start + index + 2)
                .filter(|&index| index - start >= page.len() / 2)
                .or_else(|| {
                    page.rfind('\n')
                        .map(|index| start + index + 1)
                        .filter(|&index| index - start >= page.len() / 2)
                })
                .unwrap_or(end)
        } else {
            end
        };
        offsets.push(boundary);
        start = boundary;
    }
    if offsets.len() == 1 {
        offsets.push(0);
    }
    offsets
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn document_page_offsets(source: &str) -> Vec<u32> {
    let mut offset = 0;
    let pages = page_offsets(source);
    std::iter::once(0)
        .chain(pages.windows(2).map(|page| {
            offset += source[page[0]..page[1]].encode_utf16().count() as u32;
            offset
        }))
        .collect()
}

pub(super) fn document_html(path: &str, source: &str) -> String {
    if is_markdown(path) {
        return markdown_html(source);
    }
    source_html(path, source).0
}

fn source_html(path: &str, source: &str) -> (String, bool) {
    let mut html = String::with_capacity(source.len() + 64);
    let mut styled = false;
    html.push_str("<pre class=\"source-code\"><code>");
    for (raw, line) in source
        .split_inclusive('\n')
        .zip(syntax::highlight(path, source))
    {
        let mut cursor = 0;
        for span in line.spans {
            let start = span.bytes.start;
            let end = span.bytes.end;
            if span.kind == syntax::Kind::Plain || start < cursor || end <= start {
                continue;
            }
            let (Some(before), Some(token)) = (raw.get(cursor..start), raw.get(start..end)) else {
                continue;
            };
            escape_source(&mut html, before);
            let [red, green, blue] =
                code_texture::ink(span.kind).map(|channel| (channel * 255.).round() as u8);
            write!(
                html,
                "<span style=\"color:#{red:02x}{green:02x}{blue:02x}\">"
            )
            .unwrap();
            escape_source(&mut html, token);
            html.push_str("</span>");
            styled = true;
            cursor = end;
        }
        escape_source(&mut html, &raw[cursor..]);
    }
    html.push_str("</code></pre>\n");
    (html, styled)
}

fn escape_source(html: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(ch),
        }
    }
}

pub(super) fn events(source: &str) -> Parser<'_> {
    Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn markdown_html(source: &str) -> String {
    let mut html = String::new();
    let mut links = Vec::new();
    pulldown_cmark::html::push_html(
        &mut html,
        events(source)
            .filter_map(|event| match event {
                Event::Html(text) | Event::InlineHtml(text) => Some(Event::Text(text)),
                // Preserve the image label without fetching embedded resources.
                Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
                Event::Start(Tag::Link { ref dest_url, .. }) => {
                    let permitted = safe_link(dest_url);
                    links.push(permitted);
                    permitted.then_some(event)
                }
                Event::End(TagEnd::Link) => (links.pop() == Some(true)).then_some(event),
                event => Some(event),
            })
            .flat_map(|event| match event {
                // Group each cell's inline runs into one text layout for native HTML UI.
                Event::Start(Tag::TableCell) => [Some(event), Some(Event::Html("<p>".into()))],
                Event::End(TagEnd::TableCell) => [Some(Event::Html("</p>".into())), Some(event)],
                event => [Some(event), None],
            })
            .flatten(),
    );
    html
}

fn safe_link(url: &str) -> bool {
    let url = url.trim();
    !url.chars().any(|c| c.is_control() || c == '\\')
        && (url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("mailto:")
            || (!url.contains(':') && !url.starts_with("//")))
}

#[derive(Component)]
pub(super) struct SourceFile {
    pub path: String,
    pub before: bool,
}

pub(super) fn relative_file(source: &str, href: &str) -> Option<String> {
    let href = href.split(['?', '#']).next()?;
    let href = percent_encoding::percent_decode_str(href)
        .decode_utf8()
        .ok()?;
    if href.is_empty() || href.chars().any(|c| c.is_control() || c == '\\') {
        return None;
    }
    let mut path = Vec::new();
    if !href.starts_with('/')
        && let Some((directory, _)) = source.rsplit_once('/')
    {
        path.extend(directory.split('/'));
    }
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                path.pop()?;
            }
            part => path.push(part),
        }
    }
    (!path.is_empty()).then(|| path.join("/"))
}

fn follow_links(
    mut signals: MessageReader<bevy_markup::signals::ElementSignal>,
    sources: Query<&SourceFile>,
    parents: Query<&ChildOf>,
    repo: Option<Res<Repo>>,
    reading: Res<reading::Settings>,
    #[cfg(not(target_arch = "wasm32"))] mut preview: ResMut<Request>,
    #[cfg(not(target_arch = "wasm32"))] mut image: ResMut<image_preview::Request>,
) {
    for signal in signals.read() {
        if signal.name != "open" {
            continue;
        }
        let Some(href) = signal.payload["href"]
            .as_str()
            .map(str::trim)
            .filter(|href| safe_link(href))
        else {
            continue;
        };
        if href.starts_with("https://")
            || href.starts_with("http://")
            || href.starts_with("mailto:")
        {
            #[cfg(target_arch = "wasm32")]
            if let Some(window) = web_sys::window() {
                let _ = window.open_with_url_and_target_and_features(
                    href,
                    "_blank",
                    "noopener,noreferrer",
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(error) = native_github::open_browser_url(href) {
                warn!("{error}");
            }
            continue;
        }
        let Some(source) = parents
            .iter_ancestors(signal.target)
            .find_map(|entity| sources.get(entity).ok())
        else {
            continue;
        };
        let Some(path) = relative_file(&source.path, href) else {
            continue;
        };
        let Some((index, file)) = repo.as_ref().and_then(|repo| {
            repo.0.files.iter().enumerate().find(|(_, file)| {
                file.path == path
                    && if source.before {
                        file.before.is_some()
                    } else {
                        file.after.is_some()
                    }
            })
        }) else {
            continue;
        };
        let end = reading.block_lines as usize;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = index;
            open_browser(&file.path, source.before, 0, end, false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if image_preview::supported(&file.path) {
            image.0 = Some((index, source.before));
        } else {
            preview.0 = Some((index, source.before, 0..end, false));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default)]
pub(super) struct Request(pub Option<(usize, bool, std::ops::Range<usize>, bool)>);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = previewMarkdown)]
    pub(super) fn open_browser(path: &str, before: bool, start: usize, end: usize, edit: bool);
}

pub(super) fn register(app: &mut App) {
    use bevy::asset::io::embedded::EmbeddedAssetRegistry;
    use bevy_markup::prelude::{FontFaces, FontFamilies, GenericFamily};

    app.add_plugins(bevy_markup::BevyMarkupPlugin)
        .add_systems(Update, follow_links);
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, refresh_browser_source);
    #[cfg(target_arch = "wasm32")]
    let sans = FontSource::list([Handle::<Font>::default(), MATH_FONT, EMOJI_FONT]);
    #[cfg(not(target_arch = "wasm32"))]
    let sans = FontSource::sans_serif();
    #[cfg(target_arch = "wasm32")]
    let mono = FontSource::list([Handle::<Font>::default(), MATH_FONT, EMOJI_FONT]);
    #[cfg(not(target_arch = "wasm32"))]
    let mono = FontSource::monospace();
    app.world_mut()
        .resource_mut::<FontFamilies>()
        .insert("Markdown Sans", FontFaces::new(sans))
        .insert("Markdown Mono", FontFaces::new(mono))
        .set_generic(GenericFamily::SansSerif, "Markdown Sans")
        .set_generic(GenericFamily::Monospace, "Markdown Mono");
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    registry.insert_asset(
        PathBuf::new(),
        std::path::Path::new("repo-city/markdown.html"),
        include_bytes!("markdown_preview.html"),
    );
    registry.insert_asset(
        PathBuf::new(),
        std::path::Path::new("repo-city/markdown.css"),
        include_bytes!("markdown_preview.css"),
    );
}

pub(super) fn draw(
    parent: &mut ChildSpawnerCommands,
    document: String,
    source: Option<SourceFile>,
) -> Entity {
    use bevy_markup::prelude::{HtmlStylesheet, HtmlUi, TemplateContext};
    let mut node = parent.spawn(Node {
        width: percent(100.),
        flex_shrink: 0.,
        ..default()
    });
    if let Some(source) = source {
        node.insert(source);
    }
    let entity = node.id();
    node.commands().queue(move |world: &mut World| {
        let Some(assets) = world.get_resource::<AssetServer>() else {
            return;
        };
        let template = assets.load("embedded://repo-city/markdown.html");
        let style = assets.load("embedded://repo-city/markdown.css");
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert((
                HtmlUi::new(template),
                HtmlStylesheet(style),
                TemplateContext::new().with("document", &document),
            ));
        }
    });
    entity
}

#[cfg(target_arch = "wasm32")]
#[derive(Component)]
struct BrowserSource {
    path: String,
    text: String,
    version: u32,
}

#[cfg(target_arch = "wasm32")]
pub(super) fn draw_source(parent: &mut ChildSpawnerCommands, path: &str, text: &str) -> Entity {
    let (html, styled) = source_html(path, text);
    let entity = draw(parent, html, None);
    if !styled {
        parent.commands().entity(entity).insert(BrowserSource {
            path: path.into(),
            text: text.into(),
            version: repo_world::browser_syntax_version(),
        });
    }
    entity
}

#[cfg(target_arch = "wasm32")]
fn refresh_browser_source(
    mut commands: Commands,
    mut previews: Query<(
        Entity,
        &mut BrowserSource,
        &mut bevy_markup::prelude::TemplateContext,
    )>,
) {
    let version = repo_world::browser_syntax_version();
    for (entity, mut source, mut context) in &mut previews {
        if source.version == version {
            continue;
        }
        source.version = version;
        let (html, styled) = source_html(&source.path, &source.text);
        if styled {
            *context = bevy_markup::prelude::TemplateContext::new().with("document", &html);
            commands.entity(entity).remove::<BrowserSource>();
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ReadDocument {
    pub path: String,
    pub blob: String,
    pub before: bool,
    pub rows: usize,
}

impl ReadDocument {
    pub(super) fn matches(&self, file: &repo_world::File) -> bool {
        let blob = if self.before {
            &file.before
        } else {
            &file.after
        };
        self.path == file.path
            && matches!(
                file_preview::format(&file.path),
                Some(file_preview::Format::Markdown | file_preview::Format::Text)
            )
            && self.rows != 0
            && blob
                .as_ref()
                .is_some_and(|blob| blob.id == self.blob && self.rows <= blob.size as usize)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default)]
pub(super) struct ReadQueue(pub Option<ReadDocument>);

#[cfg(target_arch = "wasm32")]
pub(super) fn take_browser_read() -> Option<ReadDocument> {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeMarkdownRead)]
        fn take_read() -> String;
    }
    serde_json::from_str(&take_read()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pages_cover_all_bytes_without_splitting_utf8() {
        let text = format!("# Recent\n\n{}\n\n# Oldest\n", "😀α".repeat(PAGE_BYTES));
        let pages = page_offsets(&text);
        assert!(pages.len() > 2);
        assert_eq!(pages.first(), Some(&0));
        assert_eq!(pages.last(), Some(&text.len()));
        assert!(pages.iter().all(|&offset| text.is_char_boundary(offset)));
        assert!(
            pages
                .windows(2)
                .all(|pair| (1..=PAGE_BYTES).contains(&(pair[1] - pair[0])))
        );
        assert!(text[pages[pages.len() - 2]..].ends_with("# Oldest\n"));
    }

    #[test]
    fn resolves_relative_links_inside_the_current_snapshot() {
        assert_eq!(
            relative_file("guides/setup.md", "../docs/MIT%20License#terms").as_deref(),
            Some("docs/MIT License")
        );
        assert_eq!(
            relative_file("guides/setup.md", "/LICENSE").as_deref(),
            Some("LICENSE")
        );
        assert!(relative_file("README.md", "../LICENSE").is_none());
        assert!(relative_file("README.md", "docs%5cLICENSE").is_none());
        assert!(relative_file("README.md", "#terms").is_none());
    }

    #[test]
    fn markdown_renders_commonmark_and_treats_html_as_text() {
        let html = markdown_html(
            "# Title\n\n**Bold** and `code`.\n\n<a onclick=alert(1)>unsafe</a>\n\n[unsafe](javascript:alert%281%29)\n",
        );
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>Bold</strong>"));
        assert!(html.contains("<code>code</code>"));
        assert!(!html.contains("<a onclick") && !html.contains("href=\"javascript:"));
        assert!(is_markdown("docs/README.MD"));
        assert!(!is_markdown("docs/README.md.bak"));
        let table = markdown_html(
            "| Action | Control |\n| --- | --- |\n| Read **all** | Press **P** once |\n",
        );
        assert!(table.contains("<td><p>Press <strong>P</strong> once</p></td>"));
    }

    #[test]
    fn source_previews_color_tokens_and_escape_the_file_without_losing_indentation() {
        let source = "    let answer = \"Привет <script>&\";\n\n    // note\n";
        let html = document_html("example.rs", source);
        assert!(html.contains("    <span style=\"color:#cc91ff\">let</span>"));
        assert!(html.contains("<span style=\"color:#b8db80\">"));
        assert!(html.contains("Привет &lt;script&gt;&amp;"));
        assert!(html.contains("\n\n    <span"));
        assert!(!html.contains("<script>"));
        let text = document_html("LICENSE", "    Unformatted terms.\n");
        assert!(text.contains("    Unformatted terms.\n"));
        assert!(!text.contains("<span"));
    }

    #[test]
    fn completion_belongs_to_the_read_snapshot_side() {
        let blob = |id: &str| repo_world::Blob {
            id: id.into(),
            size: 32,
            mode: "100644".into(),
        };
        let mut file = repo_world::File {
            path: "README.md".into(),
            before: Some(blob("old")),
            after: Some(blob("new")),
            change: Change::Modified,
            position: [0.; 2],
        };
        let mut read = ReadDocument {
            path: file.path.clone(),
            blob: "new".into(),
            before: false,
            rows: 2,
        };
        assert!(read.matches(&file));
        read.before = true;
        assert!(!read.matches(&file));
        read.before = false;
        file.after = Some(blob("next"));
        assert!(!read.matches(&file));
    }
}
