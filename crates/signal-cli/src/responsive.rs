//! Responsive image rendering (A3, ADR 0030; `<picture>` since A4,
//! ADR 0031): body-image rewriting plus the front-matter hero context
//! value.
//!
//! The planner decides which derivative artifacts exist; this module
//! expresses them as HTML. Selection (dedupe, default, `srcset`, format
//! grouping) lives in `signal-core::asset` and is shared with `explain`;
//! here it is applied to Comrak-rendered body markup and hero metadata:
//!
//! ```text
//! single format:
//! <img src="/images/hero.png" alt="Peak" />
//!   → <img src="/images/hero-1280.webp"
//!          srcset="/images/hero-640.webp 640w, …"
//!          sizes="100vw" width="1280" height="720" alt="Peak" />
//!
//! multiple formats:
//! <img src="/images/hero.png" alt="Peak" />
//!   → <picture><source type="image/avif" srcset="…avif 640w, …" />
//!       <source type="image/webp" srcset="…webp 640w, …" />
//!       <img src="…webp" srcset="…webp…" sizes="100vw"
//!            width="…" height="…" alt="Peak" /></picture>
//! ```
//!
//! Tags that resolve externally, unresolvably, to non-raster sources, or
//! while no derivatives are configured are returned byte-identical —
//! unconfigured sites render exactly A1 markup. Callers run
//! post-validation, so referenced sources exist and decode; any failure
//! here is a hard error, never a silent fallback.

use std::path::Path;

use signal_core::SignalConfig;

use crate::body_html::{find_img, parse_attributes, scan_tag};

/// Front-matter hero as templates consume it (A3, ADR 0030; format
/// groups since A4, ADR 0031): flat fallback presentation fields plus
/// the per-format source list templates render `<picture>` from.
/// Inserted under `responsive_image` only when the hero has planned
/// derivatives; templates gate with `{% if responsive_image %}` and
/// branch on `has_picture`:
///
/// ```text
/// {% if responsive_image.has_picture %}<picture>
/// {% for source in responsive_image.sources %}
/// <source type="{{ source.mime }}" srcset="{{ source.srcset }}" />
/// {% endfor %}…{% endif %}
/// ```
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResponsiveHero {
    /// Fallback `src`: largest available fallback derivative, URL-path form.
    pub src: String,
    /// Prebuilt fallback `srcset` attribute (actual widths, never requested).
    pub srcset: String,
    /// `sizes` value (`signal_core::DEFAULT_SIZES` in A3).
    pub sizes: String,
    /// Intrinsic width of the fallback.
    pub width: u32,
    /// Intrinsic height of the fallback.
    pub height: u32,
    /// Hero alternative text (`image_alt`, empty when unset).
    pub alt: String,
    /// Fallback `srcset` candidates, actual-width ascending.
    pub candidates: Vec<ResponsiveHeroCandidate>,
    /// One group per planned format, in `<source>` order (AVIF first).
    pub sources: Vec<signal_core::ResponsiveSource>,
    /// Whether more than one format is planned (render `<picture>`).
    pub has_picture: bool,
}

/// One responsive candidate for templates.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ResponsiveHeroCandidate {
    /// Candidate URL in URL-path form.
    pub url: String,
    /// Intrinsic width in pixels.
    pub width: u32,
    /// Intrinsic height in pixels.
    pub height: u32,
}

/// Build the hero value for a normalized front-matter image reference.
/// `None` (no context key) for external heroes, non-raster heroes, and
/// unconfigured builds — the existing `image`/`image_alt` keys carry
/// those cases unchanged.
pub fn responsive_hero(
    root: &Path,
    config: &SignalConfig,
    image: &str,
    alt: Option<&str>,
) -> Result<Option<ResponsiveHero>, crate::errors::BuildError> {
    let Some(source) = signal_core::resolve_front_matter_image(image) else {
        return Ok(None);
    };
    let Some(responsive) = crate::images::responsive_for_source(root, config, &source)? else {
        return Ok(None);
    };
    Ok(Some(ResponsiveHero {
        src: responsive.default_src.clone(),
        srcset: responsive.srcset.clone(),
        sizes: responsive.sizes.clone(),
        width: responsive.width,
        height: responsive.height,
        alt: alt.unwrap_or_default().to_string(),
        candidates: responsive
            .candidates
            .iter()
            .map(|candidate| ResponsiveHeroCandidate {
                url: candidate.url.clone(),
                width: candidate.width,
                height: candidate.height,
            })
            .collect(),
        sources: responsive.sources.clone(),
        has_picture: responsive.has_picture(),
    }))
}

