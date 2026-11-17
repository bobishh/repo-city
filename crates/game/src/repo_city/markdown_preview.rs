//! Markdown previews for source documents and review comments.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::*;

pub(super) fn is_markdown(path: &str) -> bool {
    PathBuf::from(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["md", "markdown", "mdown", "mkd"]
                .iter()
                .any(|kind| ext.eq_ignore_ascii_case(kind))
        })
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
pub(super) struct Request(pub Option<(usize, bool)>);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = previewMarkdown)]
    pub(super) fn open_browser(path: &str, before: bool);
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
            && is_markdown(&file.path)
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
