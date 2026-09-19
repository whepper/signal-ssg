//! Immutable normalized site model.
//!
//! `SiteModel` is an immutable collection of ordinary indexed Rust data
//! structures (`BTreeMap` / `BTreeSet`). There is no generic graph engine:
//! graph-like questions are answered with purpose-built indexes and query
//! functions.
//!
//! Semantic references (`Post A references Post B`) are query substrate only.
//! They do not imply build dependencies; those are derived later from what
//! generators actually consume and recorded in a disposable build manifest.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::body::RenderedBody;
use crate::error::CoreError;
use crate::ids::{CollectionId, ContentId, Route, Slug, SourceRef};

/// One normalized content entry.
///
/// Owned data only. No Markdown ASTs, no template objects, no file handles.
/// `description`, `date`, and `body` are the vertical-slice subset of
/// front-matter/derived data; further fields arrive when a real migration
/// requirement demonstrates the need.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentEntry {
    /// In-memory handle.
    pub id: ContentId,
    /// Owning collection.
    pub collection: CollectionId,
    /// Stable source identity.
    pub source: SourceRef,
    /// Human-readable label (not identity, not route).
    pub slug: Slug,
    /// Output route (must be unique across the model).
    pub route: Route,
    /// Title for listings and rendering contexts.
    pub title: String,
    /// Short summary for lists and metadata (front-matter `description`).
    #[serde(default)]
    pub description: Option<String>,
    /// Publication date as a normalized `YYYY-MM-DD` string, if given.
    ///
    /// Machine-readable canonical form; presentation formatting happens at
    /// the render boundary ([`crate::format_date`]).
    #[serde(default)]
    pub date: Option<String>,
    /// Last-modification date, same normalized form as [`Self::date`].
    ///
    /// Promoted from front-matter `lastmod`: JSON-LD `dateModified` and
    /// "updated" displays need its semantics.
    #[serde(default)]
    pub last_modified: Option<String>,
    /// Entry author override (front-matter `author`).
    ///
    /// Falls back to the configured site author at render time; omitted
    /// everywhere when neither exists. Never fabricated.
    #[serde(default)]
    pub author: Option<String>,
    /// Hero image reference (front-matter `image`), site-root URL form.
    ///
    /// Identity only: how the bytes arrive (static passthrough, external
    /// URL) is orthogonal. No image processing happens in the model.
    #[serde(default)]
    pub image: Option<String>,
    /// Hero image alt text (front-matter `image_alt`).
    #[serde(default)]
    pub image_alt: Option<String>,
    /// Owned derived body (rendered HTML plus extracted structure).
    #[serde(default)]
    pub body: RenderedBody,
    /// Taxonomy tags (Hugo `topics` maps here; see ingestion).
    #[serde(default)]
    pub tags: BTreeSet<String>,
    /// Whether the entry is featured (front-matter `featured`).
    ///
    /// Promoted to typed data because the home projection genuinely needs
    /// "featured entry" semantics. Only meaningful alongside a home
    /// generator; ignored elsewhere.
    #[serde(default)]
    pub featured: bool,
    /// Whether the entry addresses its collection root (`_index.md`).
    ///
    /// Section roots are rendered by section generators, never by per-entry
    /// projections: they are the section page, not a member of its own
    /// listing.
    #[serde(default)]
    pub section_root: bool,
    /// Optional translation group key; entries sharing a key are translations.
    #[serde(default)]
    pub translation_group: Option<String>,
    /// Optional language tag, e.g. `en`.
    #[serde(default)]
    pub language: Option<String>,
    /// Semantic references to other entries (not build dependencies).
    #[serde(default)]
    pub references: Vec<ContentId>,
}

impl ContentEntry {
    /// Create a minimal entry; tags, translations, and references default empty.
    pub fn new(
        id: ContentId,
        collection: CollectionId,
        source: SourceRef,
        slug: Slug,
        route: Route,
        title: impl Into<String>,
    ) -> Self {
        Self {
            id,
            collection,
            source,
            slug,
            route,
            title: title.into(),
            description: None,
            date: None,
            last_modified: None,
            author: None,
            image: None,
            image_alt: None,
            body: RenderedBody::empty(),
            tags: BTreeSet::new(),
            featured: false,
            section_root: false,
            translation_group: None,
            language: None,
            references: Vec::new(),
        }
    }
}

