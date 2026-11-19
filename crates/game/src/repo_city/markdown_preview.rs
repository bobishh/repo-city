//! Markdown previews for source documents and review comments.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::*;

pub(super) fn is_markdown(path: &str) -> bool {
    file_preview::format(path) == Some(file_preview::Format::Markdown)
}

pub(super) fn document_html(path: &str, source: &str) -> String {
    if is_markdown(path) {
        return markdown_html(source);
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(
        &mut html,
        [
            Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Indented)),
            Event::Text(source.into()),
            Event::End(TagEnd::CodeBlock),
        ]
        .into_iter(),
    );
    html
}

fn events(source: &str) -> impl Iterator<Item = Event<'_>> {
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

    app.add_plugins(bevy_markup::BevyMarkupPlugin);
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

pub(super) fn draw(parent: &mut ChildSpawnerCommands, document: String) -> Entity {
    use bevy_markup::prelude::{HtmlStylesheet, HtmlUi, TemplateContext};
    let mut node = parent.spawn(Node {
        width: percent(100.),
        flex_shrink: 0.,
        ..default()
    });
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
            && (1..=1024 * 1024).contains(&self.rows)
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
