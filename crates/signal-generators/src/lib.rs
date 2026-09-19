//! `signal-generators`: generator abstractions and list projections.
//!
//! Generators are pure projections over `&SiteModel`:
//!
//! - They receive a read-only [`signal_core::SiteModel`].
//! - They never mutate it.
//! - They emit lightweight [`signal_core::ArtifactSpec`] values (path + kind).
//! - They never consume another generator's rendered output.
//!
//! [`EntryPages`] plans one page per regular entry (section roots excluded).
//! [`SectionIndex`] plans one listing per collection at its route prefix.
//! [`Home`] plans the home page from one collection's featured + recent
//! entries. [`TopicsIndex`] plans the taxonomy index and [`TopicTerms`]
//! plans one page per topic term. [`json_ld`] builds structured metadata.
//! Rendering happens downstream in `signal-cli`, one artifact at a time,
//! from explicit contexts built with [`EntrySummary`] and [`TopicSummary`].

#![forbid(unsafe_code)]

pub mod json_ld;
pub mod robots;
pub mod rss;
pub mod search;
pub mod sitemap;

pub use robots::Robots;
pub use rss::{MainFeed, SectionFeeds, TaxonomyFeeds};
pub use search::Search;
pub use sitemap::Sitemap;

use serde::{Deserialize, Serialize};
use signal_core::{
    compare_by_date_desc, slugify, ArtifactKind, ArtifactSpec, CollectionId, ContentEntry,
    ContentId, SiteModel,
};
use std::collections::BTreeMap;
use thiserror::Error;

/// Generator failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GenerateError {
    /// Generator-specific validation failure.
    #[error("generate failed in {generator}: {message}")]
    Failed {
        /// Generator name.
        generator: String,
        /// Owned message.
        message: String,
    },
}

/// Pure projection over a read-only [`SiteModel`].
///
/// Synchronous by design. No filesystem, no rendering inside generators:
/// they only plan artifacts.
pub trait Generator {
    /// Stable generator name for diagnostics and manifests.
    fn name(&self) -> &str;

    /// Plan artifacts for the whole frozen model.
    fn generate(&self, model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError>;
}

/// Reading time in minutes for a word count: ceiling of words/200, minimum 1.
///
/// Shared by entry and listing contexts so bylines agree everywhere. Kept
/// deliberately identical to slice 1 (Hugo counts rendered words and may
/// differ by a minute; see `docs/migration/hugo.md`).
pub fn reading_time_minutes(word_count: usize) -> usize {
    (word_count.saturating_add(199) / 200).max(1)
}

/// Minimal per-entry projection for listing contexts (section pages, home).
///
/// Only the data templates actually need: no bodies, no internals, never the
/// model itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    /// Entry title.
    pub title: String,
    /// Short summary, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Publication date (`YYYY-MM-DD`), if given. Machine-readable form for
    /// `<time datetime>` attributes; human display uses `date_formatted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Presentation date per the site's configured format, if dated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_formatted: Option<String>,
    /// Output route, e.g. `/articles/foo/`.
    ///
    /// URL-path form ([`signal_core::encode_route_path`]): templates render
    /// this directly into `href` attributes, so URL-significant route
    /// characters arrive already encoded. The logical route stays untouched
    /// on [`ContentEntry`] and in manifest keys/digests.
    pub route: String,
    /// Taxonomy terms in authored front-matter order (first-occurrence
    /// deduplicated): display order for eyebrows, topic lists, and
    /// "first topic" picks. Canonical sorted order stays the contract for
    /// taxonomy indexing and term grouping.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Hero image, URL-path form (see [`signal_core::image_src_url`]), if
    /// given. Summaries feed heroes and cards; the full entry context
    /// carries the same value under `image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Hero image alternative text, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_alt: Option<String>,
    /// Reading time in minutes.
    pub reading_time: usize,
}

