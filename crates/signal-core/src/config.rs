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
    /// Git-derived metadata settings. `None` (the default) means Signal
    /// never consults Git.
    #[serde(default)]
    pub git: Option<GitConfig>,
    /// Related-entry settings. `None` means the default cap applies; the
    /// projection itself is always available to entry templates.
    #[serde(default)]
    pub related: Option<RelatedConfig>,
    /// robots.txt settings. Presence of the `[robots]` table opts the site
    /// into generating `robots.txt`; `None` (the default) means no robots
    /// file is planned.
    #[serde(default)]
    pub robots: Option<RobotsConfig>,
    /// Output settings. Absent means defaults (no minification).
    #[serde(default)]
    pub output: OutputConfig,
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
    /// Template for the themed not-found page. When set, `404.html` is
    /// planned at the output root and rendered with this template (site
    /// chrome; no route, no canonical URL, no active menu state). Absent
    /// means no 404 artifact.
    ///
    /// The output path is fixed: static hosts resolve unknown paths to
    /// `404.html` by convention, so the template name is configured but
    /// the destination is not.
    #[serde(default)]
    pub not_found_template: Option<String>,
    /// Default site author, used when an entry sets no `author`.
    ///
    /// Omitted from bylines and structured data when unset; never fabricated.
    #[serde(default)]
    pub author: Option<String>,
    /// Site-wide description, used by templates as the fallback behind a
    /// page's own `description` (e.g. `<meta name="description">` and
    /// `og:description` on pages without one).
    ///
    /// Omitted from contexts when unset or blank; never fabricated. It
    /// flows only into template contexts — feeds, sitemap, and search keep
    /// their own fixed channel copy.
    #[serde(default)]
    pub description: Option<String>,
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

/// Git-derived metadata settings.
///
/// Presence of the `[git]` table opts the site into Git-derived metadata;
/// keys select which fields participate. Absent (the default) means Signal
/// never consults Git. Git is advisory: when it is unavailable — no
/// repository, no `git` binary — the build proceeds without derived values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitConfig {
    /// Derive `last_modified` from the last commit that touched each source
    /// file (author date, `YYYY-MM-DD`). An explicit front-matter `lastmod`
    /// always wins over the derived value.
    #[serde(default)]
    pub last_modified: bool,
}

/// Related-entry settings.
///
/// Caps the shared-tag related-entries projection exposed to entry
/// templates. The projection itself is always available; only the cap is
/// tunable here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedConfig {
    /// Maximum related entries per entry. Defaults to the projection's
    /// default cap when unset (or zero, which is treated as unset).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// robots.txt settings.
///
/// Presence of the `[robots]` table opts the site into robots.txt
/// generation; the content is a fixed, deterministic allow-all policy that
/// references the sitemap when `site.base_url` is configured (the same
/// condition under which the sitemap itself is planned). Per-agent rules
/// and disallow paths stay out until a real site requires them — a genuine
/// future requirement justifies extending this table, not speculation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RobotsConfig {}

/// Output settings.
///
/// Present as the `[output]` table. Everything here is off unless explicitly
/// enabled, so existing sites byte-identical output is unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Minify rendered HTML pages (`[output] minify_html = true`).
    ///
    /// Off by default. When enabled, every template-rendered HTML artifact
    /// (entry pages, section listings, home, taxonomy pages, the themed
    /// 404) is passed through deterministic HTML-aware minification after
    /// rendering. Non-HTML artifacts (feeds, sitemap, search index, robots,
    /// static files) are never minified, and source content is untouched.
    /// The flag rides the whole-config digest, so toggling it rebuilds
    /// exactly the artifacts that already depend on configuration.
    #[serde(default)]
    pub minify_html: bool,
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

    /// Whether Git-derived `last_modified` is enabled
    /// (`[git] last_modified = true`). Off unless explicitly configured.
    pub fn git_last_modified(&self) -> bool {
        self.git.as_ref().is_some_and(|git| git.last_modified)
    }

    /// Whether rendered HTML is minified (`[output] minify_html = true`).
    /// Off unless explicitly configured.
    pub fn minify_html(&self) -> bool {
        self.output.minify_html
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

    #[test]
    fn site_description_defaults_absent_and_parses_when_set() {
        let plain = SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n").expect("parses");
        assert_eq!(plain.site.description, None);
        let cfg =
            SignalConfig::from_toml_str("[site]\ntitle = \"T\"\ndescription = \"Site summary.\"\n")
                .expect("parses");
        assert_eq!(cfg.site.description.as_deref(), Some("Site summary."));
    }

    #[test]
    fn html_minification_is_disabled_by_default_and_opt_in() {
        let plain = SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n").expect("parses");
        assert!(!plain.minify_html(), "minification must be opt-in");
        assert!(!plain.output.minify_html);
        let cfg =
            SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n[output]\nminify_html = true\n")
                .expect("parses");
        assert!(cfg.minify_html());
    }
}
