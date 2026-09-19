//! Minimal TOML + Serde configuration types.
//!
//! The structs here are pure data: no filesystem access. File loading lives
//! in `signal-cli`. This keeps `signal-core` free of I/O.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ids::CollectionId;

/// Root configuration, mirrors `signal.toml`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalConfig {
    /// Site-wide settings.
    pub site: SiteConfig,
    /// Per-collection settings, keyed by collection name.
    #[serde(default)]
    pub collections: BTreeMap<String, CollectionConfig>,
    /// Taxonomy settings. `None` means no taxonomy pages are generated
    /// (e.g. the minimal fixture).
    #[serde(default)]
    pub taxonomy: Option<TaxonomyConfig>,
    /// Feed settings. `None` means no feeds are generated.
    ///
    /// Feeds need absolute URLs, so they are additionally skipped when no
    /// `site.base_url` is configured.
    #[serde(default)]
    pub feed: Option<FeedConfig>,
    /// Named menus, keyed by menu name (e.g. `main`).
    ///
    /// Only `main` is currently consumed (see `signal_core::menu`); other
    /// names are accepted and ignored. Absent means no navigation.
    #[serde(default)]
    pub menus: BTreeMap<String, MenuConfig>,
}

/// Site-wide settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteConfig {
    /// Human-readable site title.
    pub title: String,
    /// Canonical base URL, e.g. `https://example.com/`.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Collection rendered as the home page's listing source, if any.
    ///
    /// When set, one home artifact is generated at `/`. Unset means no home
    /// page (e.g. the minimal fixture).
    #[serde(default)]
    pub home_collection: Option<String>,
    /// Template for the home page. Defaults to `home.html` when a home page
    /// is enabled.
    #[serde(default)]
    pub home_template: Option<String>,
    /// Default site author, used when an entry sets no `author`.
    ///
    /// Omitted from bylines and structured data when unset; never fabricated.
    #[serde(default)]
    pub author: Option<String>,
    /// Presentation date format (strftime-style subset, see
    /// `signal_core::format_date`), e.g. `"%-d %B %Y"` for `2 September 2026`.
    ///
    /// The model always stores machine-readable `YYYY-MM-DD`; this only
    /// controls display. Fixed English month names: formatting never depends
    /// on machine locale or timezone. Defaults to `"%Y-%m-%d"`.
    #[serde(default)]
    pub date_format: Option<String>,
}

/// Per-collection settings (vertical-slice subset).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionConfig {
    /// Source directory relative to the site root, e.g. `content/posts`.
    /// Defaults to `content/<name>`.
    #[serde(default)]
    pub source: Option<String>,
    /// Route prefix, e.g. `/posts/`. Defaults to `/<name>/`.
    #[serde(default)]
    pub route_prefix: Option<String>,
    /// Template used to render this collection's entries, e.g. `post.html`.
    /// Defaults to `post.html`.
    #[serde(default)]
    pub template: Option<String>,
    /// Human-readable section title, e.g. `Articles`.
    ///
    /// Used when no `_index.md` entry supplies the section title.
    #[serde(default)]
    pub title: Option<String>,
    /// Section description for listing headers.
    ///
    /// Used when no `_index.md` entry supplies a description.
    #[serde(default)]
    pub description: Option<String>,
    /// Template used to render this collection's section page.
    /// Defaults to `section.html`.
    #[serde(default)]
    pub section_template: Option<String>,
}

/// Default maximum items per feed.
pub const DEFAULT_FEED_LIMIT: usize = 20;

/// Feed settings.
///
/// Presence of the `[feed]` table opts the site into feed generation:
/// the main feed plus taxonomy feeds when `[taxonomy]` is also present.
/// Title and description patterns are deliberate defaults; per-feed copy
/// customization arrives only if a site requires it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedConfig {
    /// Maximum items per feed. Defaults to [`DEFAULT_FEED_LIMIT`].
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One configured menu item: a label plus a destination URL.
///
/// This is input only. Templates never see this type; resolution produces
/// the validated render model (see `signal_core::menu`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItemConfig {
    /// Display label, e.g. `Articles`.
    pub label: String,
    /// Destination: an internal route (`/articles/`) or an absolute
    /// `http(s)` URL (`https://example.org/`).
    pub url: String,
}

/// One named menu: an ordered list of items.
///
/// Configuration order is the display order; Signal never re-sorts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuConfig {
    /// Menu items in display order.
    #[serde(default)]
    pub items: Vec<MenuItemConfig>,
}
/// Taxonomy settings for the site's single tag taxonomy.
///
/// One taxonomy (e.g. Hugo-style `topics`, normalized into the model's
/// `tags` index). No generic multi-taxonomy framework is provided; a second
/// taxonomy would extend this struct first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxonomyConfig {
    /// Route prefix for the taxonomy index and term pages, e.g. `/topics/`.
    /// Defaults to `/topics/`.
    #[serde(default)]
    pub route_prefix: Option<String>,
    /// Title for the taxonomy index page, e.g. `Topics`.
    /// Defaults to `Topics`.
    #[serde(default)]
    pub title: Option<String>,
    /// Template for the taxonomy index page. Defaults to `topics.html`.
    #[serde(default)]
    pub index_template: Option<String>,
    /// Template for term pages. Defaults to `topic.html`.
    #[serde(default)]
    pub term_template: Option<String>,
}

impl SignalConfig {
    /// Parse configuration from a TOML string. File I/O stays in `signal-cli`.
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Effective presentation date format: configured `site.date_format`
    /// or [`crate::DEFAULT_DATE_FORMAT`].
    pub fn date_format_str(&self) -> &str {
        self.site
            .date_format
            .as_deref()
            .filter(|f| !f.trim().is_empty())
            .unwrap_or(crate::DEFAULT_DATE_FORMAT)
    }

    /// Collection ids declared in configuration, in deterministic order.
    pub fn collection_ids(&self) -> Vec<CollectionId> {
        self.collections
            .keys()
            .map(|k| CollectionId::new(k.clone()))
            .collect()
    }

    /// Route prefix for a collection, normalized with leading and trailing
    /// slashes. Defaults to `/<name>/`.
    pub fn route_prefix_for(&self, collection: &str) -> String {
        let raw = self
            .collections
            .get(collection)
            .and_then(|c| c.route_prefix.clone())
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| format!("/{collection}/"));
        let trimmed = raw.trim_matches('/').to_string();
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{trimmed}/")
        }
    }

    /// Source directory for a collection, relative to the site root.
    /// Defaults to `content/<name>`.
    pub fn source_dir_for(&self, collection: &str) -> String {
        self.collections
            .get(collection)
            .and_then(|c| c.source.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("content/{collection}"))
    }
}

// `toml` is intentionally a dependency of `signal-core` only for pure
// string deserialization. No `std::fs` access happens in this crate.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let cfg = SignalConfig::from_toml_str(
            "[site]\ntitle = \"Example\"\n[collections.posts]\nsource = \"content/posts\"\n",
        )
        .expect("config parses");
        assert_eq!(cfg.site.title, "Example");
        assert_eq!(cfg.collection_ids(), vec![CollectionId::new("posts")]);
    }
}