impl EntrySummary {
    /// Project one entry. Section roots have no summary (they are the page,
    /// not a member of the listing); returns `None` for them.
    /// `date_format` is the site's presentation format (see
    /// `signal_core::format_date`); the raw `date` is always retained.
    pub fn of(entry: &ContentEntry, date_format: &str) -> Option<Self> {
        if entry.section_root {
            return None;
        }
        Some(Self {
            title: entry.title.clone(),
            description: entry.description.clone(),
            date: entry.date.clone(),
            date_formatted: entry
                .date
                .as_deref()
                .and_then(|d| signal_core::format_date(d, date_format)),
            route: signal_core::encode_route_path(&entry.route),
            tags: entry.tag_order.clone(),
            image: entry.image.as_deref().map(signal_core::image_src_url),
            image_alt: entry.image_alt.clone(),
            reading_time: reading_time_minutes(entry.body.word_count),
        })
    }
}

/// Listing members of a collection: non-section-root entries newest-first.
///
/// Uses [`SiteModel::entries_in_collection_by_date`] (dated newest first,
/// undated last, slug then id tie-breaks) and drops section roots, which
/// belong to the section page itself, not its listing.
pub fn collection_summaries(
    model: &SiteModel,
    collection: &CollectionId,
    date_format: &str,
) -> Vec<EntrySummary> {
    model
        .entries_in_collection_by_date(collection)
        .iter()
        .filter_map(|entry| EntrySummary::of(entry, date_format))
        .collect()
}

/// Derives an output path deterministically from a route
/// (`/posts/hello/` -> `posts/hello/index.html`, `/` -> `index.html`).
pub fn route_to_output_path(route: &str) -> String {
    let trimmed = route.trim_matches('/');
    if trimmed.is_empty() {
        return "index.html".to_string();
    }
    format!("{trimmed}/index.html")
}

/// Projection: one [`ArtifactKind::Page`] spec per regular entry.
///
/// Section-root entries (`_index.md`) are skipped here; the section generator
/// renders them. Rendering happens later; this only plans.
pub struct EntryPages;

impl EntryPages {
    /// Create the projection.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EntryPages {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for EntryPages {
    fn name(&self) -> &str {
        "entry-pages"
    }

    fn generate(&self, model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        let mut specs: Vec<ArtifactSpec> = model
            .entries()
            .filter(|entry| !entry.section_root)
            .map(|entry| {
                ArtifactSpec::new(route_to_output_path(&entry.route.0), ArtifactKind::Page)
                    .with_route(entry.route.clone())
            })
            .collect();
        // Deterministic output order by path.
        specs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(specs)
    }
}

/// Projection: one [`ArtifactKind::CollectionIndex`] spec for a collection.
///
/// The spec route is the collection's route prefix (e.g. `/articles/`); the
/// section title/body resolve downstream from the section-root entry when
/// present, else collection configuration.
pub struct SectionIndex {
    collection: CollectionId,
    route_prefix: String,
}

impl SectionIndex {
    /// Create a section projection for one collection at its route prefix
    /// (e.g. `/articles/`, leading and trailing slashes normalized).
    pub fn new(collection: CollectionId, route_prefix: impl Into<String>) -> Self {
        let raw = route_prefix.into();
        let trimmed = raw.trim_matches('/').to_string();
        let route_prefix = if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{trimmed}/")
        };
        Self {
            collection,
            route_prefix,
        }
    }

    /// Collection being listed.
    pub fn collection(&self) -> &CollectionId {
        &self.collection
    }
}

impl Generator for SectionIndex {
    fn name(&self) -> &str {
        "section-index"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new(
            route_to_output_path(&self.route_prefix),
            ArtifactKind::CollectionIndex,
        )
        .with_route(signal_core::Route::new(
            self.route_prefix.clone(),
        ))])
    }
}

/// Projection: the home page from one collection.
///
/// Plans a single [`ArtifactKind::Home`] spec at `/`. The hero ("featured")
/// entry is the newest-dated featured entry; the recent list is the
/// collection listing capped at `recent_limit` (the migrated Hugo home shows
/// the first featured article plus up to 8 latest articles).
pub struct Home {
    collection: CollectionId,
    recent_limit: usize,
}

