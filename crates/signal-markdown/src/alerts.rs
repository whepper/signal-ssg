//! GitHub-style alert/callout blocks (`> [!WARNING]`).
//!
//! Alerts are transformed at the AST level, inside the single Comrak parse:
//! a `BlockQuote` whose first paragraph starts with a `[!KIND]` marker is
//! spliced into an `<aside>` wrapper (open marker, original children,
//! close marker). Children stay real parsed nodes — inline Markdown,
//! lists, and fenced code inside alerts render through the normal
//! pipeline, and headings inside alerts keep working anchors.
//!
//! Known kinds: NOTE, TIP, IMPORTANT, WARNING, CAUTION. Anything else
//! (including a missing marker) is left as a plain blockquote.

use comrak::arena_tree::Node;
use comrak::nodes::{Ast, AstNode, LineColumn, NodeHtmlBlock, NodeValue};
use comrak::Arena;
use std::cell::RefCell;

/// Recognized alert kinds with their display labels.
const KINDS: [(&str, &str); 5] = [
    ("NOTE", "Note"),
    ("TIP", "Tip"),
    ("IMPORTANT", "Important"),
    ("WARNING", "Warning"),
    ("CAUTION", "Caution"),
];

/// Parse a leading `[!KIND]` marker: returns the marker length in bytes and
/// the lowercase kind. The word must be ASCII letters; anything else (or an
/// unknown kind) is not an alert.
fn parse_marker(text: &str) -> Option<(usize, &'static str, String)> {
    let rest = text.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let word = &rest[..end];
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let upper = word.to_ascii_uppercase();
    let label = KINDS.iter().find(|(kind, _)| *kind == upper)?.1;
    Some((2 + end + 1, label, upper.to_ascii_lowercase()))
}

/// Strip the marker plus one optional following space or tab. The remainder
/// (if any) stays as body text.
fn strip_marker(text: &str, marker_len: usize) -> &str {
    let rest = &text[marker_len..];
    rest.strip_prefix(' ')
        .or_else(|| rest.strip_prefix('\t'))
        .unwrap_or(rest)
}

fn html_node<'a>(
    arena: &'a Arena<AstNode<'a>>,
    pos: LineColumn,
    literal: String,
) -> &'a AstNode<'a> {
    let node: &'a AstNode<'a> = arena.alloc(Node::new(RefCell::new(Ast::new(
        NodeValue::HtmlBlock(NodeHtmlBlock {
            block_type: 6,
            literal,
        }),
        pos,
    ))));
    node
}

/// Transform one alert blockquote in place: strip the marker, splice
/// `[open, children…, close]` in its position, detach the blockquote.
/// Returns true when the node was an alert.
fn transform_alert<'a>(arena: &'a Arena<AstNode<'a>>, blockquote: &'a AstNode<'a>) -> bool {
    // First block child must be a paragraph whose first inline child is
    // text starting with the marker.
    let paragraph = match blockquote.first_child() {
        Some(node) if matches!(node.data.borrow().value, NodeValue::Paragraph) => node,
        _ => return false,
    };
    let text_node = match paragraph.first_child() {
        Some(node) if matches!(node.data.borrow().value, NodeValue::Text(_)) => node,
        _ => return false,
    };
    let (marker_len, label, kind) = match &text_node.data.borrow().value {
        NodeValue::Text(literal) => match parse_marker(literal) {
            Some(parsed) => parsed,
            None => return false,
        },
        _ => return false,
    };

    let stripped = {
        let borrowed = text_node.data.borrow();
        let NodeValue::Text(literal) = &borrowed.value else {
            return false;
        };
        strip_marker(literal, marker_len).to_string()
    };
    if stripped.is_empty() {
        text_node.detach();
        if paragraph.first_child().is_none() {
            paragraph.detach();
        }
    } else if let NodeValue::Text(literal) = &mut text_node.data.borrow_mut().value {
        *literal = stripped;
    }

    let pos = blockquote.data.borrow().sourcepos.start;
    // ARIA semantics follow the alert kind: urgent kinds are live
    // announcements (`role="alert"`), the rest are advisory notes. The
    // element and classes stay theme-styling hooks; the role is semantics.
    let role = if kind == "warning" || kind == "caution" {
        "alert"
    } else {
        "note"
    };
    let open = html_node(
        arena,
        pos,
        format!(
            "<aside class=\"alert alert-{kind}\" data-alert=\"{kind}\" role=\"{role}\">\n<p class=\"alert-title\">{label}</p>"
        ),
    );
    let close = html_node(arena, pos, "</aside>".to_string());
    blockquote.insert_before(open);
    let mut anchor = open;
    let children: Vec<&'a AstNode<'a>> = blockquote.children().collect();
    for child in children {
        child.detach();
        anchor.insert_after(child);
        anchor = child;
    }
    anchor.insert_after(close);
    blockquote.detach();
    true
}

