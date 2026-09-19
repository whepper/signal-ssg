//! RSS 2.0 serialization from normalized model data.
//!
//! Feeds are built directly from [`ContentEntry`] values — never from
//! rendered HTML. XML structure is written with `quick-xml`'s event writer
//! (balanced tags by construction); text goes through `BytesText::new` and
//! attribute values through `escape_attribute`, so front matter can never
//! inject markup. MiniJinja is deliberately bypassed: feed schemas are
//! fixed, and string-templated XML is where escaping bugs hide. The
//! [`Generator`] boundary is unchanged — generators plan `ArtifactSpec`
//! values; these serializers produce the bytes at resolve time, one
//! artifact at a time.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::writer::Writer;
use serde::{Deserialize, Serialize};
use signal_core::{canonical_url, rfc2822_date, CollectionId, ContentEntry};
use std::io::Cursor;

use crate::Generator;
use crate::{ArtifactKind, ArtifactSpec, GenerateError, SiteModel};

/// Words kept by the body fallback when an entry has no front-matter
/// `description`. Matches the reference auto-summary length; the author's
/// own description always wins when present.
pub const FEED_EXCERPT_WORDS: usize = 70;

/// One feed item: all values pre-resolved, `None` date omits `<pubDate>`.
///
/// Serializable so manifest query digests can hash the exact consumed
/// projection (never bare IDs: title/date/description edits must invalidate
/// feeds that embed them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedItem {
    /// Item title.
    pub title: String,
    /// Absolute item URL (also the guid).
    pub url: String,
    /// RFC 2822 publication timestamp, if the entry is dated.
    pub pub_date: Option<String>,
    /// Summary HTML or plain-text excerpt (escaped at write time).
    pub description: String,
}

/// Build one item from an entry. Description prefers the author's
/// front-matter summary; otherwise the first [`FEED_EXCERPT_WORDS`] words
/// of the rendered body as plain text.
pub fn feed_item(entry: &ContentEntry, base_url: &str) -> FeedItem {
    FeedItem {
        title: entry.title.clone(),
        url: canonical_url(base_url, &entry.route),
        pub_date: entry.date.as_deref().and_then(rfc2822_date),
        description: entry
            .description
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| excerpt(&entry.body.html)),
    }
}

/// Plain-text excerpt: strip tags, unescape entities, keep the first
/// `FEED_EXCERPT_WORDS` whitespace-separated words joined by single spaces.
pub fn excerpt(html: &str) -> String {
    let text = strip_tags(html);
    let unescaped = unescape_entities(&text);
    unescaped
        .split_whitespace()
        .take(FEED_EXCERPT_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn unescape_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            break;
        };
        let entity = &tail[..=semi];
        match entity {
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&amp;" => out.push('&'),
            "&quot;" => out.push('"'),
            "&apos;" => out.push('\''),
            _ if entity.starts_with("&#x") || entity.starts_with("&#X") => {
                match u32::from_str_radix(&entity[3..entity.len() - 1], 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(ch) => out.push(ch),
                    None => out.push_str(entity),
                }
            }
            _ if entity.starts_with("&#") => {
                match entity[2..entity.len() - 1]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(ch) => out.push(ch),
                    None => out.push_str(entity),
                }
            }
            _ => out.push_str(entity),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

fn text_element(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    name: &str,
    text: &str,
) -> quick_xml::Result<()> {
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

/// Serialize one RSS 2.0 channel. `last_build_date` should be the newest
/// item timestamp (callers derive it from content); `None` omits the
/// element — feeds never stamp build time, keeping output deterministic.
pub fn channel_xml(
    title: &str,
    link: &str,
    description: &str,
    self_url: &str,
    last_build_date: Option<&str>,
    items: &[FeedItem],
) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer
        .write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("utf-8"),
            Some("yes"),
        )))
        .expect("write decl");
    let rss = BytesStart::new("rss").with_attributes([
        ("version", "2.0"),
        ("xmlns:atom", "http://www.w3.org/2005/Atom"),
    ]);
    writer.write_event(Event::Start(rss)).expect("write rss");
    writer
        .write_event(Event::Start(BytesStart::new("channel")))
        .expect("write channel");
    text_element(&mut writer, "title", title).expect("write title");
    text_element(&mut writer, "link", link).expect("write link");
    text_element(&mut writer, "description", description).expect("write description");
    // `(&str, &str)` attribute tuples escape values on conversion, so raw
    // URLs are passed here and escaped exactly once.
    writer
        .write_event(Event::Empty(BytesStart::new("atom:link").with_attributes(
            [
                ("href", self_url),
                ("rel", "self"),
                ("type", "application/rss+xml"),
            ],
        )))
        .expect("write atom link");
    if let Some(date) = last_build_date {
        text_element(&mut writer, "lastBuildDate", date).expect("write lastBuildDate");
    }
    for item in items {
        writer
            .write_event(Event::Start(BytesStart::new("item")))
            .expect("write item");
        text_element(&mut writer, "title", &item.title).expect("write item title");
        text_element(&mut writer, "link", &item.url).expect("write item link");
        if let Some(date) = &item.pub_date {
            text_element(&mut writer, "pubDate", date).expect("write pubDate");
        }
        text_element(&mut writer, "guid", &item.url).expect("write guid");
        text_element(&mut writer, "description", &item.description).expect("write description");
        writer
            .write_event(Event::End(BytesEnd::new("item")))
            .expect("write item end");
    }
    writer
        .write_event(Event::End(BytesEnd::new("channel")))
        .expect("write channel end");
    writer
        .write_event(Event::End(BytesEnd::new("rss")))
        .expect("write rss end");
    String::from_utf8(writer.into_inner().into_inner()).expect("utf-8 xml")
}

