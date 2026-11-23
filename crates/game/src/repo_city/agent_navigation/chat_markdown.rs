//! Display parsed Markdown as selectable VibePad text and native controls.
use super::*;
use pulldown_cmark::HeadingLevel;

pub(in super::super) fn draw(parent: &mut ChildSpawnerCommands, source: &str, repo: &Repository) {
    let mut message = Message::default();
    for event in markdown_preview::events(source) {
        match event {
            Event::Text(text) if message.code.is_some() => {
                if let Some(code) = &mut message.code {
                    code.push_str(&text);
                }
            }
            Event::Text(text) if message.link.is_none() => {
                message
                    .runs
                    .extend(referenced_runs(&text, repo, message.style));
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                message.push(text.into_string());
            }
            Event::Code(text) => {
                let link = message
                    .link
                    .clone()
                    .or_else(|| resolve(repo, &text).map(MessageLink::Source));
                message.runs.push(TextRun {
                    text: text.into_string(),
                    style: RunStyle {
                        code: true,
                        ..message.style
                    },
                    link,
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                message.flush(parent);
                message.heading = Some(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                message.flush(parent);
                message.heading = None;
            }
            Event::Start(Tag::List(first)) => {
                message.flush(parent);
                message.lists.push(first);
            }
            Event::End(TagEnd::List(_)) => {
                message.flush(parent);
                message.lists.pop();
            }
            Event::Start(Tag::Item) => {
                message.flush(parent);
                let marker = if let Some(Some(number)) = message.lists.last_mut() {
                    let marker = format!("{number}. ");
                    *number = number.saturating_add(1);
                    marker
                } else {
                    "• ".into()
                };
                message.runs.push(TextRun::plain(marker));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                message.flush(parent);
                message.quotes += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                message.flush(parent);
                message.quotes = message.quotes.saturating_sub(1);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                message.flush(parent);
                message.code = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(code) = message.code.take() {
                    draw_code(parent, code);
                }
            }
            Event::Start(Tag::Table(_)) => {
                message.flush(parent);
                message.table = Some(Vec::new());
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(cells) = &mut message.table {
                    cells.push(std::mem::take(&mut message.runs));
                }
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(cells) = &mut message.table {
                    table_row(
                        parent,
                        std::mem::take(cells),
                        matches!(event, Event::End(TagEnd::TableHead)),
                    );
                }
            }
            Event::End(TagEnd::Table) => message.table = None,
            Event::Start(Tag::Link { dest_url, .. }) => {
                message.link = message_link(repo, &dest_url)
            }
            Event::End(TagEnd::Link) => message.link = None,
            Event::Start(Tag::Strong) => message.style.bold = true,
            Event::End(TagEnd::Strong) => message.style.bold = false,
            Event::Start(Tag::Emphasis) => message.style.italic = true,
            Event::End(TagEnd::Emphasis) => message.style.italic = false,
            Event::Start(Tag::Strikethrough) => message.style.strike = true,
            Event::End(TagEnd::Strikethrough) => message.style.strike = false,
            Event::End(TagEnd::Paragraph | TagEnd::Item) => message.flush(parent),
            Event::SoftBreak => message.push(" "),
            Event::HardBreak => message.push("\n"),
            Event::TaskListMarker(done) => message.push(if done { "☑ " } else { "☐ " }),
            Event::Rule => {
                message.flush(parent);
                parent.spawn((
                    Node {
                        height: px(1.),
                        margin: UiRect::vertical(px(6.)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(44, 73, 81)),
                ));
            }
            _ => {}
        }
    }
    message.flush(parent);
}

#[derive(Default)]
struct Message {
    runs: Vec<TextRun>,
    style: RunStyle,
    heading: Option<HeadingLevel>,
    lists: Vec<Option<u64>>,
    quotes: usize,
    code: Option<String>,
    link: Option<MessageLink>,
    table: Option<Vec<Vec<TextRun>>>,
}

impl Message {
    fn push(&mut self, text: impl Into<String>) {
        self.runs.push(TextRun {
            text: text.into(),
            style: self.style,
            link: self.link.clone(),
        });
    }

    fn flush(&mut self, parent: &mut ChildSpawnerCommands) {
        if self.runs.is_empty() {
            return;
        }
        let font = match self.heading {
            Some(heading) => native_panel::panel_font()
                .with_font_size(match heading {
                    HeadingLevel::H1 => 24.,
                    HeadingLevel::H2 => 21.,
                    _ => 18.,
                })
                .with_font_weight(FontWeight::BOLD),
            None => native_panel::panel_font(),
        };
        let runs = std::mem::take(&mut self.runs);
        let indent = self.lists.len().saturating_sub(1).min(6) + self.quotes.min(6);
        if indent > 0 || self.heading.is_some() {
            let mut node = Node {
                min_width: px(0.),
                margin: UiRect::left(px(indent as f32 * 12.)),
                ..default()
            };
            if self.heading.is_some() {
                node.margin.top = px(6.);
            }
            if self.quotes > 0 {
                node.border.left = px(2.);
                node.padding.left = px(8.);
            }
            parent
                .spawn((node, BorderColor::all(Color::srgb_u8(55, 107, 109))))
                .with_children(|row| {
                    draw_runs(row, runs, &font);
                });
        } else {
            draw_runs(parent, runs, &font);
        }
    }
}

fn table_row(parent: &mut ChildSpawnerCommands, cells: Vec<Vec<TextRun>>, header: bool) {
    let mut font = native_panel::panel_font();
    if header {
        font.weight = FontWeight::BOLD;
    }
    parent
        .spawn(Node {
            width: percent(100.),
            column_gap: px(2.),
            ..default()
        })
        .with_children(|row| {
            for runs in cells {
                row.spawn((
                    Node {
                        width: px(0.),
                        min_width: px(0.),
                        flex_grow: 1.,
                        padding: UiRect::all(px(6.)),
                        ..default()
                    },
                    BackgroundColor(if header {
                        Color::srgb_u8(20, 52, 59)
                    } else {
                        Color::srgb_u8(10, 34, 43)
                    }),
                ))
                .with_children(|cell| {
                    draw_runs(cell, runs, &font);
                });
            }
        });
}
