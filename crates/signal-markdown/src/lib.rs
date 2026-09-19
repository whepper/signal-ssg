//! `signal-markdown`: Markdown and front-matter parsing.
//!
//! Comrak lives behind this crate boundary. `signal-core` never sees the
//! arena/lifetime-heavy AST; it only ever receives owned [`RenderedBody`]
//! values (defined in `signal-core`) produced here.
//!
//! The pipeline is intentionally one-way:
//!
//! ```text
//! Markdown -> Comrak -> owned derived representation
//! ```
//!
//! No second full Markdown AST is introduced here.

#![forbid(unsafe_code)]

pub mod front_matter;

mod alerts;
mod code;

pub use front_matter::{split_front_matter, FrontMatter, FrontMatterError};
// `RenderedBody`/`Heading`/`Toc`/`CodeBlock` are canonical core data;
// re-exported here so existing consumers keep a single import path for
// parsing outputs.
pub use signal_core::{CodeBlock, Heading, RenderedBody, Toc, TocItem};

use comrak::adapters::{HeadingAdapter, HeadingMeta};
use comrak::arena_tree::NodeEdge;
use comrak::nodes::{AstNode, NodeValue, Sourcepos};
use comrak::{format_html_with_plugins, parse_document, Arena, Options, Plugins};
use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::sync::Mutex;

/// Parse Markdown into its owned derived representation.
///
/// Single Comrak parse, then AST-level transforms strictly inside this
/// boundary, then one render pass:
///
/// ```text
/// source → parse → drop author raw HTML → expand alerts → render code
///   → render HTML (headings recorded with anchors in the same pass)
/// ```
///
/// Raw HTML has no passthrough in Signal's content model: author `HtmlBlock`
/// and `HtmlInline` nodes are detached before rendering, so enabling the
/// renderer passthrough below only ever exposes Signal-generated structural
/// markup (code wrappers, alerts, Mermaid). Structural link/image
/// extraction stays intentionally simple line/scan based.
pub fn parse_markdown(source: &str) -> RenderedBody {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    // Required for Signal-generated structural nodes (code wrappers,
    // asides, Mermaid blocks). Safe: author HTML nodes are detached first.
    options.render.unsafe_ = true;

    let arena = Arena::new();
    let root = parse_document(&arena, source, &options);
    neutralize_user_html(root);
    neutralize_unsafe_urls(root);
    alerts::expand_alerts(&arena, root);
    let code_blocks = code::transform_code(root);
    let plain_text = extract_plain_text(root);

    let state = SignalHeadings::default();
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&state);
    let mut html_bytes = Vec::new();
    format_html_with_plugins(root, &options, &mut html_bytes, &plugins)
        .expect("rendering to memory cannot fail");
    let html = String::from_utf8(html_bytes).expect("Comrak emits UTF-8");
    let headings = state.take_headings();

    RenderedBody {
        html,
        headings,
        links: extract_links(source),
        images: extract_images(source),
        code_blocks,
        plain_text,
        word_count: source.split_whitespace().count(),
    }
}

