//! First-class asset inventory (A1): discovery, validation counts, and
//! filesystem metadata for reporting.
//!
//! Vocabulary (see `signal_core::asset` and `signal_core::social`):
//!
//! ```text
//! source asset     a file under `static/` (enumerated as an
//!                  `ArtifactKind::Static` spec)
//! asset reference  an authored string in content (`body.images`,
//!                  front-matter `image`) resolving to a source path
//! output asset     the resolved bytes written through `write_artifact`
//! ```
//!
//! Generated artifacts are reported here but are not assets: image
//! derivatives (`ArtifactKind::DerivedImage`, A2) and social images
//! (`ArtifactKind::SocialImage`, A5) are counted from the plan and
//! explained through their own records, because their identity is a
//! transformation or a page, not a source path.
//!
//! Assets stay inside the existing build model: discovery reuses the planned
//! `Static` specs (no second walker), dependencies reuse `InputRef::Static`
//! (no parallel edge type), and output flows through `resolve_artifact` /
//! `write_artifact` (no independent copy). This module only derives views
//! over that model: pure count reports for `check`, and measured metadata
//! (size, hash) plus reverse references for `explain`.
//!
//! Reference resolution itself lives in `signal_core::asset` so planning
//! (`build_plan::artifact_inputs`) and validation (`link_check`) resolve
//! identically. Matching against the spec inventory tries the raw authored
//! form first, then the percent-decoded form — the same rule `link_check`
//! uses — so encoded references (`a%20b.svg`) find literal files
//! (`a b.svg`) without inventing normalization.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use signal_core::{entry_asset_paths, percent_decode, ArtifactSpec, SignalConfig, SiteModel};

use crate::errors::BuildError;
use crate::manifest::{Digest, InputRef};

/// Asset counts for `signal check`. All fields are pure derivations over
/// the planned specs plus the frozen model — no filesystem reads — so
/// `check` reports from exactly what it validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetReport {
    /// Source assets on disk (planned `Static` specs).
    pub discovered: usize,
    /// Distinct internal asset paths referenced by content.
    pub referenced: usize,
    /// Referenced paths that resolve to a planned source asset.
    pub resolved: usize,
    /// Referenced paths with no matching source asset (0 on success;
    /// reference validation fails the build before this reports).
    pub missing: usize,
    /// Internal references that escape the site root (0 on success).
    pub unsafe_paths: usize,
    /// Source assets no entry references (informational; future A5 input).
    pub unreferenced: usize,
    /// Planned image derivatives (A2 `DerivedImage` specs).
    pub derivatives: usize,
    /// Planned social cards (A5 `SocialImage` specs).
    pub social_images: usize,
}

/// Build the pure asset report from planned specs and the frozen model.
pub fn build_asset_report(specs: &[ArtifactSpec], model: &SiteModel) -> AssetReport {
    let inventory = static_inventory(specs);
    let referenced_set = referenced_paths(model);
    let mut resolved = 0;
    let mut missing = 0;
    for path in &referenced_set {
        if is_known_asset(&inventory, path) {
            resolved += 1;
        } else {
            missing += 1;
        }
    }
    let unsafe_paths = count_unsafe_references(model);
    let unreferenced = inventory
        .iter()
        .filter(|p| !is_referenced(&referenced_set, p))
        .count();
    let derivatives = specs
        .iter()
        .filter(|s| s.kind == signal_core::ArtifactKind::DerivedImage)
        .count();
    let social_images = specs
        .iter()
        .filter(|s| s.kind == signal_core::ArtifactKind::SocialImage)
        .count();
    AssetReport {
        discovered: inventory.len(),
        referenced: referenced_set.len(),
        resolved,
        missing,
        unsafe_paths,
        unreferenced,
        derivatives,
        social_images,
    }
}

/// Planned source assets no content entry references, sorted.
///
/// The reverse of [`entry_asset_paths`](signal_core::entry_asset_paths) with
/// the same raw-then-decoded matching rule. Shared by the `check` count and
/// A6 diagnostics, so "unreferenced" means one thing.
pub fn unreferenced_source_assets(specs: &[ArtifactSpec], model: &SiteModel) -> Vec<String> {
    let inventory = static_inventory(specs);
    let referenced_set = referenced_paths(model);
    inventory
        .into_iter()
        .filter(|path| !is_referenced(&referenced_set, path))
        .collect()
}