/// Newest item timestamp, for `lastBuildDate`. Content-derived, never now.
pub fn newest_pub_date(items: &[FeedItem]) -> Option<&str> {
    items.iter().find_map(|item| item.pub_date.as_deref())
}

/// Projection: the main feed (`index.xml`) over all regular entries.
pub struct MainFeed {
    limit: usize,
}

impl MainFeed {
    /// Create a main-feed projection capped at `limit` items.
    pub fn new(limit: usize) -> Self {
        Self { limit }
    }

    /// Item cap for the feed.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Feed items: regular (non-section-root) entries newest-first,
    /// capped at the limit. Drafts never appear — they are absent from
    /// the model before this projection runs.
    pub fn items(&self, model: &SiteModel, base_url: &str) -> Vec<FeedItem> {
        model
            .regular_entries_by_date()
            .iter()
            .take(self.limit)
            .map(|entry| feed_item(entry, base_url))
            .collect()
    }
}

/// Channel copy for the main feed.
pub fn main_channel_meta(site_title: &str) -> (String, String) {
    (
        site_title.to_string(),
        format!("Recent content on {site_title}"),
    )
}

/// Channel copy for a scoped feed (section, taxonomy term, or taxonomy
/// label index), following the reference pattern the migrated blog emits:
/// "{Scope} on {Site}" / "Recent content in {Scope} on {Site}".
pub fn scoped_channel_meta(scope: &str, site_title: &str) -> (String, String) {
    (
        format!("{scope} on {site_title}"),
        format!("Recent content in {scope} on {site_title}"),
    )
}

/// Output path of the feed generated for a route (`/posts/` ->
/// `posts/index.xml`, `/` -> `index.xml`).
pub fn feed_path_for_route(route: &str) -> String {
    let trimmed = route.trim_matches('/');
    if trimmed.is_empty() {
        "index.xml".to_string()
    } else {
        format!("{trimmed}/index.xml")
    }
}

/// Projection: one feed per collection (`<prefix>/index.xml`).
///
/// Mirrors the reference behavior where every section publishes a feed
/// alongside its HTML listing: items are the collection's regular
/// entries, newest-first, capped. A collection whose only member is its
/// section root still gets an (empty-item) feed — the listing page
/// exists, and the reference publishes empty feeds for it.
pub struct SectionFeeds {
    collection: CollectionId,
    route_prefix: String,
    limit: usize,
}

