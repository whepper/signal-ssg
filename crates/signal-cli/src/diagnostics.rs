//! Asset and image diagnostics (A6): evidence-based observations over the
//! publishing model.
//!
//! A6 adds no artifact, no dependency edge, and no manifest field. This
//! module reads what the publishing pipeline already knows — the planned
//! specs, the frozen model, the configuration, and source bytes — and
//! derives a small closed set of [`Diagnostic`] values. `signal check`
//! prints them; `signal explain` prints the ones about the explained
//! subject. Both call [`analyze`], so they can never disagree.
//!
//! ```text
//! planned specs + frozen model + config + source bytes
//!                        │
//!                        ▼
//!                   analyze()          (this module: pure + header probes)
//!                        │
//!            ┌───────────┴───────────┐
//!            ▼                       ▼
//!      check presentation      explain presentation
//! ```
//!
//! Diagnostics are **advisory**: a diagnostic never fails a build, never
//! becomes an artifact, and never enters the manifest. Hard failures stay
//! [`BuildError`](crate::errors::BuildError), unchanged: a missing or unsafe
//! reference, a malformed image, an output collision, or an invalid
//! configuration all fail before diagnostics are computed. For that reason
//! the severity model is deliberately two-valued — [`Severity::Warning`] and
//! [`Severity::Info`] — with no `error` level: there is nothing left to
//! report as an error by the time this module runs.
//!
//! Every diagnostic states a measured fact ("source is 4000×3000; the
//! largest generated representation is 1200px wide"), never advice ("shrink
//! the image"). Conditions that Signal cannot establish are not reported;
//! see the module-level rule on each [`Diagnostic`] variant.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use signal_core::{ArtifactSpec, DerivativeFormat, DerivativeSpec, SignalConfig, SiteModel};

/// How serious a diagnostic is. Advisory only — see the module docs for why
/// there is no `error` level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// An actionable inefficiency or content gap. Never a build failure.
    Warning,
    /// An observation that may be entirely intentional.
    Info,
}

impl Severity {
    /// Stable lowercase label for rendering.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// How much larger a decoded source must be than its largest published
/// representation before [`Diagnostic::OversizedSource`] reports it.
///
/// `3` tolerates the common 2×-DPR source (good practice, not a problem)
/// while catching sources whose resolution is far beyond anything Signal
/// publishes: at 3×, the source carries at least 9× the pixel area of the
/// largest derivative. The factor is a documented build constant, not a
/// user knob.
pub const OVERSIZED_FACTOR: u32 = 3;

/// One evidence-based observation about the publishing model.
///
/// Each variant documents the exact condition it requires, so a reader can
/// see what evidence produced it. `subject` (see [`Diagnostic::subject`])
/// names what the diagnostic is about: an asset path for asset/image
/// diagnostics, a route for page-content diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    /// A raster source asset that no content entry references.
    ///
    /// Condition: a planned `ArtifactKind::Static` spec whose path is a
    /// derivable raster source (`png`/`jpg`/`jpeg`) and which no entry
    /// references. Non-raster assets (CSS, JS, SVG, favicons) are *not*
    /// reported: they are commonly referenced from templates, which the
    /// model does not track, so reporting them would be a false positive.
    /// Generated artifacts are never reported (only `Static` specs are
    /// considered).
    UnreferencedAsset {
        /// `static/`-relative asset path.
        path: String,
    },
    /// A raster source whose decoded width far exceeds the largest actual
    /// representation Signal generates from it.
    ///
    /// Condition: the source has planned derivatives, its decoded width is
    /// at least [`OVERSIZED_FACTOR`] times the largest *actual* candidate
    /// width. Requires a configured `[images]` pipeline: with no derivatives
    /// Signal has no publishing requirement to compare against.
    OversizedSource {
        /// `static/`-relative source path.
        source: String,
        /// Decoded source width.
        source_width: u32,
        /// Decoded source height.
        source_height: u32,
        /// Largest actual width among the source's generated candidates.
        largest_width: u32,
    },
    /// Two or more requested derivative widths render at the same actual
    /// width because the source is smaller than the request (clamping).
    ///
    /// Condition: the source has planned derivatives and at least two
    /// configured widths clamp to one actual width, so those widths produce
    /// byte-identical derivative artifacts. Configuration is user intent, so
    /// this is informational, not a warning.
    RedundantDerivativeWidth {
        /// `static/`-relative source path.
        source: String,
        /// The requested widths that collapse to one actual width, sorted.
        widths: Vec<u32>,
        /// The actual width they all render at.
        actual_width: u32,
    },
    /// A page declares a front-matter hero image but no `image_alt`.
    ///
    /// Condition: `entry.image` is a non-empty reference and `entry.image_alt`
    /// is absent or blank. The hero is a first-class content field with an
    /// explicit alternative-text field, so its absence is a concrete gap.
    HeroAltMissing {
        /// The page's route.
        route: String,
        /// The declared hero reference.
        image: String,
    },
    /// A rendered body image whose `alt` is empty or whitespace-only.
    ///
    /// Condition: an `<img>` in `entry.body.html` whose `alt` attribute is
    /// blank. Signal's Markdown model cannot distinguish an omitted alt from
    /// an explicitly empty one — Comrak renders both as `alt=""` — so this
    /// is informational: an empty alt is the correct, intentional marker for
    /// a decorative image.
    ImageAltEmpty {
        /// The page's route.
        route: String,
        /// The image `src` as authored.
        source: String,
    },
}