/// Planned `Static` spec paths as a set (the source-asset inventory).
fn static_inventory(specs: &[ArtifactSpec]) -> BTreeSet<String> {
    specs
        .iter()
        .filter(|s| s.kind == signal_core::ArtifactKind::Static)
        .map(|s| s.path.clone())
        .collect()
}

/// Distinct internal asset paths referenced by content, across all entries.
fn referenced_paths(model: &SiteModel) -> BTreeSet<String> {
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for entry in model.entries() {
        for path in entry_asset_paths(entry) {
            referenced.insert(path);
        }
    }
    referenced
}

/// Whether a referenced path matches a planned source asset: raw form
/// first, then the percent-decoded form (mirrors `link_check`).
fn is_known_asset(inventory: &BTreeSet<String>, path: &str) -> bool {
    if inventory.contains(path) {
        return true;
    }
    match percent_decode(path) {
        Some(decoded) => decoded != path && inventory.contains(&decoded),
        None => false,
    }
}

/// Whether a planned source asset is named by any reference (raw or
/// decoded form, either direction).
fn is_referenced(referenced: &BTreeSet<String>, planned: &str) -> bool {
    if referenced.contains(planned) {
        return true;
    }
    for path in referenced {
        if let Some(decoded) = percent_decode(path) {
            if decoded == planned {
                return true;
            }
        }
    }
    // A planned path containing `%` could itself be the decoded target of
    // an encoded reference only when the encoded form decodes to it —
    // covered above. A planned literal `%` file referenced verbatim is
    // covered by the direct check.
    false
}

/// Count internal image references that resolve to nothing because they
/// escape the site root. External destinations are not assets and do not
/// count; missing-but-well-formed references count as `missing`, not here.
fn count_unsafe_references(model: &SiteModel) -> usize {
    let mut count = 0;
    for entry in model.entries() {
        if let Some(image) = entry.image.as_deref() {
            let target = image.trim();
            if !target.is_empty()
                && !signal_core::is_external_reference(target)
                && signal_core::resolve_front_matter_image(target).is_none()
            {
                count += 1;
            }
        }
        for raw in &entry.body.images {
            let target = raw.trim();
            if target.is_empty() || signal_core::is_external_reference(target) {
                continue;
            }
            // Strip query/fragment like resolution does; an empty remainder
            // is a self-reference, not an asset.
            let before_fragment = match target.find('#') {
                Some(i) => &target[..i],
                None => target,
            };
            let path_part = match before_fragment.find('?') {
                Some(i) => &before_fragment[..i],
                None => before_fragment,
            };
            if path_part.is_empty() {
                continue;
            }
            if signal_core::resolve_body_image(&entry.route.0, raw).is_none() {
                count += 1;
            }
        }
    }
    count
}

/// Normalize a user-supplied asset target to its `static/`-relative path.
///
/// Accepts `images/a.svg`, `/images/a.svg`, and `static/images/a.svg`.
/// Rejects escaping (`..`), absolute-filesystem-looking, and empty inputs
/// with a model diagnostic (existing input-guard convention).
pub fn normalize_asset_target(raw: &str) -> Result<String, BuildError> {
    let mut target = raw.trim().to_string();
    if target.is_empty() {
        return Err(BuildError::Model {
            message: "asset target must not be empty".to_string(),
        });
    }
    if let Some(stripped) = target.strip_prefix("static/") {
        target = stripped.to_string();
    }
    target = target.trim_start_matches('/').to_string();
    if target.is_empty()
        || target
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || target.contains('\\')
    {
        return Err(BuildError::Model {
            message: format!("invalid asset target {raw:?}"),
        });
    }
    Ok(target)
}

/// Resolve a normalized target to the planned spec path (raw, else decoded).
fn match_planned(specs: &[ArtifactSpec], target: &str) -> Option<String> {
    let inventory: BTreeSet<&str> = specs
        .iter()
        .filter(|s| s.kind == signal_core::ArtifactKind::Static)
        .map(|s| s.path.as_str())
        .collect();
    if inventory.contains(target) {
        return Some(target.to_string());
    }
    if let Some(decoded) = percent_decode(target) {
        if decoded != target && inventory.contains(decoded.as_str()) {
            return Some(decoded);
        }
    }
    None
}