/// Immutable normalized site model.
///
/// Construct via [`SiteModelBuilder`]; query via `&SiteModel`.
/// All iteration is deterministic (ordered maps/sets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteModel {
    entries: BTreeMap<ContentId, ContentEntry>,
    by_collection: BTreeMap<CollectionId, BTreeSet<ContentId>>,
    by_slug: BTreeMap<(CollectionId, Slug), ContentId>,
    by_tag: BTreeMap<String, BTreeSet<ContentId>>,
    by_route: BTreeMap<Route, ContentId>,
    referenced_by: BTreeMap<ContentId, BTreeSet<ContentId>>,
    by_translation_group: BTreeMap<String, BTreeSet<ContentId>>,
}

impl SiteModel {
    /// Look up an entry by id.
    pub fn get(&self, id: ContentId) -> Option<&ContentEntry> {
        self.entries.get(&id)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the model is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All entries in deterministic `ContentId` order.
    pub fn entries(&self) -> impl Iterator<Item = &ContentEntry> {
        self.entries.values()
    }

    /// Entries in a collection, in deterministic order.
    pub fn entries_in_collection(&self, collection: &CollectionId) -> Vec<&ContentEntry> {
        self.by_collection
            .get(collection)
            .map(|ids| ids.iter().filter_map(|id| self.entries.get(id)).collect())
            .unwrap_or_default()
    }

    /// Entries in a collection newest-first for listings.
    ///
    /// Ordering rule (mirrors the migrated Hugo `.ByDate.Reverse`):
    /// dated entries first, newest date first; undated entries last;
    /// ties broken by slug ascending, then `ContentId`. Dates are
    /// normalized `YYYY-MM-DD` strings, so lexicographic order is
    /// chronological order. Fully deterministic: no filesystem, hash-map,
    /// or discovery order leaks in.
    pub fn entries_in_collection_by_date(&self, collection: &CollectionId) -> Vec<&ContentEntry> {
        let mut entries = self.entries_in_collection(collection);
        entries.sort_by(compare_by_date_desc);
        entries
    }

    /// Entries carrying a tag, in deterministic order.
    pub fn entries_tagged(&self, tag: &str) -> Vec<&ContentEntry> {
        self.by_tag
            .get(tag)
            .map(|ids| ids.iter().filter_map(|id| self.entries.get(id)).collect())
            .unwrap_or_default()
    }

    /// Entries carrying a tag, newest-first (same ordering rule as
    /// [`Self::entries_in_collection_by_date`]).
    ///
    /// Term pages list members newest-first regardless of collection, which
    /// is what the migrated Hugo taxonomy pages render.
    pub fn entries_tagged_by_date(&self, tag: &str) -> Vec<&ContentEntry> {
        let mut entries = self.entries_tagged(tag);
        entries.sort_by(compare_by_date_desc);
        entries
    }

    /// All taxonomy terms (tag display labels) present in the model, in
    /// index (`BTreeMap`) order.
    ///
    /// This is explicitly *not* display order: term listings sort
    /// case-insensitively downstream to match Hugo's alphabetical terms.
    /// Terms exist only because entries carry them — empty terms are
    /// unrepresentable, matching Hugo (which derives terms from content).
    pub fn all_tags(&self) -> Vec<String> {
        self.by_tag.keys().cloned().collect()
    }

    /// Regular entries site-wide, newest-first (same ordering rule as
    /// [`Self::entries_in_collection_by_date`]).
    ///
    /// Section roots are excluded: they render as their section page, and
    /// the main feed lists consumable entries — mirroring how the reference
    /// feed carries articles and project pages but no section indexes.
    pub fn regular_entries_by_date(&self) -> Vec<&ContentEntry> {
        let mut entries: Vec<&ContentEntry> = self.entries().filter(|e| !e.section_root).collect();
        entries.sort_by(compare_by_date_desc);
        entries
    }

    /// Translations of an entry: other entries sharing its translation group.
    pub fn translations_of(&self, id: ContentId) -> Vec<&ContentEntry> {
        let group = self
            .entries
            .get(&id)
            .and_then(|e| e.translation_group.clone());
        match group {
            None => Vec::new(),
            Some(g) => self
                .by_translation_group
                .get(&g)
                .map(|ids| {
                    ids.iter()
                        .filter(|other| **other != id)
                        .filter_map(|other| self.entries.get(other))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Reverse semantic references: entries that reference `id`.
    pub fn referencing(&self, id: ContentId) -> Vec<&ContentEntry> {
        self.referenced_by
            .get(&id)
            .map(|ids| ids.iter().filter_map(|rid| self.entries.get(rid)).collect())
            .unwrap_or_default()
    }

    /// Resolve an output route to its entry.
    pub fn lookup_by_route(&self, route: &Route) -> Option<&ContentEntry> {
        self.by_route.get(route).and_then(|id| self.entries.get(id))
    }

    /// All routes in deterministic order.
    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.by_route.keys()
    }
}

/// Newest-first comparator shared by date-ordered listing queries.
///
/// Dated entries first (newest date first); undated entries last; ties
/// broken by slug ascending, then `ContentId`.
fn compare_by_date_desc(a: &&ContentEntry, b: &&ContentEntry) -> std::cmp::Ordering {
    match (&b.date, &a.date) {
        (Some(bd), Some(ad)) => bd
            .cmp(ad)
            .then_with(|| a.slug.cmp(&b.slug))
            .then_with(|| a.id.cmp(&b.id)),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.slug.cmp(&b.slug).then_with(|| a.id.cmp(&b.id)),
    }
}

/// Builder that validates and freezes a [`SiteModel`].
///
/// The builder is the only mutating surface. `build()` consumes it and
/// returns an immutable model, so post-freeze mutation is impossible by API.
#[derive(Clone, Debug, Default)]
pub struct SiteModelBuilder {
    pending: Vec<ContentEntry>,
}

impl SiteModelBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage one normalized entry. Validation happens in [`Self::build`].
    pub fn add_entry(&mut self, entry: ContentEntry) -> &mut Self {
        self.pending.push(entry);
        self
    }

    /// Validate (duplicate ids/sources/routes, dangling references) and freeze.
    pub fn build(self) -> Result<SiteModel, CoreError> {
        let mut entries: BTreeMap<ContentId, ContentEntry> = BTreeMap::new();
        let mut seen_source: BTreeMap<(CollectionId, String), ContentId> = BTreeMap::new();
        let mut by_route: BTreeMap<Route, ContentId> = BTreeMap::new();
        // (route, first_source_label, second_source_label) for collision reports.
        let mut route_claimants: BTreeMap<Route, (String, String)> = BTreeMap::new();

        for entry in self.pending {
            if entries.contains_key(&entry.id) {
                return Err(CoreError::DuplicateContentId(entry.id.0));
            }
            let source_key = (entry.collection.clone(), entry.source.relative_path.clone());
            if let Some(_prev) = seen_source.insert(source_key, entry.id) {
                return Err(CoreError::DuplicateSource {
                    collection: entry.collection.0.clone(),
                    relative_path: entry.source.relative_path.clone(),
                });
            }
            if let Some(prev_id) = by_route.get(&entry.route) {
                let prev_label = format!("{:?}", prev_id);
                let next_label = format!("{:?}", entry.id);
                route_claimants.insert(entry.route.clone(), (prev_label, next_label));
            } else {
                by_route.insert(entry.route.clone(), entry.id);
            }
            entries.insert(entry.id, entry);
        }

        if let Some((route, (first, second))) = route_claimants.into_iter().next() {
            return Err(CoreError::RouteCollision {
                route: route.0,
                first,
                second,
            });
        }

        // Dangling semantic references are a validation error at freeze time.
        for entry in entries.values() {
            for target in &entry.references {
                if !entries.contains_key(target) {
                    return Err(CoreError::UnknownReference {
                        from: entry.id.0,
                        to: target.0,
                    });
                }
            }
        }

        let mut by_collection: BTreeMap<CollectionId, BTreeSet<ContentId>> = BTreeMap::new();
        let mut by_slug: BTreeMap<(CollectionId, Slug), ContentId> = BTreeMap::new();
        let mut by_tag: BTreeMap<String, BTreeSet<ContentId>> = BTreeMap::new();
        let mut referenced_by: BTreeMap<ContentId, BTreeSet<ContentId>> = BTreeMap::new();
        let mut by_translation_group: BTreeMap<String, BTreeSet<ContentId>> = BTreeMap::new();

        for entry in entries.values() {
            by_collection
                .entry(entry.collection.clone())
                .or_default()
                .insert(entry.id);
            // First writer wins for slug lookup; slugs are not required unique.
            by_slug
                .entry((entry.collection.clone(), entry.slug.clone()))
                .or_insert(entry.id);
            for tag in &entry.tags {
                by_tag.entry(tag.clone()).or_default().insert(entry.id);
            }
            for target in &entry.references {
                referenced_by.entry(*target).or_default().insert(entry.id);
            }
            if let Some(group) = &entry.translation_group {
                by_translation_group
                    .entry(group.clone())
                    .or_default()
                    .insert(entry.id);
            }
        }

        Ok(SiteModel {
            entries,
            by_collection,
            by_slug,
            by_tag,
            by_route,
            referenced_by,
            by_translation_group,
        })
    }
}

// Compile-time assertion helper used by integration tests:
// `SiteModel` must be `Send + Sync` without interior mutability tricks.
#[allow(dead_code)]
fn assert_send_sync<T: Send + Sync>() {}

#[allow(dead_code)]
fn assert_model_is_send_sync() {
    assert_send_sync::<SiteModel>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, collection: &str, path: &str, slug: &str, route: &str) -> ContentEntry {
        ContentEntry::new(
            ContentId(id),
            CollectionId::new(collection),
            SourceRef::new(CollectionId::new(collection), path),
            Slug::new(slug),
            Route::new(route),
            format!("Title {id}"),
        )
    }

    fn two_entry_model() -> SiteModel {
        let mut b = SiteModelBuilder::new();
        b.add_entry(entry(1, "posts", "a.md", "a", "/a/"));
        b.add_entry(entry(2, "posts", "b.md", "b", "/b/"));
        b.build().expect("builds")
    }

    #[test]
    fn freezes_and_queries_deterministically() {
        let model = two_entry_model();
        assert_eq!(model.len(), 2);
        let ids: Vec<u32> = model.entries().map(|e| e.id.0).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(
            model
                .entries_in_collection(&CollectionId::new("posts"))
                .len(),
            2
        );
        assert!(model.lookup_by_route(&Route::new("/a/")).is_some());
    }

    #[test]
    fn route_collisions_are_detected() {
        let mut b = SiteModelBuilder::new();
        b.add_entry(entry(1, "posts", "a.md", "a", "/same/"));
        b.add_entry(entry(2, "posts", "b.md", "b", "/same/"));
        let err = b.build().expect_err("must collide");
        assert!(matches!(err, CoreError::RouteCollision { .. }));
    }

    #[test]
    fn semantic_references_are_queryable_but_not_build_deps() {
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "posts", "a.md", "a", "/a/");
        a.references.push(ContentId(2));
        b.add_entry(a);
        b.add_entry(entry(2, "posts", "b.md", "b", "/b/"));
        let model = b.build().expect("builds");
        assert_eq!(model.referencing(ContentId(2)).len(), 1);
        // No build-dependency surface exists on the model by construction.
    }

    #[test]
    fn dangling_references_fail_validation() {
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "posts", "a.md", "a", "/a/");
        a.references.push(ContentId(99));
        b.add_entry(a);
        assert!(matches!(b.build(), Err(CoreError::UnknownReference { .. })));
    }

