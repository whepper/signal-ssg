//! Fenced code-block rendering: highlighting, copy affordance, Mermaid.
//!
//! All fenced code blocks pass through one AST-level transform inside the
//! Markdown boundary. Rendering outcomes:
//!
//! ```text
//! ```mermaid        →  <pre class="mermaid"> (escaped source, client renders)
//! ```rust etc.      →  <div class="code-block"> wrapper with highlighted
//!                       spans, language hook, and copy button
//! ``` (bare)        →  same wrapper without language hooks
//! ```
//!
//! Highlighting uses Comrak's `SyntectAdapter` (class spans, no inline
//! styles, deterministic class output) composed directly — no separate
//! highlighter dependency. Unknown languages fall back to the adapter's
//! plain-text rendering, deterministically. Indented code blocks keep
//! Comrak's default plain rendering but are still recorded.

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::nodes::{AstNode, NodeValue};
use comrak::plugins::syntect::SyntectAdapter;
use signal_core::CodeBlock;
use std::sync::OnceLock;

/// Shared highlighter: syntax/theme sets load once per process. Loading is
/// expensive; the sets are immutable and deterministic after load.
static HIGHLIGHTER: OnceLock<SyntectAdapter> = OnceLock::new();

fn highlighter() -> &'static SyntectAdapter {
    HIGHLIGHTER.get_or_init(|| SyntectAdapter::new(None))
}

/// Escape text for HTML element content or attribute values.
pub(crate) fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// First info-string token (`rust` in ```` ```rust linenos ````), trimmed.
/// `None` when the fence carries no info string.
fn info_language(info: &str) -> Option<String> {
    let token = info.split_whitespace().next()?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Whether a fenced block is a diagram for client-side rendering.
fn is_mermaid(language: Option<&str>) -> bool {
    language.is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
}

/// Render one fenced block to its final HTML fragment.
///
/// Mermaid source is preserved verbatim as escaped text — Signal never
/// executes it; the site's client renderer owns that. Everything else is
/// syntax-highlighted into class spans inside a copyable wrapper.
fn render_fenced(language: Option<&str>, literal: &str) -> String {
    if is_mermaid(language) {
        return format!(
            "<pre class=\"mermaid\">{}</pre>",
            escape_html(literal.trim_end_matches('\n'))
        );
    }
    let mut highlighted = Vec::new();
    if highlighter()
        .write_highlighted(&mut highlighted, language, literal)
        .is_err()
    {
        highlighted.clear();
        highlighted.extend_from_slice(escape_html(literal).as_bytes());
    }
    let highlighted = String::from_utf8(highlighted).unwrap_or_else(|_| escape_html(literal));
    let mut out = String::from("<div class=\"code-block\"");
    if let Some(lang) = language {
        out.push_str(&format!(" data-language=\"{}\"", escape_html(lang)));
    }
    out.push_str("><pre><code");
    if let Some(lang) = language {
        out.push_str(&format!(" class=\"language-{}\"", escape_html(lang)));
    }
    out.push('>');
    out.push_str(&highlighted);
    out.push_str("</code></pre>");
    out.push_str(
        "<button type=\"button\" class=\"code-block-copy\" data-code-copy>Copy</button></div>",
    );
    out
}

/// Walk the parsed tree once: record every code block (fenced and indented)
/// in document order, and replace fenced blocks in place with their final
/// HTML. Replacing the node *value* keeps tree positions intact — no
/// sibling surgery, no reordering, no second parse.
pub(crate) fn transform_code<'a>(root: &'a AstNode<'a>) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let targets: Vec<&'a AstNode<'a>> = root
        .traverse()
        .filter_map(|edge| match edge {
            comrak::arena_tree::NodeEdge::Start(node) => match &node.data.borrow().value {
                NodeValue::CodeBlock(_) => Some(node),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for node in targets {
        let (fenced, info, literal) = match &node.data.borrow().value {
            NodeValue::CodeBlock(ncb) => (ncb.fenced, ncb.info.clone(), ncb.literal.clone()),
            _ => continue,
        };
        let language = if fenced { info_language(&info) } else { None };
        blocks.push(CodeBlock {
            language: language.clone(),
            source: literal.trim_end_matches('\n').to_string(),
        });
        if fenced {
            let html = render_fenced(language.as_deref(), &literal);
            node.data.borrow_mut().value = NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                block_type: 0,
                literal: html,
            });
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_language_takes_first_token() {
        assert_eq!(info_language("rust"), Some("rust".to_string()));
        assert_eq!(
            info_language("  python linenos  "),
            Some("python".to_string())
        );
        assert_eq!(info_language(""), None);
        assert_eq!(info_language("   "), None);
    }

    #[test]
    fn rust_block_highlights_with_language_hooks() {
        let html = render_fenced(Some("rust"), "fn main() {}\n");
        assert!(
            html.starts_with("<div class=\"code-block\" data-language=\"rust\">"),
            "{html}"
        );
        assert!(html.contains("<code class=\"language-rust\">"), "{html}");
        // Syntect class spans, not inline styles.
        assert!(html.contains("<span class="), "{html}");
        assert!(!html.contains("style="), "{html}");
        assert!(html.contains("data-code-copy"), "{html}");
        assert!(html.ends_with("</button></div>"), "{html}");
    }

    #[test]
    fn bare_fence_renders_without_language_hooks() {
        let html = render_fenced(None, "plain\n");
        assert!(html.starts_with("<div class=\"code-block\">"), "{html}");
        assert!(html.contains("<pre><code>"), "{html}");
        assert!(!html.contains("data-language"), "{html}");
    }

    #[test]
    fn unknown_language_renders_deterministically() {
        let first = render_fenced(Some("nosuchlang"), "hello <world>\n");
        let second = render_fenced(Some("nosuchlang"), "hello <world>\n");
        assert_eq!(first, second);
        assert!(first.contains("data-language=\"nosuchlang\""), "{first}");
        assert!(first.contains("hello &lt;world&gt;"), "{first}");
        assert!(!first.contains("<world>"), "{first}");
    }

    #[test]
    fn code_escapes_hostile_content() {
        let html = render_fenced(Some("rust"), "</code><script>alert(1)</script>\n");
        assert!(!html.contains("</code><script>"), "{html}");
        assert!(!html.contains("<script>alert"), "{html}");
        // Highlighting splits entities across spans, so assert on the
        // encoded brackets themselves rather than a contiguous run.
        assert!(html.contains("&lt;"), "{html}");
        assert!(html.contains("&gt;"), "{html}");
    }

    #[test]
    fn mermaid_preserves_source_as_text() {
        let src = "flowchart LR\n    A --> B & C\n";
        let html = render_fenced(Some("mermaid"), src);
        assert_eq!(
            html,
            "<pre class=\"mermaid\">flowchart LR\n    A --&gt; B &amp; C</pre>"
        );
        // Case-insensitive info match, but recorded language keeps authorship.
        let html_upper = render_fenced(Some("Mermaid"), src);
        assert!(
            html_upper.starts_with("<pre class=\"mermaid\">"),
            "{html_upper}"
        );
    }

    #[test]
    fn mermaid_never_emits_raw_markup() {
        let html = render_fenced(Some("mermaid"), "</pre><script>alert(1)</script>\n");
        assert!(!html.contains("</pre><script>"), "{html}");
        assert!(html.contains("&lt;/pre&gt;"), "{html}");
    }
}