impl Home {
    /// Create a home projection over one collection with a recent-list cap.
    pub fn new(collection: CollectionId, recent_limit: usize) -> Self {
        Self {
            collection,
            recent_limit,
        }
    }

    /// Collection the home page lists.
    pub fn collection(&self) -> &CollectionId {
        &self.collection
    }

    /// Newest-dated featured entry, if any.
    pub fn featured(&self, model: &SiteModel, date_format: &str) -> Option<EntrySummary> {
        model
            .entries_in_collection_by_date(&self.collection)
            .iter()
            .filter(|entry| entry.featured && !entry.section_root)
            .filter_map(|entry| EntrySummary::of(entry, date_format))
            .next()
    }

    /// Recent listing members, capped at `recent_limit`.
    pub fn recent(&self, model: &SiteModel, date_format: &str) -> Vec<EntrySummary> {
        collection_summaries(model, &self.collection, date_format)
            .into_iter()
            .take(self.recent_limit)
            .collect()
    }
}

impl Generator for Home {
    fn name(&self) -> &str {
        "home"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new("index.html", ArtifactKind::Home)
            .with_route(signal_core::Route::new("/"))])
    }
}

/// Normalize a taxonomy root prefix with leading and trailing slashes
/// (`topics` -> `/topics/`).
pub(crate) fn normalize_root(root: &str) -> String {
    let trimmed = root.trim_matches('/').to_string();
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}/")
    }
}

/// One topic term: display label, URL slug, route, and member summaries.
///
/// The display label keeps its original case (`Beta Tools`); the slug is
/// the folded URL form (`beta-tools`). They are deliberately separate
/// concepts: only the slug enters routes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicSummary {
    /// Display label, original case.
    pub label: String,
    /// URL slug ([`slugify`]).
    pub slug: String,
    /// Term route, e.g. `/topics/beta-tools/`.
    pub route: String,
    /// Number of member entries.
    pub count: usize,
    /// Member summaries, newest-first.
    pub entries: Vec<EntrySummary>,
}

/// Listing members carrying one tag: non-section-root entries newest-first.
pub fn tagged_summaries(model: &SiteModel, tag: &str, date_format: &str) -> Vec<EntrySummary> {
    model
        .entries_tagged_by_date(tag)
        .iter()
        .filter_map(|entry| EntrySummary::of(entry, date_format))
        .collect()
}

/// Default maximum of related entries surfaced for one entry. Matches the
/// compatibility target's `first 3` related-articles block; overridable via
/// `[related] limit`.
pub const DEFAULT_RELATED_LIMIT: usize = 3;

/// Related entries for one entry: strongest shared-tag overlaps.
///
/// Candidates are non-section-root entries site-wide sharing at least one
/// tag with `entry` (the entry itself never appears). Ranking is by
/// shared-tag count descending; ties resolve in the standard listing order
/// (newest date first, then slug, then `ContentId` — the shared
/// [`compare_by_date_desc`] rule). Fully deterministic; capped at `limit`
/// (zero yields an empty list). Entries are projected through
/// [`EntrySummary::of`], so section roots can never appear twice over.
pub fn related_entries(
    model: &SiteModel,
    entry: &ContentEntry,
    limit: usize,
    date_format: &str,
) -> Vec<EntrySummary> {
    if limit == 0 || entry.tags.is_empty() {
        return Vec::new();
    }
    // Shared-tag counts over the per-tag index. BTreeMap keeps iteration
    // deterministic; a candidate appearing under several of the entry's
    // tags accumulates its score.
    let mut scores: BTreeMap<ContentId, usize> = BTreeMap::new();
    for tag in &entry.tags {
        for candidate in model.entries_tagged(tag) {
            if candidate.id == entry.id || candidate.section_root {
                continue;
            }
            *scores.entry(candidate.id).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(usize, &ContentEntry)> = scores
        .into_iter()
        .filter_map(|(id, score)| model.get(id).map(|candidate| (score, candidate)))
        .collect();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            // `compare_by_date_desc` takes `&&ContentEntry`; the pairs carry
            // `&ContentEntry`, so borrow one more level for the shared rule.
            compare_by_date_desc(&a.1, &b.1)
        })
    });
    ranked.truncate(limit);
    ranked
        .into_iter()
        .filter_map(|(_, candidate)| EntrySummary::of(candidate, date_format))
        .collect()
}