/// Rewrite body HTML `<img>` tags to responsive markup.
///
/// Every tag whose `src` resolves (against `entry_route`) to a derivable
/// source with planned derivatives is replaced; all other tags —
/// external, unresolvable, non-raster, unconfigured — pass through
/// byte-identical. Attribute scanning is quote-aware; surviving `alt`
/// values and any further attributes (e.g. Comrak `title`s) are carried
/// over verbatim, never re-escaped.
pub fn rewrite_body_images(
    html: &str,
    entry_route: &str,
    config: &SignalConfig,
    root: &Path,
) -> Result<String, crate::errors::BuildError> {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(start) = find_img(bytes, cursor) {
        out.push_str(&html[cursor..start]);
        let (_, end) = scan_tag(bytes, start);
        match rewrite_tag(&html[start..end], entry_route, config, root)? {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(&html[start..end]),
        }
        cursor = end;
    }
    out.push_str(&html[cursor..]);
    Ok(out)
}

/// Rewrite one `<img>` tag, or `None` to leave it byte-identical.
fn rewrite_tag(
    tag: &str,
    entry_route: &str,
    config: &SignalConfig,
    root: &Path,
) -> Result<Option<String>, crate::errors::BuildError> {
    let attributes = parse_attributes(tag);
    let Some(src) = attributes
        .iter()
        .find(|(name, _)| name == "src")
        .map(|(_, value)| value.clone())
    else {
        return Ok(None);
    };
    if src.trim().is_empty() || signal_core::is_external_reference(&src) {
        return Ok(None);
    }
    let Some(source) = signal_core::resolve_body_image(entry_route, &src) else {
        return Ok(None);
    };
    let Some(responsive) = crate::images::responsive_for_source(root, config, &source)? else {
        return Ok(None);
    };
    let alt = attributes
        .iter()
        .find(|(name, _)| name == "alt")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let mut img = format!(
        "<img src=\"{}\" srcset=\"{}\" sizes=\"{}\" width=\"{}\" height=\"{}\" alt=\"{}\"",
        responsive.default_src,
        responsive.srcset,
        responsive.sizes,
        responsive.width,
        responsive.height,
        alt,
    );
    for (name, value) in &attributes {
        if name == "src"
            || name == "srcset"
            || name == "sizes"
            || name == "width"
            || name == "height"
            || name == "alt"
        {
            continue;
        }
        img.push_str(&format!(" {name}=\"{value}\""));
    }
    img.push_str(" />");
    // Single planned format: the A3 responsive `<img>`. Multiple formats:
    // `<picture>` with one `<source>` per group (AVIF first — browsers
    // take the first supported source) and the fallback `<img>` carrying
    // `alt` and any carried-over attributes (only `<img>` may carry them).
    if !responsive.has_picture() {
        return Ok(Some(img));
    }
    let mut picture = String::from("<picture>");
    for group in &responsive.sources {
        picture.push_str(&format!(
            "<source type=\"{}\" srcset=\"{}\" />",
            group.mime, group.srcset
        ));
    }
    picture.push_str(&img);
    picture.push_str("</picture>");
    Ok(Some(picture))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SignalConfig {
        SignalConfig::from_toml_str(
            "[site]\ntitle = \"T\"\n[images]\nwidths = [80, 320]\nformat = \"webp\"\n",
        )
        .expect("config parses")
    }

    fn test_root() -> tempfile::TempDir {
        use image::ImageEncoder as _;
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("static/images")).expect("mkdir");
        let mut image = image::RgbImage::new(160, 90);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, 128];
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 160, 90, image::ExtendedColorType::Rgb8)
            .expect("fixture encodes");
        std::fs::write(dir.path().join("static/images/hero.png"), &bytes).expect("write");
        std::fs::write(dir.path().join("static/images/logo.svg"), "<svg></svg>").expect("write");
        dir
    }

    #[test]
    fn rewrites_resolvable_raster_images() {
        let dir = test_root();
        let root = dir.path();
        let config = test_config();
        let html = rewrite_body_images(
            "<p>Text <img src=\"/images/hero.png\" alt=\"Peak\" /> more.</p>",
            "/posts/a/",
            &config,
            root,
        )
        .expect("rewrites");
        assert_eq!(
            html,
            "<p>Text <img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Peak\" /> more.</p>"
        );
    }

    #[test]
    fn leaves_everything_else_byte_identical() {
        let dir = test_root();
        let root = dir.path();
        let config = test_config();
        // External, unresolvable, non-raster, and missing-src tags pass
        // through untouched — as does every tag when unconfigured.
        for tag in [
            "<p><img src=\"https://example.com/a.png\" alt=\"x\" /></p>",
            "<p><img src=\"../../../evil.png\" alt=\"x\" /></p>",
            "<p><img src=\"/images/logo.svg\" alt=\"x\" /></p>",
            "<p><img alt=\"no src\" /></p>",
            "<p>No images here.</p>",
        ] {
            let html =
                rewrite_body_images(tag, "/posts/a/", &config, root).expect("passes through");
            assert_eq!(html, tag, "must pass through: {tag}");
        }
        let plain: SignalConfig =
            SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n").expect("parses");
        let tag = "<p><img src=\"/images/hero.png\" alt=\"x\" /></p>";
        assert_eq!(
            rewrite_body_images(tag, "/posts/a/", &plain, root).expect("passes"),
            tag
        );
    }

    #[test]
    fn preserves_titles_and_relative_references() {
        let dir = test_root();
        let root = dir.path();
        let config = test_config();
        // Document-relative references resolve against the entry route
        // (`../../images/hero.png` from `/posts/a/` names
        // `/images/hero.png`); Comrak titles survive verbatim after the
        // rewritten attributes.
        let html = rewrite_body_images(
            "<p><img src=\"../../images/hero.png\" alt=\"A\" title=\"T\" /></p>",
            "/posts/a/",
            &config,
            root,
        )
        .expect("rewrites");
        assert!(
            html.contains("src=\"/images/hero-320.webp\""),
            "got: {html}"
        );
        assert!(html.contains("title=\"T\""), "got: {html}");
    }

    fn multi_format_config() -> SignalConfig {
        SignalConfig::from_toml_str(
            "[site]\ntitle = \"T\"\n[images]\nwidths = [80, 320]\nformats = [\"webp\", \"avif\"]\n",
        )
        .expect("config parses")
    }

    #[test]
    fn multi_format_body_images_render_picture_with_avif_first() {
        let dir = test_root();
        let root = dir.path();
        let config = multi_format_config();
        let html = rewrite_body_images(
            "<p>Text <img src=\"/images/hero.png\" alt=\"Peak\" title=\"T\" /> more.</p>",
            "/posts/a/",
            &config,
            root,
        )
        .expect("rewrites");
        // Clamped 320w request advertises actual 160w in both groups; the
        // fallback `<img>` is WebP and carries `alt` plus the title.
        assert_eq!(
            html,
            "<p>Text <picture><source type=\"image/avif\" srcset=\"/images/hero-80.avif 80w, /images/hero-320.avif 160w\" /><source type=\"image/webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" /><img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Peak\" title=\"T\" /></picture> more.</p>"
        );
    }

    #[test]
    fn single_format_config_keeps_plain_responsive_img() {
        let dir = test_root();
        let root = dir.path();
        // AVIF-only: one group, no `<picture>` — the A3 shape with an AVIF
        // fallback.
        let config: SignalConfig = SignalConfig::from_toml_str(
            "[site]\ntitle = \"T\"\n[images]\nwidths = [80]\nformat = \"avif\"\n",
        )
        .expect("parses");
        let html = rewrite_body_images(
            "<p><img src=\"/images/hero.png\" alt=\"A\" /></p>",
            "/posts/a/",
            &config,
            root,
        )
        .expect("rewrites");
        assert_eq!(
            html,
            "<p><img src=\"/images/hero-80.avif\" srcset=\"/images/hero-80.avif 80w\" sizes=\"100vw\" width=\"80\" height=\"45\" alt=\"A\" /></p>"
        );
    }

    #[test]
    fn multi_format_hero_exposes_sources_and_picture_flag() {
        let dir = test_root();
        let root = dir.path();
        let config = multi_format_config();
        let hero = responsive_hero(root, &config, "/images/hero.png", Some("Peak"))
            .expect("builds")
            .expect("present");
        assert!(hero.has_picture);
        assert_eq!(hero.src, "/images/hero-320.webp");
        assert_eq!(hero.sources.len(), 2);
        assert_eq!(hero.sources[0].mime, "image/avif");
        assert_eq!(
            hero.sources[0].srcset,
            "/images/hero-80.avif 80w, /images/hero-320.avif 160w"
        );
        assert_eq!(hero.sources[1].mime, "image/webp");
        // WebP-only heroes carry one group and no picture flag.
        let single = responsive_hero(root, &test_config(), "/images/hero.png", None)
            .expect("builds")
            .expect("present");
        assert!(!single.has_picture);
        assert_eq!(single.sources.len(), 1);
    }

    #[test]
    fn hero_value_carries_flat_fields_and_candidates() {
        let dir = test_root();
        let root = dir.path();
        let config = test_config();
        let hero = responsive_hero(root, &config, "/images/hero.png", Some("Peak"))
            .expect("builds")
            .expect("present");
        assert_eq!(hero.src, "/images/hero-320.webp");
        assert_eq!(
            hero.srcset,
            "/images/hero-80.webp 80w, /images/hero-320.webp 160w"
        );
        assert_eq!(hero.sizes, "100vw");
        assert_eq!((hero.width, hero.height), (160, 90));
        assert_eq!(hero.alt, "Peak");
        assert_eq!(hero.candidates.len(), 2);
        // SVG heroes and missing alt text degrade gracefully.
        let svg = responsive_hero(root, &config, "/images/logo.svg", None).expect("builds");
        assert!(svg.is_none());
        let no_alt = responsive_hero(root, &config, "/images/hero.png", None)
            .expect("builds")
            .expect("present");
        assert_eq!(no_alt.alt, "");
    }
}