/// One measured source asset for `explain`: identity plus filesystem facts
/// and reverse references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainedAsset {
    /// Planned path (`static/`-relative, output-relative in A1).
    pub path: String,
    /// Source file (`static/<path>`, root-relative display).
    pub source: String,
    /// MIME type from the extension.
    pub mime: String,
    /// Byte size of the source file.
    pub size: u64,
    /// Content hash of the source bytes.
    pub digest: Digest,
    /// Routes referencing this asset, sorted (empty when unreferenced).
    pub referrers: Vec<String>,
    /// Output path (identity in A1).
    pub output: String,
    /// Planned derivatives of this source (A2), in output-path order:
    /// output path plus actual output dimensions.
    pub derivatives: Vec<signal_core::DerivativeView>,
}

/// Why one page's social image is or is not planned (A5, ADR 0032).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocialExplainState {
    /// Planned: the artifact exists in the current plan.
    Planned,
    /// `[social]` is absent or `enabled = false`.
    Disabled,
    /// The page opted out with front matter `social_image: false`.
    OptedOut,
    /// Section roots are listings, never cards.
    Listing,
}

/// One page's social image for `explain`: identity, metadata inputs, and
/// whether the artifact is planned. All pure — no bytes are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainedSocial {
    /// Card output path, e.g. `social/posts/example.png`.
    pub path: String,
    /// The page's route, e.g. `/posts/example/`.
    pub route: String,
    /// Collection-relative source display, e.g. `posts/example.md`.
    pub source: String,
    /// Configured card dimensions, when the site enables social images.
    pub dimensions: Option<(u32, u32)>,
    /// Page title (always rendered).
    pub title: String,
    /// Page description (rendered when present).
    pub description: Option<String>,
    /// Effective author (entry or site), when present.
    pub author: Option<String>,
    /// Composited hero source, when one is rendered.
    pub hero: Option<String>,
    /// Whether the artifact is planned for this page.
    pub state: SocialExplainState,
}

/// Measure one page's social-image record for `explain`.
///
/// `raw_target` is a social output path (`social/posts/a.png`, with or
/// without a leading `/`). Resolution goes through
/// [`crate::social::planned_social`] — the same path → page inversion
/// planning, input derivation, source validation, and resolution use — so
/// `explain` can never disagree with the build about which page a card
/// belongs to or what it consumes. The plan state is derived from
/// `social_image_eligible`, the same predicate planning uses.
pub fn explain_social_data(
    raw_target: &str,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<ExplainedSocial, BuildError> {
    let normalized = normalize_asset_target(raw_target)?;
    let planned =
        crate::social::planned_social(model, &normalized).map_err(|_| BuildError::Model {
            message: format!("not a social image path with a known page: {raw_target:?}"),
        })?;
    let entry = planned.entry;
    let dimensions = config.social_size();
    let state = if dimensions.is_none() {
        SocialExplainState::Disabled
    } else if signal_core::social_image_override(entry) == Some(false) {
        SocialExplainState::OptedOut
    } else if entry.section_root {
        SocialExplainState::Listing
    } else {
        SocialExplainState::Planned
    };
    let author = entry
        .author
        .clone()
        .or_else(|| config.site.author.clone())
        .filter(|author| !author.trim().is_empty());
    Ok(ExplainedSocial {
        path: normalized,
        route: planned.route,
        source: format!("{}/{}", entry.collection.0, entry.source.relative_path),
        dimensions,
        title: entry.title.clone(),
        description: entry
            .description
            .clone()
            .filter(|description| !description.trim().is_empty()),
        author,
        hero: planned.hero,
        state,
    })
}

/// One planned artifact for `explain`: identity, declared inputs, and
/// kind-specific facts. Pure — no bytes are read, resolved, or written.
///
/// This is the generic record `signal explain <output-path>` renders for a
/// planned artifact that is not a source asset, derivative, or social card.
/// The search index (`index.json`) is the motivating case (A7.1); the same
/// plan lookup also covers the sitemap, feeds, `robots.txt`, `404.html`,
/// and rendered pages, because the record is derived from the spec list and
/// [`artifact_inputs`](crate::build_plan::artifact_inputs) — the same
/// values the build plans from, so `explain` can never disagree with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainedArtifact {
    /// Planned output path, e.g. `index.json`.
    pub path: String,
    /// Planned artifact kind.
    pub kind: signal_core::ArtifactKind,
    /// Declared inputs, in declaration order.
    pub inputs: Vec<InputRef>,
    /// Search-index document count (`SearchIndex` only; `None` otherwise).
    pub documents: Option<usize>,
}

