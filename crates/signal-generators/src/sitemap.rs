//! Sitemap XML from the explicit public route inventory.
//!
//! The URL set derives from the model plus configuration — entry routes,
//! collection prefixes, the home route when configured, and taxonomy
//! routes — never from generated files or rendered HTML. Static assets,
//! feeds, and drafts are excluded by construction: they never enter the
//! inventory. `<lastmod>` uses entry dates (`last_modified`, else `date`,
//! both already `YYYY-MM-DD` and valid sitemap timestamps); list pages
//! carry no reliable modification date, so they omit it rather than
//! fabricating one from build or filesystem time.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::writer::Writer;
use serde::{Deserialize, Serialize};
use signal_core::{canonical_url, SiteModel};
use std::collections::BTreeSet;
use std::io::Cursor;

use crate::{ArtifactKind, ArtifactSpec, GenerateError, Generator};

/// One sitemap URL: absolute location plus optional modification date.
///
/// Serializable so the manifest can digest the exact route inventory the
/// sitemap consumes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SitemapUrl {
    /// Absolute URL.
    pub loc: String,
    /// `YYYY-MM-DD` modification date, if the model provides one.
    pub lastmod: Option<String>,
}

/// Explicit public route inventory for one site.
///
/// `sections` are collection route prefixes, `home` adds `/` when a home
/// page is generated, and `taxonomy_root` adds the taxonomy index route
/// (term routes come from the model's tags). Sorted and deduplicated by
/// construction.
pub fn sitemap_urls(
    model: &SiteModel,
    base_url: &str,
    home: bool,
    sections: &[String],
    taxonomy_root: Option<&str>,
) -> Vec<SitemapUrl> {
    let mut routes: BTreeSet<String> = BTreeSet::new();
    if home {
        routes.insert("/".to_string());
    }
    for section in sections {
        routes.insert(normalize_route(section));
    }
    for entry in model.entries() {
        routes.insert(entry.route.0.clone());
    }
    if let Some(root) = taxonomy_root {
        let root = normalize_route(root);
        routes.insert(root.clone());
        for tag in model.all_tags() {
            routes.insert(format!("{}{}/", root, signal_core::slugify(&tag)));
        }
    }
    let by_route: std::collections::BTreeMap<String, &signal_core::ContentEntry> = model
        .entries()
        .map(|entry| (entry.route.0.clone(), entry))
        .collect();
    routes
        .into_iter()
        .map(|route| {
            let lastmod = by_route.get(&route).and_then(|entry| {
                entry
                    .last_modified
                    .clone()
                    .filter(|d| !d.trim().is_empty())
                    .or_else(|| entry.date.clone().filter(|d| !d.trim().is_empty()))
            });
            SitemapUrl {
                loc: canonical_url(base_url, &signal_core::Route::new(route)),
                lastmod,
            }
        })
        .collect()
}

fn normalize_route(route: &str) -> String {
    let trimmed = route.trim_matches('/').to_string();
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}/")
    }
}

/// Serialize the URL set as `sitemap.xml`. Deterministic for identical
/// input: sorted by location, no timestamps, no counts.
pub fn sitemap_xml(urls: &[SitemapUrl]) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    writer
        .write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("utf-8"),
            Some("yes"),
        )))
        .expect("write decl");
    let urlset = BytesStart::new("urlset")
        .with_attributes([("xmlns", "http://www.sitemaps.org/schemas/sitemap/0.9")]);
    writer
        .write_event(Event::Start(urlset))
        .expect("write urlset");
    let mut sorted: Vec<&SitemapUrl> = urls.iter().collect();
    sorted.sort();
    for url in sorted {
        writer
            .write_event(Event::Start(BytesStart::new("url")))
            .expect("write url");
        writer
            .write_event(Event::Start(BytesStart::new("loc")))
            .expect("write loc");
        writer
            .write_event(Event::Text(BytesText::new(&url.loc)))
            .expect("write loc text");
        writer
            .write_event(Event::End(BytesEnd::new("loc")))
            .expect("write loc end");
        if let Some(lastmod) = &url.lastmod {
            writer
                .write_event(Event::Start(BytesStart::new("lastmod")))
                .expect("write lastmod");
            writer
                .write_event(Event::Text(BytesText::new(lastmod)))
                .expect("write lastmod text");
            writer
                .write_event(Event::End(BytesEnd::new("lastmod")))
                .expect("write lastmod end");
        }
        writer
            .write_event(Event::End(BytesEnd::new("url")))
            .expect("write url end");
    }
    writer
        .write_event(Event::End(BytesEnd::new("urlset")))
        .expect("write urlset end");
    String::from_utf8(writer.into_inner().into_inner()).expect("utf-8 xml")
}

