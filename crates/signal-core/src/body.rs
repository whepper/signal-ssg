//! Owned derived representation of parsed content.
//!
//! [`RenderedBody`] is canonical normalized data owned by `signal-core`.
//! `signal-markdown` produces these values (via Comrak, behind its boundary)
//! but the types live here so the [`crate::SiteModel`] can embed them without
//! depending on any Markdown implementation.
//!
//! Headings carry deterministic fragment anchors ([`Heading::id`]) assigned
//! once at parse time. The table of contents ([`Toc`]) is a pure projection
//! over those headings — templates never parse HTML to discover structure.

use serde::{Deserialize, Serialize};

/// One extracted heading, in document order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    /// Heading level 1..=6.
    pub level: u8,
    /// Plain heading text (no markup).
    pub text: String,
    /// Deterministic fragment anchor, unique within the page.
    ///
    /// Assigned by the Markdown boundary together with the rendered HTML,
    /// so `href="#{id}"` fragment links always resolve. See
    /// `signal-markdown` for the generation algorithm.
    #[serde(default)]
    pub id: String,
}

/// One table-of-contents row with nested children.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocItem {
    /// Heading level.
    pub level: u8,
    /// Plain heading text (no markup, no HTML).
    pub text: String,
    /// Fragment anchor; `href` is always `#` + this value.
    pub id: String,
    /// Nested sub-headings, in document order.
    #[serde(default)]
    pub children: Vec<TocItem>,
}

/// Table of contents: hierarchy of [`TocItem`] in document order.
///
/// Built by [`Toc::build`] from normalized headings. Level-1 headings are
/// excluded — the page title owns the H1 — and levels 2–6 nest by level.
/// Templates render `href="#{{ item.id }}"` links; no HTML is involved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toc {
    /// Top-level items, in document order.
    #[serde(default)]
    pub items: Vec<TocItem>,
}

impl Toc {
    /// Build a TOC from normalized headings, preserving order and nesting.
    ///
    /// A heading nests under the nearest preceding heading of a lower
    /// level; equal or higher levels close the open subtree. Skipped levels
    /// (e.g. H2 directly followed by H4) still nest — depth follows level
    /// order, not arithmetic adjacency.
    pub fn build(headings: &[Heading]) -> Self {
        let mut roots: Vec<TocItem> = Vec::new();
        // Stack of (level, index-path) for the currently open chain.
        let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
        for heading in headings.iter().filter(|h| (2..=6).contains(&h.level)) {
            let item = TocItem {
                level: heading.level,
                text: heading.text.clone(),
                id: heading.id.clone(),
                children: Vec::new(),
            };
            while stack
                .last()
                .is_some_and(|(level, _)| *level >= heading.level)
            {
                stack.pop();
            }
            if let Some((_, path)) = stack.last() {
                let parent = navigate(&mut roots, path);
                parent.children.push(item);
                let mut child_path = path.clone();
                child_path.push(parent.children.len() - 1);
                stack.push((heading.level, child_path));
            } else {
                roots.push(item);
                stack.push((heading.level, vec![roots.len() - 1]));
            }
        }
        Self { items: roots }
    }

    /// Whether the TOC has any entries.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn navigate<'a>(roots: &'a mut [TocItem], path: &[usize]) -> &'a mut TocItem {
    let mut node = &mut roots[path[0]];
    for index in &path[1..] {
        node = &mut node.children[*index];
    }
    node
}

/// One fenced or indented code block, in document order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeBlock {
    /// First info-string token for fenced blocks (`rust`, `mermaid`, …),
    /// as authored and trimmed; `None` for indented blocks and bare fences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Literal source text with trailing newlines trimmed.
    #[serde(default)]
    pub source: String,
}

/// Owned derived representation of a Markdown document.
///
/// Rendered HTML plus extracted structure: headings (with fragment anchors
/// for [`Toc`] projection), link/image destinations, code blocks (so
/// templates can gate per-language assets such as diagram renderers without
/// parsing HTML), and a word count.
/// Deliberately owned (`String`/`Vec`) so it can live in normalized data
/// without lifetimes. Extended only when a real migration requirement
/// demonstrates the need.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedBody {
    /// Rendered HTML fragment.
    #[serde(default)]
    pub html: String,
    /// Extracted headings in document order.
    #[serde(default)]
    pub headings: Vec<Heading>,
    /// Extracted link destinations in document order.
    #[serde(default)]
    pub links: Vec<String>,
    /// Extracted image destinations in document order.
    #[serde(default)]
    pub images: Vec<String>,
    /// Extracted code blocks in document order.
    #[serde(default)]
    pub code_blocks: Vec<CodeBlock>,
    /// Plain-text content for search indexing: prose, headings, and inline
    /// code in document order, whitespace-normalized.
    ///
    /// Fenced code literals and generated markup are excluded (see
    /// `signal-markdown`); this is never derived from rendered HTML.
    #[serde(default)]
    pub plain_text: String,
    /// Whitespace-separated word count of the Markdown source.
    #[serde(default)]
    pub word_count: usize,
}

impl RenderedBody {
    /// Empty body (useful for tests and placeholder entries).
    pub fn empty() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(level: u8, text: &str, id: &str) -> Heading {
        Heading {
            level,
            text: text.to_string(),
            id: id.to_string(),
        }
    }

    #[test]
    fn toc_nests_h3_under_h2_and_skips_h1() {
        let toc = Toc::build(&[
            heading(1, "Title", "title"),
            heading(2, "Alpha", "alpha"),
            heading(3, "Detail", "detail"),
            heading(2, "Beta", "beta"),
        ]);
        assert_eq!(toc.items.len(), 2);
        assert_eq!(toc.items[0].text, "Alpha");
        assert_eq!(toc.items[0].children.len(), 1);
        assert_eq!(toc.items[0].children[0].text, "Detail");
        assert_eq!(toc.items[1].text, "Beta");
        assert!(toc.items[1].children.is_empty());
    }

    #[test]
    fn toc_nests_skipped_levels_by_order() {
        let toc = Toc::build(&[heading(2, "A", "a"), heading(4, "Deep", "deep")]);
        assert_eq!(toc.items.len(), 1);
        assert_eq!(toc.items[0].children.len(), 1);
        assert_eq!(toc.items[0].children[0].level, 4);
    }

    #[test]
    fn toc_closes_subtree_on_equal_level() {
        let toc = Toc::build(&[
            heading(2, "A", "a"),
            heading(3, "A.1", "a-1"),
            heading(2, "B", "b"),
        ]);
        assert_eq!(toc.items.len(), 2);
        assert_eq!(toc.items[0].children.len(), 1);
        assert!(toc.items[1].children.is_empty());
    }

    #[test]
    fn toc_is_empty_without_listable_headings() {
        assert!(Toc::build(&[]).is_empty());
        assert!(Toc::build(&[heading(1, "Only", "only")]).is_empty());
    }

    #[test]
    fn toc_preserves_ids_for_fragment_links() {
        let toc = Toc::build(&[heading(2, "Install", "install-1")]);
        assert_eq!(toc.items[0].id, "install-1");
    }
}