    fn dated_entry(id: u32, slug: &str, date: Option<&str>) -> ContentEntry {
        let mut e = entry(
            id,
            "posts",
            &format!("{slug}.md"),
            slug,
            &format!("/{slug}/"),
        );
        e.date = date.map(str::to_string);
        e
    }

    #[test]
    fn date_ordering_is_newest_first_with_undated_last() {
        let mut b = SiteModelBuilder::new();
        b.add_entry(dated_entry(1, "old", Some("2026-01-01")));
        b.add_entry(dated_entry(2, "undated", None));
        b.add_entry(dated_entry(3, "new", Some("2026-09-03")));
        b.add_entry(dated_entry(4, "mid", Some("2026-09-02")));
        let model = b.build().expect("builds");
        let slugs: Vec<&str> = model
            .entries_in_collection_by_date(&CollectionId::new("posts"))
            .iter()
            .map(|e| e.slug.0.as_str())
            .collect();
        assert_eq!(slugs, vec!["new", "mid", "old", "undated"]);
    }

    #[test]
    fn date_ties_break_by_slug_then_id() {
        let mut b = SiteModelBuilder::new();
        b.add_entry(dated_entry(2, "b-slug", Some("2026-09-02")));
        b.add_entry(dated_entry(1, "a-slug", Some("2026-09-02")));
        let model = b.build().expect("builds");
        let slugs: Vec<&str> = model
            .entries_in_collection_by_date(&CollectionId::new("posts"))
            .iter()
            .map(|e| e.slug.0.as_str())
            .collect();
        assert_eq!(slugs, vec!["a-slug", "b-slug"]);
    }