impl SectionFeeds {
    /// Create a section-feed projection for one collection route prefix.
    pub fn new(collection: CollectionId, route_prefix: impl Into<String>, limit: usize) -> Self {
        let raw = route_prefix.into();
        let trimmed = raw.trim_matches('/');
        let route_prefix = if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{trimmed}/")
        };
        Self {
            collection,
            route_prefix,
            limit,
        }
    }

    /// Item cap per feed.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Items: the collection's regular (non-section-root) entries,
    /// newest-first, capped — the same selection the section HTML listing
    /// makes, projected to feed items.
    pub fn items(&self, model: &SiteModel, base_url: &str) -> Vec<FeedItem> {
        model
            .entries_in_collection_by_date(&self.collection)
            .iter()
            .filter(|entry| !entry.section_root)
            .take(self.limit)
            .map(|entry| feed_item(entry, base_url))
            .collect()
    }
}

impl Generator for SectionFeeds {
    fn name(&self) -> &str {
        "section-feeds"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new(
            feed_path_for_route(&self.route_prefix),
            ArtifactKind::Rss,
        )])
    }
}

impl Generator for MainFeed {
    fn name(&self) -> &str {
        "main-feed"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new("index.xml", ArtifactKind::Rss)])
    }
}

/// Projection: one feed per taxonomy term (`<root>/<slug>/index.xml`)
/// plus the taxonomy label-index feed (`<root>/index.xml`).
///
/// The label feed lists the terms themselves — one item per term with the
/// newest member's date as its timestamp — mirroring how the reference
/// publishes an RSS alongside the terms listing page.
pub struct TaxonomyFeeds {
    root: String,
    limit: usize,
}

impl TaxonomyFeeds {
    /// Create taxonomy-feed projections under `root` (slashes normalized).
    pub fn new(root: impl Into<String>, limit: usize) -> Self {
        let root = root.into();
        Self {
            root: crate::normalize_root(&root),
            limit,
        }
    }

    /// Item cap per feed.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Taxonomy root route (normalized, leading and trailing slashes).
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Items for one term: tagged regular entries newest-first, capped.
    pub fn term_items(&self, model: &SiteModel, tag: &str, base_url: &str) -> Vec<FeedItem> {
        model
            .entries_tagged_by_date(tag)
            .iter()
            .filter(|entry| !entry.section_root)
            .take(self.limit)
            .map(|entry| feed_item(entry, base_url))
            .collect()
    }

    /// Items for the label-index feed: one per term in display order,
    /// titled with the label, linked to the term route, and timestamped
    /// with the newest member's date (undated membership leaves the
    /// timestamp absent — never fabricated). Capped like every feed.
    ///
    /// A term-slug collision (a content error the plan would reject
    /// anyway) yields an empty list rather than a partial projection.
    pub fn label_items(&self, model: &SiteModel, base_url: &str) -> Vec<FeedItem> {
        let terms = match crate::topic_terms(model, &self.root, signal_core::DEFAULT_DATE_FORMAT) {
            Ok(terms) => terms,
            Err(_) => return Vec::new(),
        };
        terms
            .into_iter()
            .take(self.limit)
            .map(|term| FeedItem {
                title: term.label.clone(),
                url: signal_core::canonical_url(
                    base_url,
                    &signal_core::Route::new(term.route.clone()),
                ),
                pub_date: term
                    .entries
                    .iter()
                    .find_map(|member| member.date.clone())
                    .and_then(|date| rfc2822_date(&date)),
                description: String::new(),
            })
            .collect()
    }
}

impl Generator for TaxonomyFeeds {
    fn name(&self) -> &str {
        "taxonomy-feeds"
    }