/// All topic terms with members, ordered case-insensitively by display
/// label (matches Hugo's alphabetical terms index).
///
/// Returns a [`GenerateError`] when two labels fold to the same slug: the
/// collision is a content error, never silently resolved.
pub fn topic_terms(
    model: &SiteModel,
    root: &str,
    date_format: &str,
) -> Result<Vec<TopicSummary>, GenerateError> {
    let root = normalize_root(root);
    let mut labels = model.all_tags();
    labels.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut seen_slugs = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(labels.len());
    for label in labels {
        let slug = slugify(&label);
        if slug.is_empty() {
            return Err(GenerateError::Failed {
                generator: "topics".to_string(),
                message: format!("topic {label:?} has an empty URL slug"),
            });
        }
        if !seen_slugs.insert(slug.clone()) {
            return Err(GenerateError::Failed {
                generator: "topics".to_string(),
                message: format!("topic slug collision on {slug:?}"),
            });
        }
        let entries = tagged_summaries(model, &label, date_format);
        out.push(TopicSummary {
            route: format!("{root}{slug}/"),
            count: entries.len(),
            label,
            slug,
            entries,
        });
    }
    Ok(out)
}

/// Projection: one [`ArtifactKind::Taxonomy`] spec for the taxonomy index
/// (e.g. `/topics/`).
pub struct TopicsIndex {
    root: String,
}

impl TopicsIndex {
    /// Create a taxonomy index projection at `root` (slashes normalized).
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            root: normalize_root(&root),
        }
    }

    /// Taxonomy root route, e.g. `/topics/`.
    pub fn root(&self) -> &str {
        &self.root
    }
}

impl Generator for TopicsIndex {
    fn name(&self) -> &str {
        "topics-index"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new(
            route_to_output_path(&self.root),
            ArtifactKind::Taxonomy,
        )
        .with_route(signal_core::Route::new(self.root.clone()))])
    }
}

/// Projection: one [`ArtifactKind::Taxonomy`] spec per topic term
/// (e.g. `/topics/beta-tools/`).
pub struct TopicTerms {
    root: String,
}

impl TopicTerms {
    /// Create a term-page projection under `root` (slashes normalized).
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            root: normalize_root(&root),
        }
    }

    /// Taxonomy root route, e.g. `/topics/`.
    pub fn root(&self) -> &str {
        &self.root
    }
}

impl Generator for TopicTerms {
    fn name(&self) -> &str {
        "topic-terms"
    }

    fn generate(&self, model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        // Only routes feed the specs; formatted dates never leave this call,
        // so the default format suffices for enumeration.
        let mut specs = Vec::new();
        for term in topic_terms(model, &self.root, signal_core::DEFAULT_DATE_FORMAT)? {
            specs.push(
                ArtifactSpec::new(route_to_output_path(&term.route), ArtifactKind::Taxonomy)
                    .with_route(signal_core::Route::new(term.route)),
            );
        }
        specs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(specs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{ContentId, Route, SiteModelBuilder, Slug, SourceRef};

    fn entry_with(
        id: u32,
        slug: &str,
        route: &str,
        date: Option<&str>,
        featured: bool,
        section_root: bool,
    ) -> ContentEntry {
        let mut e = ContentEntry::new(
            ContentId(id),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), format!("{slug}.md")),
            Slug::new(slug),
            Route::new(route),
            format!("Title {slug}"),
        );
        e.date = date.map(str::to_string);
        e.featured = featured;
        e.section_root = section_root;
        e
    }