impl Diagnostic {
    /// Stable machine-readable identifier, for scripts and tests.
    pub fn code(&self) -> &'static str {
        match self {
            Diagnostic::UnreferencedAsset { .. } => "unreferenced-asset",
            Diagnostic::OversizedSource { .. } => "oversized-source",
            Diagnostic::RedundantDerivativeWidth { .. } => "redundant-derivative-width",
            Diagnostic::HeroAltMissing { .. } => "hero-alt-missing",
            Diagnostic::ImageAltEmpty { .. } => "image-alt-empty",
        }
    }

    /// Advisory severity. Never an error — hard failures stay `BuildError`.
    pub fn severity(&self) -> Severity {
        match self {
            Diagnostic::UnreferencedAsset { .. } => Severity::Info,
            Diagnostic::OversizedSource { .. } => Severity::Warning,
            Diagnostic::RedundantDerivativeWidth { .. } => Severity::Info,
            Diagnostic::HeroAltMissing { .. } => Severity::Warning,
            Diagnostic::ImageAltEmpty { .. } => Severity::Info,
        }
    }

    /// What the diagnostic is about: an asset path or a route.
    pub fn subject(&self) -> &str {
        match self {
            Diagnostic::UnreferencedAsset { path } => path,
            Diagnostic::OversizedSource { source, .. } => source,
            Diagnostic::RedundantDerivativeWidth { source, .. } => source,
            Diagnostic::HeroAltMissing { route, .. } => route,
            Diagnostic::ImageAltEmpty { route, .. } => route,
        }
    }

    /// The measured fact, with no advice attached.
    pub fn message(&self) -> String {
        match self {
            Diagnostic::UnreferencedAsset { .. } => {
                "no content entry references this asset".to_string()
            }
            Diagnostic::OversizedSource {
                source_width,
                source_height,
                largest_width,
                ..
            } => format!(
                "source is {source_width}×{source_height}; the largest generated representation is {largest_width}px wide"
            ),
            Diagnostic::RedundantDerivativeWidth {
                widths,
                actual_width,
                ..
            } => format!(
                "requested widths {} render at the same {actual_width}px",
                join_widths(widths)
            ),
            Diagnostic::HeroAltMissing { image, .. } => {
                format!("hero image {image} has no image_alt")
            }
            Diagnostic::ImageAltEmpty { source, .. } => {
                format!("image {source} has empty alt text")
            }
        }
    }
}

/// Human-readable width list: `640`, `640 and 800`, `640, 800 and 1280`.
fn join_widths(widths: &[u32]) -> String {
    let parts: Vec<String> = widths.iter().map(|width| width.to_string()).collect();
    match parts.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Analyze the publishing model and return every diagnostic, sorted
/// deterministically by `(severity, subject, code)`.
///
/// Infallible and best-effort by design: a source that cannot be read or
/// probed is skipped, because hard failures are already reported by
/// validation (which runs before this on every command that reaches it).
/// Reads only image headers for raster sources with planned derivatives.
pub fn analyze(
    root: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    specs: &[ArtifactSpec],
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for path in crate::assets::unreferenced_source_assets(specs, model) {
        if signal_core::is_derivable_source(&path) {
            out.push(Diagnostic::UnreferencedAsset { path });
        }
    }
    out.extend(derivative_diagnostics(root, config, model));
    out.extend(alt_diagnostics(model));
    out.sort_by(|a, b| {
        a.severity()
            .cmp(&b.severity())
            .then_with(|| a.subject().cmp(b.subject()))
            .then_with(|| a.code().cmp(b.code()))
    });
    out
}

/// Diagnostics for one subject (asset path), for `explain`.
pub fn for_subject(diagnostics: &[Diagnostic], subject: &str) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.subject() == subject)
        .cloned()
        .collect()
}