/// Detach author raw-HTML nodes: Signal's content model carries no raw HTML.
/// Runs before any Signal-generated `HtmlBlock` exists, so generated
/// structural markup is never affected.
fn neutralize_user_html<'a>(root: &'a AstNode<'a>) {
    let targets: Vec<&'a AstNode<'a>> = root
        .traverse()
        .filter_map(|edge| match edge {
            NodeEdge::Start(node) => match &node.data.borrow().value {
                NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => Some(node),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for node in targets {
        node.detach();
    }
}

/// Neutralize author-controlled link/image destinations that fail Signal's
/// shared URL-safety policy, before any HTML is generated.
///
/// The policy lives in `signal-core` (`is_safe_author_url`) and is the same
/// one front-matter images use. A rejected destination is removed at the AST
/// level: the node is unwrapped so its inline children — link text or image
/// alt text, including nested emphasis — remain as ordinary text. No
/// `href`/`src` can then be emitted for a rejected destination, and nodes
/// after it are untouched.
///
/// Targets are collected before mutation, so unwrapping one node cannot
/// invalidate the walk (the same pattern alerts use). Pre-order guarantees a
/// parent link is unwrapped before an unsafe child image inside it.
fn neutralize_unsafe_urls<'a>(root: &'a AstNode<'a>) {
    use signal_core::{is_safe_author_url, UrlUse};
    let targets: Vec<&'a AstNode<'a>> = root
        .traverse()
        .filter_map(|edge| match edge {
            NodeEdge::Start(node) => match &node.data.borrow().value {
                NodeValue::Link(link) if !is_safe_author_url(&link.url, UrlUse::Link) => Some(node),
                NodeValue::Image(link) if !is_safe_author_url(&link.url, UrlUse::Image) => {
                    Some(node)
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    for node in targets {
        unwrap_node(node);
    }
}

/// Replace `node` with its children, preserving order, then detach the node
/// itself (and therefore its unsafe destination).
fn unwrap_node<'a>(node: &'a AstNode<'a>) {
    let children: Vec<&'a AstNode<'a>> = node.children().collect();
    let mut anchor = node;
    for child in children {
        child.detach();
        anchor.insert_after(child);
        anchor = child;
    }
    node.detach();
}

/// Fold plain heading text into a URL-safe fragment base.
///
/// Rules: Unicode-lowercase; keep alphanumeric characters (Unicode-aware),
/// `-`, and `_`; collapse each whitespace run to a single `-`; drop
/// everything else; trim leading/trailing `-`. An empty result becomes
/// `"section"` (uniqueness is applied afterwards). Punctuation such as
/// `?`, `!`, `:`, `(`, `)`, `'`, and `.` disappears; `&` disappears rather
/// than becoming `and`.
pub fn anchor_base(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch);
        } else if ch.is_whitespace() && !out.is_empty() {
            pending_dash = true;
        }
        // All other characters are dropped.
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        return "section".to_string();
    }
    trimmed.to_string()
}

/// Make a fragment base unique within the page: first use is bare,
/// repeats gain `-1`, `-2`, … suffixes (the widely-used GitHub convention,
/// matching observable Hugo output).
pub fn anchor_unique(base: &str, seen: &mut HashSet<String>) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    let mut counter = 1;
    loop {
        let candidate = format!("{base}-{counter}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

/// Escape a fragment id for an HTML attribute value.
///
/// The generator only emits `[alphanumeric - _]`, so this is structural
/// defense rather than a reachable path — ids can never break out of the
/// attribute whatever content produced them.
fn escape_attr(value: &str) -> String {
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

/// Comrak heading adapter: assigns deterministic anchors and records the
/// normalized headings in render order.
///
/// Comrak calls `enter`/`exit` around each heading's inline content in
/// document order (headings cannot nest), so one shared queue pairs ids
/// with tags exactly. The flattened [`HeadingMeta::content`] is the plain
/// heading text — the same string stored on [`Heading::text`].
#[derive(Debug, Default)]
struct SignalHeadings {
    state: Mutex<HeadingState>,
}

#[derive(Debug, Default)]
struct HeadingState {
    seen: HashSet<String>,
    headings: VecDeque<Heading>,
}

impl SignalHeadings {
    fn take_headings(&self) -> Vec<Heading> {
        std::mem::take(&mut self.state.lock().expect("heading lock").headings).into()
    }
}

impl HeadingAdapter for SignalHeadings {
    fn enter(
        &self,
        output: &mut dyn Write,
        heading: &HeadingMeta,
        _sourcepos: Option<Sourcepos>,
    ) -> std::io::Result<()> {
        let mut state = self.state.lock().expect("heading lock");
        let id = anchor_unique(&anchor_base(&heading.content), &mut state.seen);
        state.headings.push_back(Heading {
            level: heading.level,
            text: heading.content.clone(),
            id: id.clone(),
        });
        write!(output, "<h{} id=\"{}\">", heading.level, escape_attr(&id))
    }

    fn exit(&self, output: &mut dyn Write, heading: &HeadingMeta) -> std::io::Result<()> {
        write!(output, "</h{}>", heading.level)
    }
}

/// Extract plain search text from the transformed tree.
///
/// Runs after alert expansion and code replacement, so alert bodies
/// contribute as normal prose (markers already stripped), fenced code and
/// generated markup are `HtmlBlock` nodes (skipped wholesale), and detached
/// author HTML is simply gone. Inline code counts as prose; soft/line
/// breaks and block boundaries become single spaces via the final
/// whitespace normalization. Never touches rendered HTML.
fn extract_plain_text<'a>(root: &'a AstNode<'a>) -> String {
    use comrak::nodes::NodeCode;
    let mut out = String::new();
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(node) => match &node.data.borrow().value {
                NodeValue::Text(literal) => out.push_str(literal),
                NodeValue::Code(NodeCode { literal, .. }) => out.push_str(literal),
                NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
                NodeValue::Math(math) => out.push_str(&math.literal),
                _ => {}
            },
            NodeEdge::End(node) => match &node.data.borrow().value {
                NodeValue::Paragraph
                | NodeValue::Heading(_)
                | NodeValue::Item(_)
                | NodeValue::TableCell
                | NodeValue::TableRow(_)
                | NodeValue::BlockQuote => out.push(' '),
                _ => {}
            },
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract non-image `[text](destination)` link destinations.
fn extract_links(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Image; skip the whole `![alt](dest)` span so it is not
            // misreported as a link.
            if let Some(close_bracket) = find_byte(bytes, i + 2, b']') {
                let after = close_bracket + 1;
                if after < bytes.len() && bytes[after] == b'(' {
                    if let Some(close_paren) = find_byte(bytes, after + 1, b')') {
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'[' {
            if let Some(close_bracket) = find_byte(bytes, i + 1, b']') {
                let after = close_bracket + 1;
                if after < bytes.len() && bytes[after] == b'(' {
                    if let Some(close_paren) = find_byte(bytes, after + 1, b')') {
                        let dest = source[after + 1..close_paren].trim().to_string();
                        if !dest.is_empty() {
                            out.push(dest);
                        }
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Extract `![alt](destination)` image destinations.
fn extract_images(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'!' && bytes[i + 1] == b'[' {
            if let Some(close_bracket) = find_byte(bytes, i + 2, b']') {
                let after = close_bracket + 1;
                if after < bytes.len() && bytes[after] == b'(' {
                    if let Some(close_paren) = find_byte(bytes, after + 1, b')') {
                        let dest = source[after + 1..close_paren].trim().to_string();
                        if !dest.is_empty() {
                            out.push(dest);
                        }
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

fn find_byte(haystack: &[u8], from: usize, needle: u8) -> Option<usize> {
    haystack
        .iter()
        .skip(from)
        .position(|b| *b == needle)
        .map(|p| from + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_html_and_counts_words() {
        let body = parse_markdown("# Hello\n\nWorld.\n");
        assert!(
            body.html.contains("<h1 id=\"hello\">"),
            "html: {}",
            body.html
        );
        assert!(body.html.contains("Hello"), "html: {}", body.html);
        assert_eq!(body.word_count, 3);
    }

    #[test]
    fn extracts_headings_links_images() {
        let src = "# Title\n\nSee [other](/other/) and ![alt](/img.png).\n\n## Sub\n";
        let body = parse_markdown(src);
        assert_eq!(body.headings.len(), 2);
        assert_eq!(body.headings[0].level, 1);
        assert_eq!(body.headings[0].text, "Title");
        assert_eq!(body.headings[0].id, "title");
        assert_eq!(body.headings[1].id, "sub");
        assert_eq!(body.links, vec!["/other/".to_string()]);
        assert_eq!(body.images, vec!["/img.png".to_string()]);
    }

    #[test]
    fn images_are_not_reported_as_links() {
        let body = parse_markdown("![alt](/img.png)\n");
        assert!(body.links.is_empty());
        assert_eq!(body.images.len(), 1);
    }

    #[test]
    fn plain_text_holds_prose_headings_and_inline_code() {
        let body = parse_markdown("# Title Here\n\nSome *emphasized* text and `inline code`.\n");
        assert_eq!(
            body.plain_text,
            "Title Here Some emphasized text and inline code."
        );
    }

    #[test]
    fn plain_text_excludes_fenced_code_and_markup() {
        let body = parse_markdown(
            "Intro paragraph.\n\n```rust\nfn secret() {}\n```\n\n> [!NOTE]\n> Alert body words.\n",
        );
        assert!(
            body.plain_text.contains("Intro paragraph."),
            "{}",
            body.plain_text
        );
        assert!(
            body.plain_text.contains("Alert body words."),
            "{}",
            body.plain_text
        );
        assert!(
            !body.plain_text.contains("fn secret"),
            "{}",
            body.plain_text
        );
        assert!(!body.plain_text.contains("[!NOTE]"), "{}", body.plain_text);
        assert!(!body.plain_text.contains('<'), "{}", body.plain_text);
    }

    #[test]
    fn plain_text_excludes_mermaid_source() {
        let body =
            parse_markdown("Before.\n\n```mermaid\nflowchart LR\n    A --> B\n```\n\nAfter.\n");
        assert_eq!(body.plain_text, "Before. After.");
    }

    #[test]
    fn plain_text_preserves_unicode_and_collapses_whitespace() {
        let body = parse_markdown("Über alles   weiter.\nSecond   line.\n");
        assert_eq!(body.plain_text, "Über alles weiter. Second line.");
    }

    #[test]
    fn anchor_base_folds_text_deterministically() {
        let cases = [
            ("Installation", "installation"),
            ("What's new?", "whats-new"),
            ("Getting  started\tguide", "getting-started-guide"),
            ("Deploy: staging & prod (v2)", "deploy-staging-prod-v2"),
            ("  padded  ", "padded"),
            ("Über den Tellerrand", "über-den-tellerrand"),
            ("日本語の見出し", "日本語の見出し"),
            ("snake_case kept", "snake_case-kept"),
            ("a/b\\c:d", "abcd"),
            ("---", "section"),
            ("", "section"),
        ];
        for (text, base) in cases {
            assert_eq!(anchor_base(text), base, "text {text:?}");
        }
    }

    #[test]
    fn anchor_duplicates_gain_ordered_suffixes() {
        let mut seen = HashSet::new();
        assert_eq!(anchor_unique("installation", &mut seen), "installation");
        assert_eq!(anchor_unique("installation", &mut seen), "installation-1");
        assert_eq!(anchor_unique("installation", &mut seen), "installation-2");
        // A natural `-1` base still collides deterministically, never reuses.
        assert_eq!(
            anchor_unique("installation-1", &mut seen),
            "installation-1-1"
        );
    }

    #[test]
    fn headings_strip_inline_markup_and_anchor_duplicates() {
        let src = "## Using `code` and *emphasis*\n\nText.\n\n## Using `code` and *emphasis*\n";
        let body = parse_markdown(src);
        assert_eq!(body.headings.len(), 2);
        assert_eq!(body.headings[0].text, "Using code and emphasis");
        assert_eq!(body.headings[0].id, "using-code-and-emphasis");
        assert_eq!(body.headings[1].id, "using-code-and-emphasis-1");
    }

    #[test]
    fn html_heading_ids_match_normalized_headings() {
        let src = "## Installation\n\nBody.\n\n### Config & tuning!\n";
        let body = parse_markdown(src);
        assert!(
            body.html.contains("<h2 id=\"installation\">"),
            "html: {}",
            body.html
        );
        assert!(
            body.html.contains("<h3 id=\"config-tuning\">"),
            "html: {}",
            body.html
        );
        let ids: Vec<&str> = body.headings.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["installation", "config-tuning"]);
    }

    #[test]
    fn unicode_and_empty_headings_anchor_safely() {
        let body = parse_markdown("## Über alles\n\n## 日本語\n");
        assert_eq!(body.headings[0].id, "über-alles");
        assert_eq!(body.headings[1].id, "日本語");
        assert!(
            body.html.contains("id=\"über-alles\""),
            "html: {}",
            body.html
        );
        // No raw `<`, `>`, `&`, or `"` can leak into an id attribute.
        for heading in &body.headings {
            assert!(
                heading
                    .id
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
                "id {:?} has unsafe chars",
                heading.id
            );
        }
    }

    // --- Slice 14A: author-controlled URL safety at the AST boundary ---

    /// Assert the rendered HTML carries no executable/unsafe attribute
    /// value, case-insensitively.
    fn assert_no_unsafe_attributes(html: &str) {
        let lower = html.to_ascii_lowercase();
        for bad in [
            "href=\"javascript:",
            "href=\"data:",
            "href=\"vbscript:",
            "src=\"javascript:",
            "src=\"data:",
            "src=\"vbscript:",
        ] {
            assert!(
                !lower.contains(bad),
                "unsafe attribute {bad:?} survived in: {html}"
            );
        }
    }

    #[test]
    fn unsafe_link_destinations_are_neutralized() {
        // Comrak entity-decodes destinations before this runs, so encoded
        // schemes must be caught as their literal form.
        for dest in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "JaVaScRiPt:alert(1)",
            "javascript&#58;alert(1)",
            "&#106;avascript:alert(1)",
            "&#x6a;avascript:alert(1)",
            "data:text/html,x",
            "DATA:text/html,x",
            "vbscript:msgbox(1)",
            "VBScript:msgbox(1)",
            "  javascript:alert(1)  ",
            "",
        ] {
            let body = parse_markdown(&format!("[click]({dest})"));
            assert_no_unsafe_attributes(&body.html);
            assert!(
                !body.html.contains("href="),
                "dest {dest:?} produced an anchor: {}",
                body.html
            );
            assert!(
                body.html.contains("click"),
                "dest {dest:?} lost its text: {}",
                body.html
            );
        }
    }

    #[test]
    fn percent_encoded_scheme_is_treated_as_a_relative_path() {
        // `%` cannot start a URL scheme, so browsers parse this as a
        // relative path, not as `javascript:`. The policy allows it and the
        // destination must stay relative (never rewritten into a scheme).
        let body = parse_markdown("[g](%6aavascript:alert(1))");
        assert_no_unsafe_attributes(&body.html);
        assert!(
            body.html.contains("href=\"%6aavascript:alert(1)\""),
            "{}",
            body.html
        );
    }

    #[test]
    fn unsafe_image_destinations_are_neutralized() {
        for dest in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(2)",
            "data:image/svg+xml,x",
            "vbscript:x",
        ] {
            let body = parse_markdown(&format!("![alt text]({dest})"));
            assert_no_unsafe_attributes(&body.html);
            assert!(
                !body.html.contains("src="),
                "dest {dest:?} produced an image: {}",
                body.html
            );
            // The alt text survives as ordinary text.
            assert!(
                body.html.contains("alt text"),
                "dest {dest:?}: {}",
                body.html
            );
        }
    }

    #[test]
    fn safe_markdown_urls_are_preserved() {
        for dest in [
            "relative/path",
            "/path",
            "../path",
            "/page?x=1",
            "/page#section",
            "https://example.com/page",
            "http://example.com/page",
            "https://example.com/page?x=1#section",
            "mailto:someone@example.com",
        ] {
            let body = parse_markdown(&format!("[t]({dest})"));
            assert!(
                body.html.contains(&format!("href=\"{dest}\"")),
                "safe dest {dest:?} was altered: {}",
                body.html
            );
        }
        let image = parse_markdown("![alt](/img/a.png)");
        assert!(image.html.contains("src=\"/img/a.png\""), "{}", image.html);
        let remote = parse_markdown("![alt](https://cdn.example/a.png)");
        assert!(
            remote.html.contains("src=\"https://cdn.example/a.png\""),
            "{}",
            remote.html
        );
    }

    #[test]
    fn mailto_is_allowed_for_links_but_rejected_for_images() {
        let link = parse_markdown("[mail](mailto:someone@example.com)");
        assert!(link.html.contains("href=\"mailto:someone@example.com\""));
        // Not a meaningful image source: neutralized like any other rejected
        // scheme, with the alt text preserved as text.
        let image = parse_markdown("![mail](mailto:someone@example.com)");
        assert!(!image.html.contains("src="), "{}", image.html);
        assert!(image.html.contains("mail"), "{}", image.html);
    }

    #[test]
    fn url_neutralization_preserves_surrounding_ast() {
        let body = parse_markdown(
            "Before [safe](/a) middle [bad](javascript:alert(1)) after [also](https://example.com/).",
        );
        for expected in ["Before", "middle", "after", "bad"] {
            assert!(
                body.html.contains(expected),
                "missing {expected}: {}",
                body.html
            );
        }
        assert!(body.html.contains("href=\"/a\""), "{}", body.html);
        assert!(
            body.html.contains("href=\"https://example.com/\""),
            "{}",
            body.html
        );
        // Only the two safe links survive as anchors.
        assert_eq!(body.html.matches("href=").count(), 2, "{}", body.html);
    }

    #[test]
    fn url_neutralization_keeps_inline_markup_and_order() {
        // Emphasis inside rejected link text is preserved as inline markup.
        let body = parse_markdown("[*em*](javascript:x) tail");
        assert!(body.html.contains("<em>em</em>"), "{}", body.html);
        assert!(!body.html.contains("href="), "{}", body.html);
        assert!(body.html.contains("tail"), "{}", body.html);

        // safe → unsafe → safe keeps order and drops only the unsafe anchor.
        let ordered = parse_markdown("[first](javascript:x) [second](/safe) [third](javascript:y)");
        let html = &ordered.html;
        let first = html.find("first").expect("first present");
        let second = html.find("second").expect("second present");
        let third = html.find("third").expect("third present");
        assert!(first < second && second < third, "order preserved: {html}");
        assert_eq!(html.matches("href=").count(), 1, "{html}");
    }

    #[test]
    fn generated_html_contains_no_executable_author_urls() {
        // One document, every hostile shape; only the safe link is an anchor.
        let src = "A [js](javascript:alert(1)) B [Js](JaVaScRiPt:alert(2)) \
C [entity](javascript&#58;alert(3)) D [data](data:text/html,x) E [vb](vbscript:x) \
F <javascript:alert(4)> G ![img](javascript:alert(5)) H [safe](/ok).";
        let body = parse_markdown(src);
        assert_no_unsafe_attributes(&body.html);
        for expected in ["A", "B", "C", "D", "E", "F", "G", "H"] {
            assert!(
                body.html.contains(expected),
                "missing {expected}: {}",
                body.html
            );
        }
        assert!(body.html.contains("href=\"/ok\""), "{}", body.html);
        assert_eq!(body.html.matches("href=").count(), 1, "{}", body.html);
    }

    #[test]
    fn raw_html_still_detached_alongside_url_policy() {
        // The pre-existing raw-HTML boundary is unchanged by this slice.
        let body = parse_markdown(
            "<script>alert(1)</script>\n\n[ok](/a) [bad](javascript:x)\n\n<iframe src=\"x\"></iframe>",
        );
        assert!(!body.html.contains("<script"), "{}", body.html);
        assert!(!body.html.contains("<iframe"), "{}", body.html);
        assert!(body.html.contains("href=\"/a\""), "{}", body.html);
        assert_eq!(body.html.matches("href=").count(), 1, "{}", body.html);
    }
}