    fn generate(&self, model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        let mut specs = vec![ArtifactSpec::new(
            feed_path_for_route(&self.root),
            ArtifactKind::Rss,
        )];
        for term in crate::topic_terms(model, &self.root, signal_core::DEFAULT_DATE_FORMAT)? {
            specs.push(ArtifactSpec::new(
                format!("{}/index.xml", term.route.trim_matches('/')),
                ArtifactKind::Rss,
            ));
        }
        specs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(specs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::reader::Reader;
    use signal_core::{CollectionId, ContentId, Route, Slug, SourceRef};

    fn entry(id: u32, title: &str, date: Option<&str>, description: Option<&str>) -> ContentEntry {
        let mut e = ContentEntry::new(
            ContentId(id),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), format!("{id}.md")),
            Slug::new(format!("e{id}")),
            Route::new(format!("/posts/e{id}/")),
            title,
        );
        e.date = date.map(str::to_string);
        e.description = description.map(str::to_string);
        e
    }

    /// Parse event kinds with quick-xml: fails on malformed XML, which is
    /// the actual validation (not string matching). Text is accumulated
    /// across `Text` and `GeneralRef` events (quick-xml splits `&lt;` and
    /// friends out of text runs) and entity-decoded.
    fn parse_ok(xml: &str) -> Vec<String> {
        let mut reader = Reader::from_str(xml);
        // No trimming: the writer emits no indentation, and trimming would
        // eat spaces adjacent to entity references inside text runs.
        reader.config_mut().trim_text(false);
        let mut kinds = Vec::new();
        let mut pending = String::new();
        loop {
            let flush = !pending.is_empty();
            match reader.read_event().expect("well-formed xml") {
                quick_xml::events::Event::Eof => {
                    if flush {
                        kinds.push(format!("text:{}", std::mem::take(&mut pending)));
                    }
                    break;
                }
                quick_xml::events::Event::Start(e) => {
                    if flush {
                        kinds.push(format!("text:{}", std::mem::take(&mut pending)));
                    }
                    kinds.push(format!("start:{}", e.name().into_inner()));
                }
                quick_xml::events::Event::Text(e) => {
                    pending.push_str(&e.xml10_content());
                }
                quick_xml::events::Event::GeneralRef(e) => {
                    let content = e.xml10_content();
                    let raw = format!("&{content};");
                    let decoded = quick_xml::escape::unescape(&raw).expect("decodable entity");
                    pending.push_str(&decoded);
                }
                _ => {
                    if flush {
                        kinds.push(format!("text:{}", std::mem::take(&mut pending)));
                    }
                }
            }
        }
        kinds
    }

    #[test]
    fn excerpt_prefers_plain_words_and_truncates() {
        let html = "<p>Hello <strong>brave</strong> world &amp; friends.</p><p>More.</p>";
        assert_eq!(excerpt(html), "Hello brave world & friends. More.");
        let long = format!("<p>{}</p>", "word ".repeat(100));
        assert_eq!(
            excerpt(&long).split_whitespace().count(),
            FEED_EXCERPT_WORDS
        );
    }

    #[test]
    fn excerpt_unescapes_numeric_entities() {
        assert_eq!(excerpt("<p>A&#x2f;b&#47;c</p>"), "A/b/c");
        assert_eq!(excerpt("<p>&ldquo;hi&rdquo;</p>"), "&ldquo;hi&rdquo;");
    }

    #[test]
    fn feed_item_prefers_description_over_excerpt() {
        let mut e = entry(1, "T", Some("2026-09-02"), Some("Author summary."));
        e.body = signal_core::RenderedBody {
            html: "<p>Body words here.</p>".to_string(),
            ..Default::default()
        };
        let item = feed_item(&e, "https://example.com");
        assert_eq!(item.url, "https://example.com/posts/e1/");
        assert_eq!(
            item.pub_date.as_deref(),
            Some("Wed, 02 Sep 2026 00:00:00 +0000")
        );
        assert_eq!(item.description, "Author summary.");
    }

