//! Generated social images (A5, ADR 0032): pure identity, naming, and
//! participation rules.
//!
//! A social image is a page-metadata projection, not an image derivative:
//!
//! ```text
//! Static        source bytes → output bytes (verbatim)
//! DerivedImage  source image + parameters → resized/encoded image (A2)
//! SocialImage   page metadata + optional hero → generated PNG card (A5)
//! ```
//!
//! Everything here is pure, deterministic, and filesystem-free: output
//! naming, the output-path → route inversion (shared by planning, input
//! derivation, resolution, and `explain`), the participation predicate, and
//! URL forms. Pixels live in `signal-cli::social`, exactly as image
//! derivative pixels live in `signal-cli::images` (ADR 0029).
//!
//! Identity: one social image per participating page, at
//! `social/<route-path>.png` (`/posts/example/` → `social/posts/example.png`,
//! `/` → `social/index.png`). Routes are unique, so paths are unique; a
//! route that would collide (`/` vs `/index/`) fails the existing
//! output-collision validation instead of silently overwriting.

use crate::config::SignalConfig;
use crate::ids::Route;
use crate::model::ContentEntry;

/// Default social-card width in pixels.
pub const DEFAULT_SOCIAL_WIDTH: u32 = 1200;
/// Default social-card height in pixels.
pub const DEFAULT_SOCIAL_HEIGHT: u32 = 630;

/// Upper bound on either social-card dimension. Configuration requests
/// larger than this are rejected: a card is a fixed-purpose artifact, and
/// unbounded dimensions would be an allocation hazard, never a feature.
pub const MAX_SOCIAL_DIMENSION: u32 = 4096;

/// Output directory for generated social images.
pub const SOCIAL_OUTPUT_DIR: &str = "social";

/// Deterministic output path for a route's social image.
///
/// Mirrors `route_to_output_path` (`/posts/a/` → `posts/a/index.html`)
/// segment for segment — raw logical segments, never URL-encoded — so a
/// route maps to exactly one output path across the whole pipeline.
pub fn social_image_path(route: &str) -> String {
    let trimmed = route.trim_matches('/');
    if trimmed.is_empty() {
        return format!("{SOCIAL_OUTPUT_DIR}/index.png");
    }
    format!("{SOCIAL_OUTPUT_DIR}/{trimmed}.png")
}

/// Invert a social-image output path back to its route.
///
/// The exact inverse of [`social_image_path`] for every path that function
/// produces: `social/index.png` → `/`, `social/posts/a.png` →
/// `/posts/a/`. Anything else (wrong directory, wrong extension, empty or
/// doubled segments) yields `None` — the single inversion shared by
/// planning, input derivation, resolution, and `explain`, so all four
/// agree on which route an output path is (the `derivative_for_output`
/// precedent).
pub fn social_image_route(path: &str) -> Option<String> {
    let rest = path.strip_prefix(SOCIAL_OUTPUT_DIR)?.strip_prefix('/')?;
    let rest = rest.strip_suffix(".png")?;
    if rest.is_empty() || rest.split('/').any(|segment| segment.is_empty()) {
        return None;
    }
    if rest == "index" {
        Some("/".to_string())
    } else {
        Some(format!("/{rest}/"))
    }
}

/// URL-path form of a route's social image (encoded, root-relative),
/// suitable for template `content` attributes.
pub fn social_image_url(route: &str) -> String {
    crate::meta::image_src_url(&format!("/{}", social_image_path(route)))
}

/// Absolute URL of a route's social image for Open Graph metadata.
///
/// Open Graph images must be publicly resolvable, so this joins the
/// configured `base_url` with the encoded output path. The base is
/// concatenated, never encoded (the `canonical_url` contract).
pub fn social_image_absolute_url(base_url: &str, route: &str) -> String {
    crate::meta::canonical_url(
        base_url,
        &Route::new(format!("/{}", social_image_path(route))),
    )
}

/// A page's declared social-image override (front matter `social_image`).
///
/// Lives in `extra` like every other site-specific key: authors write
/// `social_image: false` to opt one page out of a site-wide policy, or
/// `social_image: true` to opt in when the site has not enabled the
/// feature. Non-boolean values are ignored here and rejected by ingestion
/// (`extra` type checking is deliberately permissive; a mistyped override
/// must never change build shape silently).
pub fn social_image_override(entry: &ContentEntry) -> Option<bool> {
    entry
        .extra
        .get("social_image")
        .and_then(serde_json::Value::as_bool)
}