/// How many diagnostics carry one severity.
pub fn count(diagnostics: &[Diagnostic], severity: Severity) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity() == severity)
        .count()
}

/// `0 warnings` / `1 warning` / `2 warnings`, and `0 info` / `1 info` / `2 info`.
pub fn label(count: usize, severity: Severity) -> String {
    let noun = severity.as_str();
    if severity == Severity::Info || count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Render diagnostics as deterministic indented lines (check presentation):
/// one `severity: message` line per diagnostic with its subject beneath.
pub fn render(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str(&format!(
            "  {}: {}\n    {}\n",
            diagnostic.severity().as_str(),
            diagnostic.message(),
            diagnostic.subject(),
        ));
    }
    out
}

/// Oversized sources and redundant requested widths, grouped by source.
///
/// Uses the same planned-derivative enumeration and the same
/// `DerivativeSpec::target_dimensions` clamp the planner and renderer use —
/// no second image model.
fn derivative_diagnostics(
    root: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Vec<Diagnostic> {
    let Ok(planned) = crate::images::planned_derivatives(config, model) else {
        return Vec::new();
    };
    let mut widths_by_source: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for spec in &planned {
        widths_by_source
            .entry(spec.source.clone())
            .or_default()
            .insert(spec.width);
    }
    let mut out = Vec::new();
    for (source, requested) in widths_by_source {
        let Ok((bytes, _)) = crate::images::read_source_bytes(root, &source) else {
            continue;
        };
        let Ok((source_width, source_height, _)) = crate::images::probe_dimensions(&bytes) else {
            continue;
        };
        let mut actual_by_width: BTreeMap<u32, u32> = BTreeMap::new();
        for width in &requested {
            let Ok(spec) = DerivativeSpec::new(source.clone(), *width, DerivativeFormat::WebP)
            else {
                continue;
            };
            let (actual, _) = spec.target_dimensions(source_width, source_height);
            actual_by_width.insert(*width, actual);
        }
        if actual_by_width.is_empty() {
            continue;
        }
        let largest_width = actual_by_width
            .values()
            .copied()
            .max()
            .unwrap_or(source_width);
        if source_width >= largest_width.saturating_mul(OVERSIZED_FACTOR) {
            out.push(Diagnostic::OversizedSource {
                source: source.clone(),
                source_width,
                source_height,
                largest_width,
            });
        }
        let mut widths_by_actual: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (width, actual) in &actual_by_width {
            widths_by_actual.entry(*actual).or_default().push(*width);
        }
        for (actual_width, widths) in widths_by_actual {
            if widths.len() > 1 {
                out.push(Diagnostic::RedundantDerivativeWidth {
                    source: source.clone(),
                    widths,
                    actual_width,
                });
            }
        }
    }
    out
}

/// Hero and body-image alternative-text diagnostics.
fn alt_diagnostics(model: &SiteModel) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for entry in model.entries() {
        if let Some(image) = entry.image.as_deref() {
            let declared = !image.trim().is_empty();
            let described = entry
                .image_alt
                .as_deref()
                .is_some_and(|alt| !alt.trim().is_empty());
            if declared && !described {
                out.push(Diagnostic::HeroAltMissing {
                    route: entry.route.0.clone(),
                    image: image.to_string(),
                });
            }
        }
        for image in crate::body_html::images(&entry.body.html) {
            let blank = image.alt.as_deref().is_none_or(|alt| alt.trim().is_empty());
            if blank {
                out.push(Diagnostic::ImageAltEmpty {
                    route: entry.route.0.clone(),
                    source: image.src,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{ArtifactKind, CollectionId, ContentId, Route, Slug, SourceRef};

    fn entry(id: u32, route: &str) -> signal_core::ContentEntry {
        signal_core::ContentEntry::new(
            ContentId(id),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), format!("e{id}.md")),
            Slug::new(format!("e{id}")),
            Route::new(route.to_string()),
            format!("Title {id}"),
        )
    }

    fn config() -> SignalConfig {
        SignalConfig::from_toml_str(
            "[site]\ntitle = \"T\"\n[images]\nwidths = [640, 1280]\nformat = \"webp\"\n",
        )
        .expect("parses")
    }

    fn spec(path: &str, kind: ArtifactKind) -> ArtifactSpec {
        ArtifactSpec::new(path, kind)
    }

    #[test]
    fn severity_and_code_are_stable() {
        let cases: Vec<(Diagnostic, Severity, &str)> = vec![
            (
                Diagnostic::UnreferencedAsset {
                    path: "images/a.png".to_string(),
                },
                Severity::Info,
                "unreferenced-asset",
            ),
            (
                Diagnostic::OversizedSource {
                    source: "images/a.png".to_string(),
                    source_width: 4000,
                    source_height: 3000,
                    largest_width: 1280,
                },
                Severity::Warning,
                "oversized-source",
            ),
            (
                Diagnostic::RedundantDerivativeWidth {
                    source: "images/a.png".to_string(),
                    widths: vec![640, 1280],
                    actual_width: 400,
                },
                Severity::Info,
                "redundant-derivative-width",
            ),
            (
                Diagnostic::HeroAltMissing {
                    route: "/posts/a/".to_string(),
                    image: "images/a.png".to_string(),
                },
                Severity::Warning,
                "hero-alt-missing",
            ),
            (
                Diagnostic::ImageAltEmpty {
                    route: "/posts/a/".to_string(),
                    source: "/images/a.png".to_string(),
                },
                Severity::Info,
                "image-alt-empty",
            ),
        ];
        for (diagnostic, severity, code) in cases {
            assert_eq!(diagnostic.severity(), severity, "{diagnostic:?}");
            assert_eq!(diagnostic.code(), code, "{diagnostic:?}");
            assert!(!diagnostic.message().is_empty());
            assert!(!diagnostic.subject().is_empty());
        }
    }

    #[test]
    fn unreferenced_assets_are_only_raster_sources() {
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut used = entry(1, "/posts/a/");
        used.body.images = vec!["/images/used.png".to_string()];
        builder.add_entry(used);
        let model = builder.build().expect("builds");
        let specs = vec![
            spec("images/used.png", ArtifactKind::Static),
            spec("images/orphan.jpg", ArtifactKind::Static),
            spec("css/site.css", ArtifactKind::Static),
            spec("js/app.js", ArtifactKind::Static),
            spec("favicon.svg", ArtifactKind::Static),
        ];
        let dir = tempfile::tempdir().expect("tempdir");
        let diagnostics = analyze(dir.path(), &config(), &model, &specs);
        let unreferenced: Vec<&str> = diagnostics
            .iter()
            .filter(|d| d.code() == "unreferenced-asset")
            .map(Diagnostic::subject)
            .collect();
        // Only the orphaned raster source is reported; template-referenced
        // CSS/JS/SVG are not (the model does not track template references).
        assert_eq!(unreferenced, vec!["images/orphan.jpg"]);
    }

    #[test]
    fn hero_and_body_alt_conditions() {
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut described = entry(1, "/posts/described/");
        described.image = Some("images/hero.png".to_string());
        described.image_alt = Some("A hero".to_string());
        described.body.html = "<p><img src=\"/images/a.png\" alt=\"A\" /></p>".to_string();
        let mut missing = entry(2, "/posts/missing/");
        missing.image = Some("images/hero.png".to_string());
        missing.body.html = "<p><img src=\"/images/b.png\" alt=\"\" /></p>".to_string();
        builder.add_entry(described);
        builder.add_entry(missing);
        let model = builder.build().expect("builds");

        let dir = tempfile::tempdir().expect("tempdir");
        let diagnostics = analyze(dir.path(), &config(), &model, &[]);
        let alt: Vec<(&str, &str, &str)> = diagnostics
            .iter()
            .filter(|d| d.code().ends_with("alt-missing") || d.code() == "image-alt-empty")
            .map(|d| (d.code(), d.subject(), d.severity().as_str()))
            .collect();
        assert_eq!(
            alt,
            vec![
                ("hero-alt-missing", "/posts/missing/", "warning"),
                ("image-alt-empty", "/posts/missing/", "info"),
            ]
        );
    }

    #[test]
    fn empty_alt_is_informational_never_a_warning() {
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut decorative = entry(1, "/posts/decorative/");
        decorative.body.html = "<img src=\"/images/rule.png\" alt=\"\" />".to_string();
        builder.add_entry(decorative);
        let model = builder.build().expect("builds");
        let dir = tempfile::tempdir().expect("tempdir");
        let diagnostics = analyze(dir.path(), &config(), &model, &[]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity(), Severity::Info);
    }

    #[test]
    fn oversized_and_redundant_use_the_planner_clamp() {
        // 4000px source, widths [640, 1280] → largest actual 1280 → 4000 ≥
        // 3 × 1280 → oversized.
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut with_image = entry(1, "/posts/a/");
        with_image.body.images = vec!["/images/hero.png".to_string()];
        builder.add_entry(with_image);
        let model = builder.build().expect("builds");
        let specs = vec![spec("images/hero.png", ArtifactKind::Static)];
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("static/images")).expect("mkdir");
        std::fs::write(
            dir.path().join("static/images/hero.png"),
            gradient_png(4000, 3000),
        )
        .expect("write");
        let diagnostics = analyze(dir.path(), &config(), &model, &specs);
        let oversized: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code() == "oversized-source")
            .collect();
        assert_eq!(oversized.len(), 1, "{diagnostics:?}");
        assert_eq!(oversized[0].subject(), "images/hero.png");
        assert!(oversized[0].message().contains("4000×3000"));

        // A source smaller than the requested widths: both clamp to the
        // source width → redundant, and no oversized report.
        std::fs::write(
            dir.path().join("static/images/hero.png"),
            gradient_png(400, 300),
        )
        .expect("write");
        let diagnostics = analyze(dir.path(), &config(), &model, &specs);
        assert!(diagnostics.iter().all(|d| d.code() != "oversized-source"));
        let redundant: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code() == "redundant-derivative-width")
            .collect();
        assert_eq!(redundant.len(), 1, "{diagnostics:?}");
        assert!(redundant[0].message().contains("400px"));
    }

    #[test]
    fn no_derivative_configuration_reports_no_image_diagnostics() {
        let mut builder = signal_core::SiteModelBuilder::new();
        let mut with_image = entry(1, "/posts/a/");
        with_image.body.images = vec!["/images/hero.png".to_string()];
        builder.add_entry(with_image);
        let model = builder.build().expect("builds");
        let specs = vec![spec("images/hero.png", ArtifactKind::Static)];
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("static/images")).expect("mkdir");
        std::fs::write(
            dir.path().join("static/images/hero.png"),
            gradient_png(4000, 3000),
        )
        .expect("write");
        let plain = SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n").expect("parses");
        let diagnostics = analyze(dir.path(), &plain, &model, &specs);
        assert!(
            diagnostics.iter().all(|d| d.code() != "oversized-source"
                && d.code() != "redundant-derivative-width"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn analysis_is_deterministically_ordered() {
        let mut builder = signal_core::SiteModelBuilder::new();
        for (id, route) in [(1, "/posts/a/"), (2, "/posts/b/"), (3, "/posts/c/")] {
            let mut e = entry(id, route);
            e.body.html = "<img src=\"/images/x.png\" alt=\"\" />".to_string();
            builder.add_entry(e);
        }
        let model = builder.build().expect("builds");
        let specs = vec![
            spec("images/one.png", ArtifactKind::Static),
            spec("images/two.jpg", ArtifactKind::Static),
        ];
        let dir = tempfile::tempdir().expect("tempdir");
        let first = analyze(dir.path(), &config(), &model, &specs);
        let second = analyze(dir.path(), &config(), &model, &specs);
        assert_eq!(first, second);
        // Every diagnostic here is info, so ordering is by subject alone.
        let subjects: Vec<&str> = first.iter().map(Diagnostic::subject).collect();
        let mut sorted = subjects.clone();
        sorted.sort_unstable();
        assert_eq!(subjects, sorted);
        // Severity dominates the sort: a warning always precedes an info.
        let severities: Vec<Severity> = first.iter().map(Diagnostic::severity).collect();
        let mut sorted_severities = severities.clone();
        sorted_severities.sort();
        assert_eq!(severities, sorted_severities);
    }

    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        use image::ImageEncoder as _;
        let mut image = image::RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, 128];
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .expect("fixture encodes");
        bytes
    }
}
