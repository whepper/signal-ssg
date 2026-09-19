//! Static search-index projection.
//!
//! The index is derived purely from the normalized model — titles,
//! descriptions, tags, dates, routes, and the pre-extracted
//! [`RenderedBody::plain_text`](signal_core::RenderedBody) — never from
//! rendered HTML. Serialization is `serde_json` over an explicit versioned
//! schema; the only client contract is that JSON file.

use serde::{Deserialize, Serialize};
use signal_core::{encode_route_path, ArtifactKind, ArtifactSpec, SiteModel};

use crate::{GenerateError, Generator};

/// Search index schema version. Bump deliberately on breaking changes.
pub const SEARCH_SCHEMA_VERSION: u32 = 1;

/// One searchable document: the smallest useful public projection of an
/// entry. Deliberately not `ContentEntry` — internal ids, bodies, and
/// presentation state stay out of the public artifact.
///
/// `id` and `url` carry the URL-path form of the route
/// ([`encode_route_path`]): clients resolve `url` against the site root, so
/// URL-significant route characters must already be encoded. The logical
/// route stays untouched on [`ContentEntry`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDocument {
    /// Stable document key: the entry's public route (e.g. `/posts/alpha/`).
    pub id: String,
    /// Entry title.
    pub title: String,
    /// Public route, relative (clients resolve against the site root).
    pub url: String,
    /// Author summary, omitted when the entry has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Plain-text content: prose, headings, and inline code in document
    /// order. Fenced code and Mermaid sources are excluded by extraction
    /// rule; alert bodies are included as normal prose.
    pub content: String,
    /// Taxonomy terms, sorted.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Owning collection (enables client-side section filtering).
    pub collection: String,
    /// Publication date (`YYYY-MM-DD`), omitted when undated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// Versioned search index: the exact JSON written to `index.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchIndex {
    /// Schema version ([`SEARCH_SCHEMA_VERSION`]).
    pub version: u32,
    /// Documents in deterministic route order.
    pub documents: Vec<SearchDocument>,
}

/// Project searchable documents from the model.
///
/// Eligibility mirrors the reference behavior (regular pages only):
/// section roots are excluded (their content lives on listing pages, which
/// are not themselves indexed), drafts never reach the model, and feeds,
/// taxonomy pages, and listings are not entries at all. Documents sort by
/// route — stable across builds, independent of dates and discovery order.
pub fn search_documents(model: &SiteModel) -> Vec<SearchDocument> {
    let mut entries: Vec<_> = model.entries().filter(|e| !e.section_root).collect();
    entries.sort_by(|a, b| a.route.0.cmp(&b.route.0));
    entries
        .into_iter()
        .map(|entry| SearchDocument {
            id: encode_route_path(&entry.route),
            title: entry.title.clone(),
            url: encode_route_path(&entry.route),
            description: entry.description.clone(),
            content: entry.body.plain_text.clone(),
            tags: entry.tags.iter().cloned().collect(),
            collection: entry.collection.0.clone(),
            date: entry.date.clone(),
        })
        .collect()
}

/// Serialize the full index. Pretty-printed: deterministic and reviewable
/// in goldens; clients parse it either way.
pub fn search_index_json(model: &SiteModel) -> String {
    let index = SearchIndex {
        version: SEARCH_SCHEMA_VERSION,
        documents: search_documents(model),
    };
    serde_json::to_string_pretty(&index).expect("owned strings always serialize")
}

/// Projection: the static search index (`index.json`).
pub struct Search;

impl Search {
    /// Create the search projection.
    pub fn new() -> Self {
        Self
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for Search {
    fn name(&self) -> &str {
        "search"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new(
            "index.json",
            ArtifactKind::SearchIndex,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{CollectionId, ContentEntry, ContentId, Route, Slug, SourceRef};

    fn entry(id: u32, collection: &str, slug: &str, title: &str) -> ContentEntry {
        ContentEntry::new(
            ContentId(id),
            CollectionId::new(collection),
            SourceRef::new(CollectionId::new(collection), format!("{slug}.md")),
            Slug::new(slug),
            Route::new(format!("/{collection}/{slug}/")),
            title,
        )
    }

    fn model() -> SiteModel {
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut section = entry(1, "posts", "", "Posts");
        section.route = Route::new("/posts/".to_string());
        section.section_root = true;
        section.body.plain_text = "Section body must not be indexed.".to_string();
        builder.add_entry(section);
        let mut alpha = entry(2, "posts", "alpha", "Alpha");
        alpha.description = Some("About A.".to_string());
        alpha.date = Some("2026-02-01".to_string());
        alpha.tags.insert("Rust".to_string());
        alpha.body.plain_text = "Alpha body Über text.".to_string();
        builder.add_entry(alpha);
        let mut beta = entry(3, "notes", "beta", "Beta");
        beta.body.plain_text = "Beta body.".to_string();
        builder.add_entry(beta);
        builder.build().expect("builds")
    }

    #[test]
    fn documents_select_regular_entries_in_route_order() {
        let docs = search_documents(&model());
        let ids: Vec<&str> = docs.iter().map(|d| d.id.as_str()).collect();
        // Section root excluded; notes sort before posts by route.
        assert_eq!(ids, vec!["/notes/beta/", "/posts/alpha/"]);
    }

    #[test]
    fn documents_carry_metadata_and_omit_missing_optionals() {
        let docs = search_documents(&model());
        let alpha = docs
            .iter()
            .find(|d| d.id == "/posts/alpha/")
            .expect("alpha");
        assert_eq!(alpha.title, "Alpha");
        assert_eq!(alpha.url, "/posts/alpha/");
        assert_eq!(alpha.description.as_deref(), Some("About A."));
        assert_eq!(alpha.content, "Alpha body Über text.");
        assert_eq!(alpha.tags, vec!["Rust".to_string()]);
        assert_eq!(alpha.collection, "posts");
        assert_eq!(alpha.date.as_deref(), Some("2026-02-01"));
        let beta = docs.iter().find(|d| d.id == "/notes/beta/").expect("beta");
        assert!(beta.description.is_none());
        assert!(beta.date.is_none());
    }

    #[test]
    fn serialized_index_is_versioned_and_valid() {
        let json = search_index_json(&model());
        let parsed: SearchIndex = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed.version, SEARCH_SCHEMA_VERSION);
        assert_eq!(parsed.documents.len(), 2);
        // Deterministic: same model, same bytes.
        assert_eq!(json, search_index_json(&model()));
    }

    #[test]
    fn empty_model_yields_valid_empty_index() {
        let empty = signal_core::SiteModelBuilder::new().build().expect("empty");
        let parsed: SearchIndex =
            serde_json::from_str(&search_index_json(&empty)).expect("valid JSON");
        assert_eq!(parsed.version, SEARCH_SCHEMA_VERSION);
        assert!(parsed.documents.is_empty());
    }

    #[test]
    fn generator_plans_index_artifact() {
        let specs = Search::new().generate(&model()).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "index.json");
        assert_eq!(specs[0].kind, ArtifactKind::SearchIndex);
    }

    #[test]
    fn projection_needs_no_html_or_filesystem() {
        // In-memory model only: no build output, no files, no network.
        // Serializing twice from the same model proves output is a pure
        // function of normalized data.
        let first = search_index_json(&model());
        let second = search_index_json(&model());
        assert_eq!(first, second);
        assert!(!first.contains("code-block"), "no HTML leaks: {first}");
        assert!(!first.contains("<aside"), "no HTML leaks: {first}");
    }
}
