//! Syntax colors come from real Tree-sitter nodes; multiline constructs are parsed whole.
use std::ops::Range;
#[cfg(not(target_arch = "wasm32"))]
use tree_sitter::{Language, Node, Parser};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
}
#[derive(Clone, Debug)]
pub struct Span {
    pub bytes: Range<usize>,
    pub kind: Kind,
}
#[derive(Clone, Debug)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub spans: Vec<Span>,
}

#[cfg(not(target_arch = "wasm32"))]
fn language(path: &str) -> Option<Language> {
    Some(match path.rsplit('.').next()? {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "rb" | "rake" => tree_sitter_ruby::LANGUAGE.into(),
        "ex" | "exs" => tree_sitter_elixir::LANGUAGE.into(),
        "elm" => tree_sitter_elm::LANGUAGE.into(),
        _ => return None,
    })
}
#[cfg(not(target_arch = "wasm32"))]
// Whole-node colors stop traversal so comment/string contents keep their color.
fn whole_kind(name: &str) -> Option<Kind> {
    match name {
        name if name.contains("comment") => Some(Kind::Comment),
        "predefined_type" => Some(Kind::Type),
        "string" | "string_literal" | "raw_string_literal" | "template_string" | "char_literal"
        | "heredoc_body" | "string_content" | "char_constant_expr" => Some(Kind::String),
        "number" | "number_constant_expr" => Some(Kind::Number),
        name if name.contains("integer") || name.contains("float") => Some(Kind::Number),
        "type_identifier" | "primitive_type" | "constant" | "upper_case_identifier" => {
            Some(Kind::Type)
        }
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_function_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let callable = parent.kind().contains("function")
        || parent.kind().contains("call")
        || parent.kind().contains("method");
    callable
        && ["name", "function", "method"]
            .into_iter()
            .any(|field| parent.child_by_field_name(field) == Some(node))
}

#[cfg(not(target_arch = "wasm32"))]
fn leaf_kind(node: Node, source: &str) -> Kind {
    let Some(text) = source.get(node.byte_range()) else {
        return Kind::Plain;
    };
    match text {
        _ if !node.is_named() && text.chars().next().is_some_and(char::is_alphabetic) => {
            Kind::Keyword
        }
        "true" | "false" | "nil" | "null" | "undefined" | "self" | "Self" => Kind::Keyword,
        _ if is_function_name(node) => Kind::Function,
        _ => Kind::Plain,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn walk(node: Node, source: &str, spans: &mut Vec<Span>) {
    if let Some(kind) = whole_kind(node.kind()) {
        spans.push(Span {
            bytes: node.byte_range(),
            kind,
        });
        return;
    }
    if node.child_count() == 0 {
        let kind = leaf_kind(node, source);
        if kind != Kind::Plain {
            spans.push(Span {
                bytes: node.byte_range(),
                kind,
            });
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, source, spans);
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn highlight(path: &str, source: &str) -> Vec<Line> {
    let mut spans = Vec::new();
    if let Some(language) = language(path) {
        let mut parser = Parser::new();
        if parser.set_language(&language).is_ok() {
            if let Some(tree) = parser.parse(source, None) {
                walk(tree.root_node(), source, &mut spans);
            }
        }
    }
    spans.sort_by_key(|s| s.bytes.start);
    lines_with_spans(source, &spans)
}

fn lines_with_spans(source: &str, spans: &[Span]) -> Vec<Line> {
    let mut offset = 0;
    let mut first = 0;
    source
        .split_inclusive('\n')
        .enumerate()
        .map(|(i, raw)| {
            let text = raw.trim_end_matches('\n').trim_end_matches('\r');
            let end = offset + text.len();
            while first < spans.len() && spans[first].bytes.end <= offset {
                first += 1;
            }
            let line_spans = spans[first..]
                .iter()
                .take_while(|s| s.bytes.start < end)
                .map(|s| Span {
                    bytes: s.bytes.start.max(offset) - offset..s.bytes.end.min(end) - offset,
                    kind: s.kind,
                })
                .collect();
            offset += raw.len();
            Line {
                number: i + 1,
                text: text.into(),
                spans: line_spans,
            }
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub fn highlight(path: &str, source: &str) -> Vec<Line> {
    let packed = crate::browser_syntax_spans(path, source);
    let spans = packed
        .chunks_exact(3)
        .filter_map(|span| {
            let kind = match span[2] {
                1 => Kind::Keyword,
                2 => Kind::String,
                3 => Kind::Comment,
                4 => Kind::Number,
                5 => Kind::Type,
                6 => Kind::Function,
                _ => return None,
            };
            let bytes = span[0] as usize..span[1] as usize;
            (!bytes.is_empty() && source.get(bytes.clone()).is_some())
                .then_some(Span { bytes, kind })
        })
        .collect::<Vec<_>>();
    lines_with_spans(source, &spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typescript_builtin_types_are_not_literals() {
        let lines = highlight(
            "example.ts",
            "const x: number = 12; const y: string = 'hi';",
        );
        for word in ["number", "string"] {
            assert!(
                lines[0]
                    .spans
                    .iter()
                    .any(|s| s.kind == Kind::Type && &lines[0].text[s.bytes.clone()] == word)
            );
        }
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|s| s.kind == Kind::Number && &lines[0].text[s.bytes.clone()] == "12")
        );
    }
    #[test]
    fn multiline_comments_strings_and_unicode_keep_byte_ranges() {
        let lines = highlight(
            "test.rs",
            "/* first\nλ second */\nfn hello() { let s = \"/* not comment */ λ\"; }\n",
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|s| s.kind == Kind::Comment && s.bytes.start == 0)
        );
        assert!(lines[2].spans.iter().any(|s| s.kind == Kind::Function));
        assert!(lines[2].spans.iter().any(|s| s.kind == Kind::String));
        assert!(!lines[2].spans.iter().any(|s| s.kind == Kind::Comment));
        for line in &lines {
            for span in &line.spans {
                assert!(line.text.get(span.bytes.clone()).is_some());
            }
        }
    }
    #[test]
    fn unsupported_text_is_preserved_without_fake_tokens() {
        let lines = highlight("readme.md", "# fn fake()\nλ");
        assert_eq!(lines[1].text, "λ");
        assert!(lines.iter().all(|l| l.spans.is_empty()));
    }
}