/// Resolve one planned artifact by output path and derive its record.
///
/// `target` is a normalized output path (`index.json`). `Ok(None)` means no
/// planned artifact has that path; the caller decides the diagnostic.
/// Resolution is a lookup in the plan's spec list, so `explain` can never
/// describe an artifact the build would not write.
pub fn explain_artifact_data(
    specs: &[ArtifactSpec],
    config: &SignalConfig,
    model: &SiteModel,
    target: &str,
) -> Result<Option<ExplainedArtifact>, BuildError> {
    let Some(spec) = specs.iter().find(|spec| spec.path == target) else {
        return Ok(None);
    };
    let inputs = crate::build_plan::artifact_inputs(spec, config, model)?;
    let documents = (spec.kind == signal_core::ArtifactKind::SearchIndex)
        .then(|| signal_generators::search::search_documents(model).len());
    Ok(Some(ExplainedArtifact {
        path: spec.path.clone(),
        kind: spec.kind.clone(),
        inputs,
        documents,
    }))
}

/// One requested derivative for `explain`: the spec plus measured facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainedDerivative {
    /// The derivative request (source, width, format).
    pub spec: signal_core::DerivativeSpec,
    /// Decoded source dimensions.
    pub source_dimensions: (u32, u32),
    /// Source format as decoded (`PNG` or `JPEG`).
    pub source_format: String,
    /// Actual output dimensions (clamped, never upscaled).
    pub output_dimensions: (u32, u32),
    /// Derivative output path.
    pub output: String,
    /// Routes referencing the source (the derivative's consumers), sorted.
    pub referrers: Vec<String>,
    /// The source's responsive representation (candidates, default,
    /// sizes), selected by the same function rendering uses.
    pub responsive: Option<signal_core::ResponsiveImage>,
}

/// Measure one planned source asset: read its bytes for size/hash and
/// collect its referrers from the model.
pub fn explain_asset_data(
    root: &Path,
    specs: &[ArtifactSpec],
    config: &SignalConfig,
    model: &SiteModel,
    raw_target: &str,
) -> Result<ExplainedAsset, BuildError> {
    let target = normalize_asset_target(raw_target)?;
    let path = match match_planned(specs, &target) {
        Some(path) => path,
        None => {
            return Err(BuildError::Model {
                message: format!("unknown asset {raw_target:?}"),
            });
        }
    };
    let source_path = root.join("static").join(&path);
    let bytes = std::fs::read(&source_path).map_err(|e| BuildError::Read {
        path: source_path.display().to_string(),
        message: e.to_string(),
    })?;
    #[allow(clippy::cast_possible_truncation)]
    let size = bytes.len() as u64;
    let digest = crate::manifest::digest_bytes(&bytes);
    let referrers = referrers_for(model, &path);
    let derivatives = derivative_views(root, config, model, &path)?;
    Ok(ExplainedAsset {
        source: format!("static/{path}"),
        output: signal_core::output_path_for_source(&path),
        mime: signal_core::mime_for_path(&path).to_string(),
        size,
        digest,
        referrers,
        path,
        derivatives,
    })
}

/// Planned derivatives of one source asset, with actual output dimensions.
///
/// Planned request set for one source plus its decoded dimensions:
/// the shared measurement behind asset records, derivative records, and
/// responsive selection. `None` when the source has no planned
/// derivatives (SVG, unreferenced files, unconfigured builds) — without
/// touching the decoder.
struct MeasuredViews {
    /// Decoded source dimensions.
    dimensions: (u32, u32),
    /// One view per planned width, in output-path order.
    views: Vec<signal_core::DerivativeView>,
}