/// Expand all alert blockquotes in the parsed tree.
///
/// Nodes are collected before mutation (arena pointers stay valid across
/// detaches), so nested alerts (`> > [!NOTE]`) transform innermost-first
/// without invalidating the walk.
pub(crate) fn expand_alerts<'a>(arena: &'a Arena<AstNode<'a>>, root: &'a AstNode<'a>) {
    let blockquotes: Vec<&'a AstNode<'a>> = root
        .traverse()
        .filter_map(|edge| match edge {
            comrak::arena_tree::NodeEdge::Start(node) => match &node.data.borrow().value {
                NodeValue::BlockQuote => Some(node),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for blockquote in blockquotes {
        // A transformed node is detached; detached subtrees visited later
        // are still valid nodes, and re-checking them is harmless (the
        // marker text is gone, so they no longer match).
        transform_alert(arena, blockquote);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_markdown;

    #[test]
    fn marker_parses_known_kinds() {
        assert_eq!(
            parse_marker("[!WARNING] rest"),
            Some((10, "Warning", "warning".to_string()))
        );
        assert_eq!(
            parse_marker("[!note]"),
            Some((7, "Note", "note".to_string()))
        );
        assert_eq!(parse_marker("[!BOGUS] x"), None);
        assert_eq!(parse_marker("[!] x"), None);
        assert_eq!(parse_marker("[!NO TE] x"), None);
        assert_eq!(parse_marker(" [!NOTE] x"), None);
        assert_eq!(parse_marker("[NOTE] x"), None);
    }

    #[test]
    fn warning_renders_semantic_aside() {
        let body = parse_markdown("> [!WARNING]\n> Be careful.\n");
        assert!(
            body.html.contains(
                "<aside class=\"alert alert-warning\" data-alert=\"warning\" role=\"alert\">"
            ),
            "html: {}",
            body.html
        );
        assert!(
            body.html.contains("<p class=\"alert-title\">Warning</p>"),
            "html: {}",
            body.html
        );
        assert!(body.html.contains("Be careful."), "html: {}", body.html);
        assert!(body.html.contains("</aside>"), "html: {}", body.html);
        assert!(!body.html.contains("blockquote"), "html: {}", body.html);
    }

    #[test]
    fn alert_roles_follow_urgency() {
        // Urgent kinds announce themselves; advisory kinds are notes.
        for (kind, role) in [
            ("NOTE", "note"),
            ("TIP", "note"),
            ("IMPORTANT", "note"),
            ("WARNING", "alert"),
            ("CAUTION", "alert"),
        ] {
            let body = parse_markdown(&format!("> [!{kind}]\n> Body.\n"));
            assert!(
                body.html.contains(&format!("role=\"{role}\"")),
                "{kind} must carry role={role:?}: {}",
                body.html
            );
        }
    }

    #[test]
    fn alert_body_keeps_multiline_and_inline_markdown() {
        let body = parse_markdown(
            "> [!TIP]\n> First **bold** line.\n>\n> - list item\n> - [link](/go/)\n",
        );
        assert!(
            body.html.contains("<strong>bold</strong>"),
            "html: {}",
            body.html
        );
        assert!(
            body.html.contains("<li>list item</li>"),
            "html: {}",
            body.html
        );
        assert!(
            body.html.contains("<a href=\"/go/\">link</a>"),
            "html: {}",
            body.html
        );
        assert!(body.html.contains("<aside"), "html: {}", body.html);
    }

    #[test]
    fn unknown_kind_stays_blockquote() {
        let body = parse_markdown("> [!BOGUS]\n> Just a quote.\n");
        assert!(body.html.contains("<blockquote>"), "html: {}", body.html);
        assert!(!body.html.contains("<aside"), "html: {}", body.html);
        assert!(body.html.contains("[!BOGUS]"), "html: {}", body.html);
    }

    #[test]
    fn plain_blockquote_untouched() {
        let body = parse_markdown("> Rotation keeps the blast radius bounded.\n");
        assert!(body.html.contains("<blockquote>"), "html: {}", body.html);
        assert!(!body.html.contains("<aside"), "html: {}", body.html);
    }

    #[test]
    fn marker_inside_code_is_not_an_alert() {
        let body = parse_markdown("```text\n> [!WARNING]\n```\n");
        assert!(!body.html.contains("<aside"), "html: {}", body.html);
    }

    #[test]
    fn alert_body_cannot_inject_markup() {
        // Author raw HTML is dropped at parse (Signal has no raw-HTML
        // passthrough); surrounding text survives.
        let body =
            parse_markdown("> [!CAUTION]\n> Press <button onclick=\"x\">here</button> now.\n");
        assert!(!body.html.contains("<button"), "html: {}", body.html);
        assert!(!body.html.contains("onclick"), "html: {}", body.html);
        assert!(body.html.contains("Press"), "html: {}", body.html);
        assert!(body.html.contains("here"), "html: {}", body.html);
        assert!(body.html.contains("<aside"), "html: {}", body.html);
    }

    #[test]
    fn marker_only_line_leaves_no_empty_paragraph() {
        let body = parse_markdown("> [!NOTE]\n> Body text.\n");
        assert!(!body.html.contains("<p></p>"), "html: {}", body.html);
        assert!(body.html.contains("Body text."), "html: {}", body.html);
    }
}