/// Whether one page participates in social-image generation.
///
/// Enabled site-wide (`[social]` present and not `enabled = false`), the
/// page is a real entry page (never a section root listing), and the page
/// has not opted out. Content type is not otherwise consulted.
pub fn social_image_eligible(config: &SignalConfig, entry: &ContentEntry) -> bool {
    config.social_size().is_some()
        && !entry.section_root
        && social_image_override(entry) != Some(false)
}

/// The hero source a social card will composite, if any.
///
/// Only raster sources the image pipeline already understands are
/// composited (PNG/JPEG, the `is_derivable_source` set). External heroes,
/// SVG/GIF heroes, and unset heroes yield a metadata-only card — the same
/// predicate that decides dependency edges, so what is declared and what
/// is drawn can never drift apart.
pub fn social_image_hero(entry: &ContentEntry) -> Option<String> {
    let image = entry.image.as_deref()?;
    let source = crate::asset::resolve_front_matter_image(image)?;
    crate::asset::is_derivable_source(&source).then_some(source)
}

/// One page's social image resolved against the model: the page it belongs
/// to, its route, and the hero source it composites.
///
/// The single path → page inversion (A5, ADR 0032): planning, input
/// derivation, source validation, resolution, and `explain` all resolve
/// through [`social_image_for_output`], so all five agree on which page a
/// card belongs to and what it consumes — the `derivative_for_output`
/// precedent, for the second generated artifact family.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedSocial<'a> {
    /// The page's route, e.g. `/posts/example/`.
    pub route: String,
    /// The page itself.
    pub entry: &'a ContentEntry,
    /// Composited hero source (`static/`-relative), when one is rendered.
    pub hero: Option<String>,
}