fn measured_views(
    root: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    source: &str,
) -> Result<Option<MeasuredViews>, BuildError> {
    let mut planned: Vec<signal_core::DerivativeSpec> =
        crate::images::planned_derivatives(config, model)?
            .into_iter()
            .filter(|deriv| deriv.source == source)
            .collect();
    if planned.is_empty() {
        return Ok(None);
    }
    planned.sort_by_key(|deriv| deriv.output_path());
    let path = root.join("static").join(source);
    let bytes = std::fs::read(&path).map_err(|e| BuildError::Read {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let (source_width, source_height, _) =
        crate::images::probe_dimensions(&bytes).map_err(|message| BuildError::Read {
            path: path.display().to_string(),
            message,
        })?;
    Ok(Some(MeasuredViews {
        dimensions: (source_width, source_height),
        views: planned
            .into_iter()
            .map(|deriv| {
                let (actual_width, actual_height) =
                    deriv.target_dimensions(source_width, source_height);
                signal_core::DerivativeView {
                    output: deriv.output_path(),
                    width: deriv.width,
                    format: deriv.format,
                    actual_width,
                    actual_height,
                }
            })
            .collect(),
    }))
}

/// Decodes the source once; the planned request set (config plus model)
/// determines which widths render. Sources with no planned derivatives
/// (SVG, unreferenced files, unconfigured builds) report none without
/// touching the decoder.
fn derivative_views(
    root: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    source: &str,
) -> Result<Vec<signal_core::DerivativeView>, BuildError> {
    Ok(measured_views(root, config, model, source)?.map_or(Vec::new(), |measured| measured.views))
}

/// Measure one requested derivative: resolve the request, decode the
/// source for input dimensions, and collect the source's referrers.
///
/// `raw_target` accepts either the source path (`images/hero.jpg`,
/// `/images/hero.jpg`, `static/images/hero.jpg`) or the derivative
/// output path (`images/hero-1280.webp`). `format_name` defaults to
/// `"webp"` when `None`. Requests outside the planned set fail with a
/// model diagnostic naming the configured widths.
pub fn explain_derivative_data(
    root: &Path,
    specs: &[ArtifactSpec],
    config: &SignalConfig,
    model: &SiteModel,
    raw_target: &str,
    width: u32,
    format_name: Option<&str>,
) -> Result<ExplainedDerivative, BuildError> {
    if width == 0 {
        return Err(BuildError::Model {
            message: "derivative width must be at least 1".to_string(),
        });
    }
    let format = match format_name {
        None => signal_core::DerivativeFormat::WebP,
        Some(name) => {
            signal_core::DerivativeFormat::parse(name).map_err(|message| BuildError::Model {
                message: format!("invalid derivative format: {message}"),
            })?
        }
    };
    let target = normalize_asset_target(raw_target)?;
    // A derivative output path inverts to its request; anything else is a
    // source path combined with the requested parameters.
    let spec = match crate::images::derivative_for_output(config, model, &target) {
        Some(deriv) if deriv.width == width && deriv.format == format => deriv,
        Some(_) => {
            return Err(BuildError::Model {
                message: format!("derivative {raw_target:?} was planned with different parameters"),
            });
        }
        None => {
            signal_core::DerivativeSpec::new(target.clone(), width, format).map_err(|message| {
                BuildError::Model {
                    message: format!("invalid derivative request: {message}"),
                }
            })?
        }
    };
    let planned = crate::images::planned_derivatives(config, model)?;
    if !planned.contains(&spec) {
        return Err(BuildError::Model {
            message: format!(
                "derivative {} at {width}w {} is not planned ([images] widths: {:?})",
                spec.source,
                spec.format,
                config.image_widths(),
            ),
        });
    }
    // The output must be a planned artifact (it is, by construction), so
    // an unknown path here is a planner disagreement — fail loudly.
    if !specs
        .iter()
        .any(|s| s.kind == signal_core::ArtifactKind::DerivedImage && s.path == spec.output_path())
    {
        return Err(BuildError::Model {
            message: format!(
                "derivative {:?} is not a planned artifact",
                spec.output_path()
            ),
        });
    }
    let path = root.join("static").join(&spec.source);
    let bytes = std::fs::read(&path).map_err(|e| BuildError::Read {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let (source_width, source_height, source_format) = crate::images::probe_dimensions(&bytes)
        .map_err(|message| BuildError::Read {
            path: path.display().to_string(),
            message,
        })?;
    let output_dimensions = spec.target_dimensions(source_width, source_height);
    let referrers = referrers_for(model, &spec.source);
    // The responsive representation comes from the same measured views
    // and pure selector rendering uses — never a second implementation.
    let responsive = measured_views(root, config, model, &spec.source)?.and_then(|measured| {
        signal_core::responsive_image(&spec.source, measured.dimensions, &measured.views)
    });
    Ok(ExplainedDerivative {
        output: spec.output_path(),
        source_dimensions: (source_width, source_height),
        source_format,
        output_dimensions,
        referrers,
        responsive,
        spec,
    })
}

/// Routes referencing one planned asset path, sorted. Matches raw and
/// decoded reference forms, mirroring validation.
pub fn referrers_for(model: &SiteModel, asset_path: &str) -> Vec<String> {
    let index = signal_core::asset_referrers(model);
    let mut out: BTreeSet<String> = BTreeSet::new();
    if let Some(routes) = index.get(asset_path) {
        out.extend(routes.iter().cloned());
    }
    for (referenced, routes) in &index {
        if referenced == asset_path {
            continue;
        }
        if let Some(decoded) = percent_decode(referenced) {
            if decoded == asset_path {
                out.extend(routes.iter().cloned());
            }
        }
    }
    // A planned literal-`%` path referenced verbatim is the direct hit
    // above; nothing else to do.
    out.into_iter().collect()
}

/// Routes each entry references, for debugging and tests: route →
/// sorted asset paths.
pub fn entry_assets(model: &SiteModel) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for entry in model.entries() {
        out.insert(entry.route.0.clone(), entry_asset_paths(entry));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{ArtifactKind, CollectionId, ContentId, Route, Slug, SourceRef};

    fn entry(
        id: u32,
        route: &str,
        image: Option<&str>,
        images: &[&str],
    ) -> signal_core::ContentEntry {
        let mut e = signal_core::ContentEntry::new(
            ContentId(id),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), format!("e{id}.md")),
            Slug::new(format!("e{id}")),
            Route::new(route.to_string()),
            format!("Title {id}"),
        );
        e.image = image.map(str::to_string);
        e.body.images = images.iter().map(|s| s.to_string()).collect();
        e
    }

    fn model() -> SiteModel {
        let mut b = signal_core::SiteModelBuilder::new();
        b.add_entry(entry(
            1,
            "/posts/a/",
            Some("/images/a.svg"),
            &["/images/b.png"],
        ));
        b.add_entry(entry(2, "/posts/b/", None, &["https://example.com/x.png"]));
        b.build().expect("builds")
    }

    fn specs() -> Vec<ArtifactSpec> {
        vec![
            ArtifactSpec::new("images/a.svg", ArtifactKind::Static),
            ArtifactSpec::new("images/b.png", ArtifactKind::Static),
            ArtifactSpec::new("js/app.js", ArtifactKind::Static),
            ArtifactSpec::new("posts/a/index.html", ArtifactKind::Page)
                .with_route(Route::new("/posts/a/")),
        ]
    }

    #[test]
    fn report_counts_discovery_and_references() {
        let report = build_asset_report(&specs(), &model());
        assert_eq!(report.discovered, 3);
        assert_eq!(report.referenced, 2);
        assert_eq!(report.resolved, 2);
        assert_eq!(report.missing, 0);
        assert_eq!(report.unsafe_paths, 0);
        // `js/app.js` is on disk but referenced by nothing.
        assert_eq!(report.unreferenced, 1);
    }

    #[test]
    fn report_detects_missing_assets() {
        let mut b = signal_core::SiteModelBuilder::new();
        b.add_entry(entry(1, "/posts/a/", Some("/images/gone.svg"), &[]));
        let model = b.build().expect("builds");
        let report = build_asset_report(&specs(), &model);
        assert_eq!(report.referenced, 1);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.missing, 1);
    }

    #[test]
    fn report_counts_escaping_references_as_unsafe() {
        let mut b = signal_core::SiteModelBuilder::new();
        b.add_entry(entry(1, "/posts/a/", None, &["../../../evil.png"]));
        let model = b.build().expect("builds");
        let report = build_asset_report(&specs(), &model);
        assert_eq!(report.unsafe_paths, 1);
        assert_eq!(report.referenced, 0);
    }

    #[test]
    fn target_normalization_accepts_common_forms() {
        assert_eq!(
            normalize_asset_target("images/a.svg").expect("ok"),
            "images/a.svg"
        );
        assert_eq!(
            normalize_asset_target("/images/a.svg").expect("ok"),
            "images/a.svg"
        );
        assert_eq!(
            normalize_asset_target("static/images/a.svg").expect("ok"),
            "images/a.svg"
        );
        for bad in ["", "static/", "../evil.svg", "a/../b.svg", "a\\b.svg"] {
            assert!(
                normalize_asset_target(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn referrers_map_to_routes() {
        assert_eq!(referrers_for(&model(), "images/a.svg"), vec!["/posts/a/"]);
        assert_eq!(referrers_for(&model(), "js/app.js"), Vec::<String>::new());
    }
}