    #[test]
    fn empty_collection_orders_to_empty() {
        let model = two_entry_model();
        assert!(model
            .entries_in_collection_by_date(&CollectionId::new("missing"))
            .is_empty());
    }

    #[test]
    fn tagged_query_orders_newest_first_across_collections() {
        let mut b = SiteModelBuilder::new();
        let mut old = entry(1, "articles", "old.md", "old", "/articles/old/");
        old.date = Some("2026-01-01".to_string());
        old.tags.insert("Rust".to_string());
        let mut new = entry(2, "projects", "new.md", "new", "/projects/new/");
        new.date = Some("2026-09-03".to_string());
        new.tags.insert("Rust".to_string());
        let mut undated = entry(3, "articles", "u.md", "u", "/articles/u/");
        undated.tags.insert("Rust".to_string());
        b.add_entry(old);
        b.add_entry(new);
        b.add_entry(undated);
        let model = b.build().expect("builds");
        let slugs: Vec<&str> = model
            .entries_tagged_by_date("Rust")
            .iter()
            .map(|e| e.slug.0.as_str())
            .collect();
        assert_eq!(slugs, vec!["new", "old", "u"]);
        assert!(model.entries_tagged_by_date("Missing").is_empty());
    }

    #[test]
    fn all_tags_lists_terms_without_empties() {
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "articles", "a.md", "a", "/a/");
        a.tags.insert("Beta Tools".to_string());
        a.tags.insert("alpha Guides".to_string());
        b.add_entry(a);
        b.add_entry(entry(2, "articles", "b.md", "b", "/b/"));
        let model = b.build().expect("builds");
        assert_eq!(
            model.all_tags(),
            vec!["Beta Tools".to_string(), "alpha Guides".to_string()]
        );
    }

    #[test]
    fn regular_entries_exclude_section_roots_newest_first() {
        let mut b = SiteModelBuilder::new();
        let mut index = entry(1, "posts", "_index.md", "", "/posts/");
        index.section_root = true;
        index.title = "Posts".to_string();
        b.add_entry(index);
        let mut old = entry(2, "posts", "old.md", "old", "/posts/old/");
        old.date = Some("2026-01-01".to_string());
        b.add_entry(old);
        let mut new = entry(3, "notes", "new.md", "new", "/notes/new/");
        new.date = Some("2026-09-03".to_string());
        b.add_entry(new);
        let model = b.build().expect("builds");
        let slugs: Vec<&str> = model
            .regular_entries_by_date()
            .iter()
            .map(|e| e.slug.0.as_str())
            .collect();
        assert_eq!(slugs, vec!["new", "old"]);
    }
}