/// Resolve a social-image output path to the page that owns it.
///
/// `None` when the path is not a social-image path or no entry owns its
/// route. Pure: no filesystem access, no decoding — a hero source is
/// *named*, never read.
pub fn social_image_for_output<'a>(
    model: &'a crate::model::SiteModel,
    path: &str,
) -> Option<PlannedSocial<'a>> {
    let route = social_image_route(path)?;
    let entry = model.lookup_by_route(&Route::new(route.clone()))?;
    let hero = social_image_hero(entry);
    Some(PlannedSocial { route, entry, hero })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{CollectionId, ContentId, Slug, SourceRef};

    fn entry(route: &str) -> ContentEntry {
        entry_with(1, route)
    }

    fn entry_with(id: u32, route: &str) -> ContentEntry {
        ContentEntry::new(
            ContentId(id),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), format!("{id}.md")),
            Slug::new("a"),
            Route::new(route.to_string()),
            "Title",
        )
    }

    fn config(toml: &str) -> SignalConfig {
        SignalConfig::from_toml_str(toml).expect("config parses")
    }

    #[test]
    fn paths_round_trip_through_the_inversion() {
        for (route, path) in [
            ("/", "social/index.png"),
            ("/posts/", "social/posts.png"),
            ("/posts/example/", "social/posts/example.png"),
            ("/a/b/c/", "social/a/b/c.png"),
        ] {
            assert_eq!(social_image_path(route), path, "route {route:?}");
            assert_eq!(
                social_image_route(path),
                Some(route.to_string()),
                "path {path:?}"
            );
        }
        // Non-social paths never invert.
        for other in [
            "posts/example/index.html",
            "social/",
            "social/a.jpg",
            "images/social/a.png",
            "social/a//b.png",
            "social/.png",
        ] {
            assert_eq!(social_image_route(other), None, "path {other:?}");
        }
    }

    #[test]
    fn urls_encode_but_keep_the_base_literal() {
        assert_eq!(
            social_image_url("/posts/example/"),
            "/social/posts/example.png"
        );
        assert_eq!(social_image_url("/posts/a b/"), "/social/posts/a%20b.png");
        assert_eq!(
            social_image_absolute_url("https://example.com", "/posts/example/"),
            "https://example.com/social/posts/example.png"
        );
        assert_eq!(
            social_image_absolute_url("https://example.com/sub/", "/a b/"),
            "https://example.com/sub/social/a%20b.png"
        );
    }

    #[test]
    fn participation_follows_config_front_matter_and_content_type() {
        let plain = entry("/posts/a/");
        let section = {
            let mut e = entry("/posts/");
            e.section_root = true;
            e
        };
        // Disabled (no `[social]`) or explicitly disabled: nobody
        // participates.
        for toml in [
            "[site]\ntitle = \"T\"\n",
            "[site]\ntitle = \"T\"\n[social]\nenabled = false\n",
        ] {
            let config = config(toml);
            assert!(config.social_size().is_none(), "{toml}");
            assert!(!social_image_eligible(&config, &plain));
        }
        let enabled = config("[site]\ntitle = \"T\"\n[social]\n");
        assert_eq!(
            enabled.social_size(),
            Some((DEFAULT_SOCIAL_WIDTH, DEFAULT_SOCIAL_HEIGHT))
        );
        assert!(social_image_eligible(&enabled, &plain));
        // Section roots are listings, never article cards.
        assert!(!social_image_eligible(&enabled, &section));
        // Front matter overrides win in both directions.
        let mut opted_out = entry("/posts/b/");
        opted_out
            .extra
            .insert("social_image".to_string(), serde_json::Value::Bool(false));
        assert!(!social_image_eligible(&enabled, &opted_out));
        let mut opted_in = entry("/posts/c/");
        opted_in
            .extra
            .insert("social_image".to_string(), serde_json::Value::Bool(true));
        assert!(social_image_eligible(&enabled, &opted_in));
        let enabled = config("[site]\ntitle = \"T\"\n[social]\nenabled = true\n");
        assert_eq!(
            enabled.social_size(),
            Some((DEFAULT_SOCIAL_WIDTH, DEFAULT_SOCIAL_HEIGHT))
        );
        assert!(social_image_eligible(&enabled, &plain));
        // Dimensions fall back per field and follow configuration.
        let sized = config("[site]\ntitle = \"T\"\n[social]\nwidth = 800\n");
        assert_eq!(sized.social_size(), Some((800, DEFAULT_SOCIAL_HEIGHT)));
        let sized = config("[site]\ntitle = \"T\"\n[social]\nwidth = 800\nheight = 400\n");
        assert_eq!(sized.social_size(), Some((800, 400)));
    }

    #[test]
    fn only_raster_heroes_are_composited() {
        let mut e = entry("/posts/a/");
        assert_eq!(social_image_hero(&e), None);
        for (image, expected) in [
            ("images/hero.jpg", Some("images/hero.jpg")),
            ("/images/hero.png", Some("images/hero.png")),
            ("images/logo.svg", None),
            ("images/anim.gif", None),
            ("https://cdn.example/hero.jpg", None),
        ] {
            e.image = Some(image.to_string());
            assert_eq!(
                social_image_hero(&e).as_deref(),
                expected,
                "image {image:?}"
            );
        }
    }

    #[test]
    fn output_paths_resolve_to_their_page() {
        // The single path → page inversion shared by planning, input
        // derivation, validation, resolution, and `explain`.
        let mut model = crate::model::SiteModelBuilder::new();
        let mut hero_entry = entry_with(1, "/posts/hero/");
        hero_entry.image = Some("images/hero.png".to_string());
        model.add_entry(hero_entry);
        model.add_entry(entry_with(2, "/posts/plain/"));
        let model = model.build().expect("builds");

        let planned = social_image_for_output(&model, "social/posts/hero.png").expect("resolves");
        assert_eq!(planned.route, "/posts/hero/");
        assert_eq!(planned.entry.route.0, "/posts/hero/");
        assert_eq!(planned.hero.as_deref(), Some("images/hero.png"));
        // A page without a raster hero composites nothing.
        let plain = social_image_for_output(&model, "social/posts/plain.png").expect("resolves");
        assert_eq!(plain.hero, None);
        // Unknown paths and unknown routes resolve to nothing.
        assert!(social_image_for_output(&model, "social/posts/nope.png").is_none());
        assert!(social_image_for_output(&model, "posts/hero/index.html").is_none());
        assert!(social_image_for_output(&model, "images/hero.png").is_none());
    }
}