    #[test]
    fn channel_escapes_hostile_values_and_parses() {
        let items = vec![FeedItem {
            title: "A <b>& \"quoted\"".to_string(),
            url: "https://example.com/a/?x=1&y=2".to_string(),
            pub_date: None,
            description: "Body </item> tricks".to_string(),
        }];
        let xml = channel_xml(
            "T <tle>",
            "https://example.com/",
            "D & D",
            "https://example.com/index.xml",
            None,
            &items,
        );
        assert!(!xml.contains("<b>"), "got: {xml}");
        let kinds = parse_ok(&xml);
        assert!(
            kinds.iter().any(|k| k == "text:A <b>& \"quoted\""),
            "item title round-trips decoded: {kinds:?}\nxml: {xml}"
        );
        assert!(
            kinds.iter().any(|k| k == "text:Body </item> tricks"),
            "item body round-trips decoded: {kinds:?}\nxml: {xml}"
        );
        // Omitted optionals leave no empty elements.
        assert!(!xml.contains("<pubDate"), "got: {xml}");
        assert!(!xml.contains("<lastBuildDate"), "got: {xml}");
    }

    #[test]
    fn main_feed_plans_index_xml() {
        let specs = MainFeed::new(20)
            .generate(&signal_core::SiteModelBuilder::new().build().expect("empty"))
            .expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "index.xml");
        assert_eq!(specs[0].kind, ArtifactKind::Rss);
    }

    #[test]
    fn main_feed_items_are_capped_newest_first_without_section_roots() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        let mut index = entry(1, "Section", None, None);
        index.section_root = true;
        b.add_entry(index);
        b.add_entry(entry(2, "Old", Some("2026-01-01"), None));
        b.add_entry(entry(3, "New", Some("2026-09-03"), None));
        let model = b.build().expect("builds");
        let feed = MainFeed::new(1);
        let items = feed.items(&model, "https://example.com");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "New");
        assert_eq!(items[0].url, "https://example.com/posts/e3/");
    }

    #[test]
    fn term_items_select_tag_members_newest_first() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "Old", Some("2026-01-01"), None);
        a.tags.insert("Rust".to_string());
        let mut c = entry(2, "New", Some("2026-09-03"), None);
        c.tags.insert("Rust".to_string());
        b.add_entry(a);
        b.add_entry(c);
        b.add_entry(entry(3, "Other", Some("2026-06-01"), None));
        let model = b.build().expect("builds");
        let feeds = TaxonomyFeeds::new("/topics/", 10);
        let items = feeds.term_items(&model, "Rust", "https://example.com");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "New");
        assert_eq!(items[1].title, "Old");
        assert!(feeds
            .term_items(&model, "Missing", "https://example.com")
            .is_empty());
    }

    #[test]
    fn channel_copy_follows_reference_patterns() {
        assert_eq!(
            main_channel_meta("Sample Site"),
            (
                "Sample Site".to_string(),
                "Recent content on Sample Site".to_string()
            )
        );
        assert_eq!(
            scoped_channel_meta("Rust", "Sample Site"),
            (
                "Rust on Sample Site".to_string(),
                "Recent content in Rust on Sample Site".to_string()
            )
        );
        assert_eq!(
            scoped_channel_meta("Articles", "Sample Site"),
            (
                "Articles on Sample Site".to_string(),
                "Recent content in Articles on Sample Site".to_string()
            )
        );
    }

    #[test]
    fn feed_path_for_route_maps_routes_to_index_xml() {
        assert_eq!(feed_path_for_route("/posts/"), "posts/index.xml");
        assert_eq!(feed_path_for_route("/"), "index.xml");
        assert_eq!(feed_path_for_route("/topics/"), "topics/index.xml");
    }

    #[test]
    fn section_feed_plans_prefix_feed_and_selects_membership() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        let mut index = entry(1, "Section root", None, None);
        index.route = Route::new("/posts/");
        index.section_root = true;
        b.add_entry(index);
        let mut old = entry(2, "Old", Some("2026-01-01"), None);
        old.route = Route::new("/posts/old/");
        b.add_entry(old);
        let mut new = entry(3, "New", Some("2026-09-03"), None);
        new.route = Route::new("/posts/new/");
        b.add_entry(new);
        // An unrelated collection must not contribute items.
        let mut other = entry(4, "Elsewhere", Some("2026-06-01"), None);
        other.collection = CollectionId::new("notes");
        other.route = Route::new("/notes/x/");
        b.add_entry(other);
        let model = b.build().expect("builds");

        let feeds = SectionFeeds::new(CollectionId::new("posts"), "/posts/", 10);
        let specs = feeds.generate(&model).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "posts/index.xml");
        assert_eq!(specs[0].kind, ArtifactKind::Rss);

        let items = feeds.items(&model, "https://example.com");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "New");
        assert_eq!(items[1].title, "Old");
        assert_eq!(items[0].url, "https://example.com/posts/new/");
    }

    #[test]
    fn section_feed_caps_membership_and_survives_empty_sections() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        for (id, date) in [(1, "2026-01-01"), (2, "2026-02-01"), (3, "2026-03-01")] {
            let mut e = entry(id, "Dated", Some(date), None);
            e.route = Route::new(format!("/posts/e{id}/"));
            b.add_entry(e);
        }
        let model = b.build().expect("builds");
        let items = SectionFeeds::new(CollectionId::new("posts"), "/posts/", 2)
            .items(&model, "https://example.com");
        assert_eq!(items.len(), 2, "capped");
        assert_eq!(items[0].title, "Dated"); // newest first
                                             // A collection with no members still plans an empty feed.
        let empty = SectionFeeds::new(CollectionId::new("missing"), "/missing/", 5);
        assert!(empty.items(&model, "https://example.com").is_empty());
        let specs = empty.generate(&model).expect("generates");
        assert_eq!(specs[0].path, "missing/index.xml");
    }

    #[test]
    fn taxonomy_feeds_plan_label_index_and_term_feeds() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "Alpha", Some("2026-01-01"), None);
        a.route = Route::new("/posts/alpha/");
        a.tags.insert("Rust".to_string());
        let mut c = entry(2, "Newest", Some("2026-09-03"), None);
        c.route = Route::new("/notes/newest/");
        c.tags.insert("Rust".to_string());
        b.add_entry(a);
        b.add_entry(c);
        let model = b.build().expect("builds");

        let feeds = TaxonomyFeeds::new("/topics/", 20);
        let specs = feeds.generate(&model).expect("generates");
        let paths: Vec<&str> = specs.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["topics/index.xml", "topics/rust/index.xml"]);

        // Label feed items: one per term, newest member's date, term link.
        let items = feeds.label_items(&model, "https://example.com");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Rust");
        assert_eq!(items[0].url, "https://example.com/topics/rust/");
        assert_eq!(
            items[0].pub_date.as_deref(),
            Some("Thu, 03 Sep 2026 00:00:00 +0000")
        );
        assert_eq!(items[0].description, "");
        // Deterministic ordering: same model, same bytes.
        assert_eq!(items, feeds.label_items(&model, "https://example.com"));
    }

    #[test]
    fn label_items_omit_dates_for_undated_membership_and_cap_terms() {
        use signal_core::SiteModelBuilder;
        let mut b = SiteModelBuilder::new();
        let mut a = entry(1, "Undated one", None, None);
        a.route = Route::new("/posts/a/");
        a.tags.insert("Zeta".to_string());
        let mut c = entry(2, "Dated two", Some("2026-05-05"), None);
        c.route = Route::new("/posts/c/");
        c.tags.insert("Alpha Tag".to_string());
        b.add_entry(a);
        b.add_entry(c);
        let model = b.build().expect("builds");
        let items = TaxonomyFeeds::new("/topics/", 20).label_items(&model, "https://example.com");
        assert_eq!(items.len(), 2);
        // Alphabetical terms: "Alpha Tag" first.
        assert_eq!(items[0].title, "Alpha Tag");
        assert_eq!(items[0].url, "https://example.com/topics/alpha-tag/");
        assert_eq!(items[1].title, "Zeta");
        assert!(items[1].pub_date.is_none(), "undated members: no date");
        let capped = TaxonomyFeeds::new("/topics/", 1).label_items(&model, "https://example.com");
        assert_eq!(capped.len(), 1);
    }
}