    fn model_two_entries() -> SiteModel {
        let mut b = SiteModelBuilder::new();
        b.add_entry(ContentEntry::new(
            ContentId(1),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "hello-world.md"),
            Slug::new("hello-world"),
            Route::new("/posts/hello-world/"),
            "Hello",
        ));
        b.add_entry(ContentEntry::new(
            ContentId(2),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "second.md"),
            Slug::new("second"),
            Route::new("/posts/second/"),
            "Second",
        ));
        b.build().expect("builds")
    }

    #[test]
    fn generators_receive_read_only_model_and_emit_specs() {
        let model = model_two_entries();
        let before = model.clone();
        let specs = EntryPages::new().generate(&model).expect("generates");
        // Read-only by API (`&SiteModel`); runtime check that nothing mutated.
        assert_eq!(model, before);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].path, "posts/hello-world/index.html");
        assert_eq!(specs[0].kind, ArtifactKind::Page);
    }

    #[test]
    fn output_order_is_deterministic() {
        let model = model_two_entries();
        let a = EntryPages::new().generate(&model).unwrap();
        let b = EntryPages::new().generate(&model).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn entry_pages_skip_section_roots() {
        let mut b = SiteModelBuilder::new();
        let mut index = entry_with(1, "", "/posts/", Some("2026-01-01"), false, true);
        index.title = "Posts".to_string();
        b.add_entry(index);
        b.add_entry(entry_with(
            2,
            "a",
            "/posts/a/",
            Some("2026-02-01"),
            false,
            false,
        ));
        let model = b.build().expect("builds");
        let specs = EntryPages::new().generate(&model).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "posts/a/index.html");
    }

    #[test]
    fn section_index_plans_prefix_route() {
        let model = model_two_entries();
        let gen = SectionIndex::new(CollectionId::new("posts"), "/posts/");
        let specs = gen.generate(&model).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, ArtifactKind::CollectionIndex);
        assert_eq!(specs[0].path, "posts/index.html");
        assert_eq!(specs[0].route, Some(Route::new("/posts/")));
    }

    #[test]
    fn summaries_are_newest_first_without_section_roots() {
        let mut b = SiteModelBuilder::new();
        let mut index = entry_with(1, "", "/posts/", None, false, true);
        index.title = "Posts".to_string();
        b.add_entry(index);
        b.add_entry(entry_with(
            2,
            "old",
            "/posts/old/",
            Some("2026-01-01"),
            false,
            false,
        ));
        b.add_entry(entry_with(
            3,
            "new",
            "/posts/new/",
            Some("2026-09-03"),
            false,
            false,
        ));
        b.add_entry(entry_with(
            4,
            "undated",
            "/posts/undated/",
            None,
            false,
            false,
        ));
        let model = b.build().expect("builds");
        let collection = CollectionId::new("posts");
        let titles: Vec<String> = collection_summaries(&model, &collection, "%Y-%m-%d")
            .iter()
            .map(|s| s.title.clone())
            .collect();
        assert_eq!(titles, vec!["Title new", "Title old", "Title undated"]);
    }

    #[test]
    fn home_plans_root_and_selects_featured_plus_recent() {
        let mut b = SiteModelBuilder::new();
        b.add_entry(entry_with(
            1,
            "old",
            "/posts/old/",
            Some("2026-01-01"),
            true,
            false,
        ));
        b.add_entry(entry_with(
            2,
            "new",
            "/posts/new/",
            Some("2026-09-03"),
            true,
            false,
        ));
        b.add_entry(entry_with(
            3,
            "plain",
            "/posts/plain/",
            Some("2026-06-01"),
            false,
            false,
        ));
        let model = b.build().expect("builds");
        let home = Home::new(CollectionId::new("posts"), 2);
        let specs = home.generate(&model).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, ArtifactKind::Home);
        assert_eq!(specs[0].path, "index.html");
        // Newest featured entry wins the hero.
        assert_eq!(
            home.featured(&model, "%Y-%m-%d").map(|s| s.title),
            Some("Title new".to_string())
        );
        // Recent list is capped and date-ordered.
        let recent: Vec<String> = home
            .recent(&model, "%Y-%m-%d")
            .iter()
            .map(|s| s.title.clone())
            .collect();
        assert_eq!(
            recent,
            vec!["Title new".to_string(), "Title plain".to_string()]
        );
    }

    #[test]
    fn home_without_featured_has_no_hero() {
        let model = model_two_entries();
        let home = Home::new(CollectionId::new("posts"), 8);
        assert!(home.featured(&model, "%Y-%m-%d").is_none());
        assert_eq!(home.recent(&model, "%Y-%m-%d").len(), 2);
    }

    #[test]
    fn summaries_carry_both_raw_and_formatted_dates() {
        let model = taxonomy_model();
        let collection = CollectionId::new("articles");
        let summaries = collection_summaries(&model, &collection, "%-d %B %Y");
        let old = summaries
            .iter()
            .find(|s| s.title == "Title old")
            .expect("old listed");
        assert_eq!(old.date.as_deref(), Some("2026-01-01"));
        assert_eq!(old.date_formatted.as_deref(), Some("1 January 2026"));
    }

    #[test]
    fn summaries_carry_hero_image_in_url_path_form() {
        let mut b = SiteModelBuilder::new();
        let mut e = ContentEntry::new(
            ContentId(1),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "a.md"),
            Slug::new("a"),
            Route::new("/posts/a/"),
            "A",
        );
        e.image = Some("/images/a b.png".to_string());
        e.image_alt = Some("Alt".to_string());
        b.add_entry(e);
        b.add_entry(ContentEntry::new(
            ContentId(2),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "b.md"),
            Slug::new("b"),
            Route::new("/posts/b/"),
            "B",
        ));
        let model = b.build().expect("builds");
        let summaries = collection_summaries(&model, &CollectionId::new("posts"), "%Y-%m-%d");
        let a = summaries.iter().find(|s| s.title == "A").expect("listed");
        // Same serialization rule as the full entry context: paths encode,
        // alt text passes through, absence omits the keys.
        assert_eq!(a.image.as_deref(), Some("/images/a%20b.png"));
        assert_eq!(a.image_alt.as_deref(), Some("Alt"));
        let b = summaries.iter().find(|s| s.title == "B").expect("listed");
        assert_eq!(b.image, None);
        assert_eq!(b.image_alt, None);
    }

    #[test]
    fn summaries_carry_authored_tag_order() {
        let mut b = SiteModelBuilder::new();
        let mut e = ContentEntry::new(
            ContentId(1),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "a.md"),
            Slug::new("a"),
            Route::new("/posts/a/"),
            "A",
        );
        e.tags = ["Alpha", "Mike", "Zulu"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        e.tag_order = vec!["Zulu".to_string(), "Alpha".to_string(), "Mike".to_string()];
        b.add_entry(e);
        let model = b.build().expect("builds");
        let summaries = collection_summaries(&model, &CollectionId::new("posts"), "%Y-%m-%d");
        assert_eq!(summaries.len(), 1);
        // Template-visible order follows authorship, not the canonical set.
        assert_eq!(
            summaries[0].tags,
            vec!["Zulu".to_string(), "Alpha".to_string(), "Mike".to_string()]
        );
        // Query semantics still use the canonical sorted set: every term
        // resolves, in canonical term order.
        assert_eq!(
            model.all_tags(),
            vec!["Alpha".to_string(), "Mike".to_string(), "Zulu".to_string()]
        );
    }

    #[test]
    fn empty_collection_summarizes_to_empty() {
        let model = model_two_entries();
        assert!(collection_summaries(&model, &CollectionId::new("missing"), "%Y-%m-%d").is_empty());
    }

    fn tagged_entry(
        id: u32,
        collection: &str,
        slug: &str,
        date: Option<&str>,
        tags: &[&str],
    ) -> ContentEntry {
        let mut e = ContentEntry::new(
            ContentId(id),
            CollectionId::new(collection),
            SourceRef::new(CollectionId::new(collection), format!("{slug}.md")),
            Slug::new(slug),
            Route::new(format!("/{collection}/{slug}/")),
            format!("Title {slug}"),
        );
        e.date = date.map(str::to_string);
        e.tags = tags.iter().map(|t| t.to_string()).collect();
        // Test entries mirror ingestion: the display list carries the same
        // terms in the given order.
        e.tag_order = tags.iter().map(|t| t.to_string()).collect();
        e
    }

    fn taxonomy_model() -> SiteModel {
        let mut b = SiteModelBuilder::new();
        b.add_entry(tagged_entry(
            1,
            "articles",
            "old",
            Some("2026-01-01"),
            &["Beta Tools", "alpha Guides"],
        ));
        b.add_entry(tagged_entry(
            2,
            "projects",
            "proj",
            Some("2026-09-02"),
            &["Beta Tools"],
        ));
        b.add_entry(tagged_entry(
            3,
            "articles",
            "plain",
            Some("2026-06-01"),
            &[],
        ));
        b.build().expect("builds")
    }

    #[test]
    fn topic_terms_span_collections_newest_first() {
        let model = taxonomy_model();
        let terms = topic_terms(&model, "/topics/", "%Y-%m-%d").expect("terms");
        let labels: Vec<&str> = terms.iter().map(|t| t.label.as_str()).collect();
        // Case-insensitive alphabetical, which differs from byte order here
        // ("Beta Tools" < "alpha Guides" byte-wise, reversed case-folded).
        assert_eq!(labels, vec!["alpha Guides", "Beta Tools"]);
        let beta = &terms[1];
        assert_eq!(beta.slug, "beta-tools");
        assert_eq!(beta.route, "/topics/beta-tools/");
        assert_eq!(beta.count, 2);
        let titles: Vec<&str> = beta.entries.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Title proj", "Title old"]);
    }

    #[test]
    fn topic_generators_plan_index_and_terms() {
        let model = taxonomy_model();
        let index = TopicsIndex::new("/topics/")
            .generate(&model)
            .expect("index");
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].kind, ArtifactKind::Taxonomy);
        assert_eq!(index[0].path, "topics/index.html");
        assert_eq!(index[0].route, Some(Route::new("/topics/")));

        let terms = TopicTerms::new("topics").generate(&model).expect("terms");
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].path, "topics/alpha-guides/index.html");
        assert_eq!(terms[1].path, "topics/beta-tools/index.html");
    }

    #[test]
    fn slug_collisions_are_an_error_not_silent() {
        let mut b = SiteModelBuilder::new();
        b.add_entry(tagged_entry(1, "articles", "a", None, &["Beta Tools"]));
        b.add_entry(tagged_entry(2, "articles", "b", None, &["beta tools"]));
        let model = b.build().expect("builds");
        let err = topic_terms(&model, "/topics/", "%Y-%m-%d").expect_err("collides");
        assert!(matches!(err, GenerateError::Failed { .. }));
    }

    #[test]
    fn duplicate_tags_within_an_entry_count_once() {
        // BTreeSet normalization means repeats collapse before indexing.
        let mut b = SiteModelBuilder::new();
        let mut e = tagged_entry(1, "articles", "a", None, &["Rust"]);
        e.tags.insert("Rust".to_string());
        b.add_entry(e);
        let model = b.build().expect("builds");
        let terms = topic_terms(&model, "/topics/", "%Y-%m-%d").expect("terms");
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].count, 1);
    }

    // --- related entries: shared-tag overlap projection ---

    fn related_model() -> SiteModel {
        let mut b = SiteModelBuilder::new();
        // `source` shares both tags with `target` (score 2).
        let mut source = tagged_entry(1, "posts", "source", Some("2026-01-01"), &["A", "B"]);
        source.route = Route::new("/posts/source/");
        // `single` shares one tag; newer date, but score sorts first.
        let mut single = tagged_entry(2, "posts", "single", Some("2026-09-09"), &["B"]);
        single.route = Route::new("/posts/single/");
        // Unrelated entry: never a candidate.
        let mut other = tagged_entry(3, "posts", "other", Some("2026-09-08"), &["Z"]);
        other.route = Route::new("/posts/other/");
        b.add_entry(source);
        b.add_entry(single);
        b.add_entry(other);
        let mut target = tagged_entry(4, "posts", "target", Some("2026-02-02"), &["A", "B"]);
        target.route = Route::new("/posts/target/");
        b.add_entry(target);
        b.build().expect("builds")
    }

    #[test]
    fn related_ranks_by_shared_tag_count() {
        let model = related_model();
        let target = model
            .lookup_by_route(&Route::new("/posts/target/"))
            .expect("t");
        let related = related_entries(&model, target, 3, "%Y-%m-%d");
        let titles: Vec<&str> = related.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Title source", "Title single"]);
    }

    #[test]
    fn related_excludes_self_unrelated_and_section_roots() {
        let mut b = SiteModelBuilder::new();
        let mut index = tagged_entry(1, "posts", "", None, &["A"]);
        index.route = Route::new("/posts/");
        index.section_root = true;
        b.add_entry(index);
        let mut solo = tagged_entry(2, "posts", "solo", None, &["A"]);
        solo.route = Route::new("/posts/solo/");
        b.add_entry(solo);
        let model = b.build().expect("builds");
        let solo_entry = model
            .lookup_by_route(&Route::new("/posts/solo/"))
            .expect("s");
        // The section root sharing the tag is excluded; `solo` itself never
        // appears; nothing else overlaps → empty.
        assert!(related_entries(&model, solo_entry, 3, "%Y-%m-%d").is_empty());
    }

    #[test]
    fn related_caps_at_the_limit_and_stays_deterministic() {
        let mut b = SiteModelBuilder::new();
        let mut target = tagged_entry(10, "posts", "target", Some("2026-01-01"), &["T"]);
        target.route = Route::new("/posts/target/");
        b.add_entry(target);
        // Four same-score candidates, distinct dates: after the (equal)
        // score, order is newest-first. Caps and orderings must agree.
        for (id, slug, date) in [
            (1u32, "a", "2026-01-02"),
            (2, "b", "2026-03-04"),
            (3, "c", "2026-02-03"),
            (4, "d", "2025-12-31"),
        ] {
            let mut e = tagged_entry(id, "posts", slug, Some(date), &["T"]);
            e.route = Route::new(format!("/posts/{slug}/"));
            b.add_entry(e);
        }
        let model = b.build().expect("builds");
        let target = model
            .lookup_by_route(&Route::new("/posts/target/"))
            .expect("t");
        let capped = related_entries(&model, target, 3, "%Y-%m-%d");
        let routes: Vec<&str> = capped.iter().map(|s| s.route.as_str()).collect();
        assert_eq!(routes, vec!["/posts/b/", "/posts/c/", "/posts/a/"]);
        // Same inputs, same output.
        assert_eq!(capped, related_entries(&model, target, 3, "%Y-%m-%d"));
        // A larger limit surfaces the remaining candidate too.
        let all = related_entries(&model, target, 10, "%Y-%m-%d");
        assert_eq!(all.len(), 4);
        assert_eq!(all[3].route, "/posts/d/");
        // Zero limit is an explicit empty result.
        assert!(related_entries(&model, target, 0, "%Y-%m-%d").is_empty());
    }

    #[test]
    fn related_of_untagged_entry_and_empty_model_is_empty() {
        let model = related_model();
        let untagged = ContentEntry::new(
            ContentId(99),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "u.md"),
            Slug::new("u"),
            Route::new("/posts/u/"),
            "Untagged",
        );
        // No tags → no candidates, without consulting the model at all.
        assert!(related_entries(&model, &untagged, 3, "%Y-%m-%d").is_empty());
        let empty = SiteModelBuilder::new().build().expect("builds");
        let scratch = related_model();
        let any = scratch
            .lookup_by_route(&Route::new("/posts/target/"))
            .expect("t");
        assert!(related_entries(&empty, any, 3, "%Y-%m-%d").is_empty());
    }
}