/// Projection: the sitemap (`sitemap.xml`).
pub struct Sitemap;

impl Sitemap {
    /// Create the sitemap projection.
    pub fn new() -> Self {
        Self
    }
}

impl Default for Sitemap {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for Sitemap {
    fn name(&self) -> &str {
        "sitemap"
    }

    fn generate(&self, _model: &SiteModel) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new(
            "sitemap.xml",
            ArtifactKind::Sitemap,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::reader::Reader;
    use signal_core::{CollectionId, ContentEntry, ContentId, Route, Slug, SourceRef};

    fn entry(id: u32, collection: &str, slug: &str, date: Option<&str>) -> ContentEntry {
        let mut e = ContentEntry::new(
            ContentId(id),
            CollectionId::new(collection),
            SourceRef::new(CollectionId::new(collection), format!("{slug}.md")),
            Slug::new(slug),
            Route::new(format!("/{collection}/{slug}/")),
            format!("Title {slug}"),
        );
        e.date = date.map(str::to_string);
        e
    }

    fn model() -> SiteModel {
        let mut b = signal_core::SiteModelBuilder::new();
        let mut index = entry(1, "posts", "", None);
        index.route = Route::new("/posts/".to_string());
        index.section_root = true;
        index.title = "Posts".to_string();
        b.add_entry(index);
        let mut dated = entry(2, "posts", "a", Some("2026-09-02"));
        dated.last_modified = Some("2026-09-10".to_string());
        dated.tags.insert("Rust".to_string());
        b.add_entry(dated);
        b.add_entry(entry(3, "posts", "b", None));
        b.build().expect("builds")
    }

    #[test]
    fn inventory_covers_pages_but_not_support_files() {
        let urls = sitemap_urls(
            &model(),
            "https://example.com",
            true,
            &["/posts/".to_string()],
            Some("/topics/"),
        );
        let locs: Vec<&str> = urls.iter().map(|u| u.loc.as_str()).collect();
        assert_eq!(
            locs,
            vec![
                "https://example.com/",
                "https://example.com/posts/",
                "https://example.com/posts/a/",
                "https://example.com/posts/b/",
                "https://example.com/topics/",
                "https://example.com/topics/rust/",
            ]
        );
        // lastmod prefers last_modified, falls back to date, omits otherwise.
        let lastmods: Vec<Option<&str>> = urls.iter().map(|u| u.lastmod.as_deref()).collect();
        assert_eq!(
            lastmods,
            vec![None, None, Some("2026-09-10"), None, None, None]
        );
    }

    #[test]
    fn sitemap_serializes_sorted_valid_xml() {
        let urls = sitemap_urls(&model(), "https://example.com", false, &[], None);
        let xml = sitemap_xml(&urls);
        assert!(xml.starts_with("<?xml"), "got: {xml}");
        let mut reader = Reader::from_str(&xml);
        let mut locs = Vec::new();
        let mut lastmods = 0;
        let mut pending: Option<String> = None;
        loop {
            match reader.read_event().expect("well-formed xml") {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) if e.name().into_inner() == "loc" => {
                    pending = Some(String::new());
                }
                quick_xml::events::Event::Text(e) if pending.is_some() => {
                    pending
                        .as_mut()
                        .expect("pending")
                        .push_str(&e.xml10_content());
                }
                quick_xml::events::Event::GeneralRef(e) if pending.is_some() => {
                    let content = e.xml10_content();
                    let raw = format!("&{content};");
                    let decoded = quick_xml::escape::unescape(&raw).expect("decodable entity");
                    pending.as_mut().expect("pending").push_str(&decoded);
                }
                quick_xml::events::Event::End(e) if e.name().into_inner() == "loc" => {
                    locs.push(pending.take().expect("loc text"));
                }
                quick_xml::events::Event::Start(e) if e.name().into_inner() == "lastmod" => {
                    lastmods += 1;
                }
                _ => {}
            }
        }
        assert_eq!(locs.len(), 3);
        assert!(locs.windows(2).all(|w| w[0] <= w[1]), "sorted: {locs:?}");
        assert_eq!(lastmods, 1);
        assert!(!xml.contains("changefreq"), "got: {xml}");
        assert!(!xml.contains("priority"), "got: {xml}");
    }
}
