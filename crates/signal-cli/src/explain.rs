//! Read-only build-plan diagnostics (`signal build --explain`).
//!
//! Rendering lives at the CLI boundary: [`render_plan`] turns an already
//! computed [`BuildPlan`] into deterministic human-readable text, and
//! [`explain_site_from_disk`] constructs that plan through the exact same
//! path execution uses ([`validated_plan`](crate::build::validated_plan)
//! plus [`plan`](crate::build_plan::plan)) — then stops. It resolves
//! nothing, writes nothing, prunes nothing, and persists no manifest.
//!
//! Output conventions follow the `signal build` summary: two-space
//! indentation, relative artifact paths only, no timestamps, no colors, no
//! absolute paths. Decisions render in plan (sorted-path) order and the
//! stale list renders sorted, so repeated runs are byte-identical.

use std::path::Path;

use signal_core::{SignalConfig, SiteModel};

use crate::build_plan::{BuildDecision, BuildPlan};
use crate::errors::BuildError;

/// Render one [`BuildPlan`] as deterministic human-readable text.
///
/// Pure function of the plan: rendering the same plan twice yields
/// byte-identical output.
pub fn render_plan(planned: &BuildPlan) -> String {
    let mut out = String::new();
    out.push_str("Build plan\n");
    out.push_str("==========\n");
    out.push_str("\nSummary:\n");
    out.push_str(&format!("  artifacts: {}\n", planned.decisions.len()));
    out.push_str(&format!("  reuse:     {}\n", planned.reused_count()));
    out.push_str(&format!("  rebuild:   {}\n", planned.rebuilt_count()));
    out.push_str(&format!("  stale:     {}\n", planned.stale.len()));
    out.push_str("\nReuse:\n");
    let mut reused = false;
    for decision in &planned.decisions {
        if let BuildDecision::Reuse { spec, .. } = decision {
            out.push_str(&format!("  {}\n", spec.path));
            reused = true;
        }
    }
    if !reused {
        out.push_str("  (none)\n");
    }
    out.push_str("\nRebuild:\n");
    let mut rebuilt = false;
    for decision in &planned.decisions {
        if let BuildDecision::Rebuild { spec, reason } = decision {
            out.push_str(&format!("  {}\n", spec.path));
            out.push_str(&format!("    reason: {reason}\n"));
            rebuilt = true;
        }
    }
    if !rebuilt {
        out.push_str("  (none)\n");
    }
    out.push_str("\nPrune:\n");
    if planned.stale.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for stale in &planned.stale {
            out.push_str(&format!("  {stale}\n"));
        }
    }
    out
}

/// Construct the plan execution would use, without executing it.
///
/// Runs the canonical pre-execution pipeline — the identical
/// [`validate_current_site`](crate::pipeline::validate_current_site)
/// construction [`build_site`](crate::build::build_site) starts from
/// (structural validation plus reference validation) — then plans and
/// returns before any resolve, write, prune, or manifest persist.
/// A site that would fail the build's pre-plan gates (invalid routes,
/// menus, output collisions, broken references) fails here identically:
/// `--explain` describes the plan for a site that would otherwise pass
/// validation, never a plan for a site the build would reject.
/// Validation runs as in a build, including its transient
/// filesystem-alias probe (created and removed during validation); no
/// artifacts are resolved or written, nothing is pruned, and
/// `.signal/manifest.json` is never touched.
pub fn explain_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<String, BuildError> {
    let validated = crate::pipeline::validate_current_site(root, out_dir, config, model)?;
    let previous = crate::manifest::load_previous(out_dir);
    let planned = crate::build_plan::plan(
        &validated.plan.specs,
        config,
        model,
        &validated.renderer,
        root,
        out_dir,
        &previous,
    )?;
    Ok(render_plan(&planned))
}

/// Explain one source asset: identity, measured facts, reverse references,
/// planned derivatives, and the build decision for its `Static` artifact.
///
/// Read-only like [`render_plan`]: constructs the same plan execution would
/// use (through the shared pipeline prefix, including reference
/// validation), then renders one asset's record without resolving,
/// writing, pruning, or persisting anything.
///
/// `target` accepts `images/a.svg`, `/images/a.svg`, or
/// `static/images/a.svg`. Unknown targets fail with a model diagnostic.
/// A derivative *output* path (`images/hero-640.webp`) transparently
/// explains the derivative instead (same as passing the source with
/// matching `--width`/`--format`).
pub fn explain_asset(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    specs: &[signal_core::ArtifactSpec],
    renderer: &signal_render::MiniJinjaRenderer,
    raw_target: &str,
) -> Result<String, BuildError> {
    match crate::assets::explain_asset_data(root, specs, config, model, raw_target) {
        Ok(asset) => {
            let previous = crate::manifest::load_previous(out_dir);
            let planned =
                crate::build_plan::plan(specs, config, model, renderer, root, out_dir, &previous)?;
            let decision = planned
                .decisions
                .iter()
                .find(|d| d.spec().path == asset.path);
            // A6: the same analysis `check` renders, filtered to this asset.
            let diagnostics = crate::diagnostics::for_subject(
                &crate::diagnostics::analyze(root, config, model, specs),
                &asset.path,
            );
            Ok(render_asset(&asset, decision, &diagnostics))
        }
        Err(BuildError::Model { .. }) => {
            // Not a source asset: a derivative output path explains its
            // derivative directly, without requiring --width/--format, a
            // social card output path explains the card (A5), and any other
            // planned artifact — the search index (A7.1), sitemap, feeds,
            // robots, 404, a rendered page — explains by output path.
            let normalized = crate::assets::normalize_asset_target(raw_target)?;
            match crate::images::derivative_for_output(config, model, &normalized) {
                Some(deriv) => explain_derivative(
                    root,
                    out_dir,
                    config,
                    model,
                    specs,
                    renderer,
                    &deriv.source,
                    deriv.width,
                    Some(&deriv.format.to_string()),
                ),
                None if signal_core::social_image_route(&normalized).is_some() => {
                    let social = crate::assets::explain_social_data(&normalized, config, model)?;
                    let previous = crate::manifest::load_previous(out_dir);
                    let planned = crate::build_plan::plan(
                        specs, config, model, renderer, root, out_dir, &previous,
                    )?;
                    let decision = planned
                        .decisions
                        .iter()
                        .find(|d| d.spec().path == social.path);
                    Ok(render_social(&social, decision))
                }
                None => {
                    match crate::assets::explain_artifact_data(specs, config, model, &normalized)? {
                        Some(artifact) => {
                            let previous = crate::manifest::load_previous(out_dir);
                            let planned = crate::build_plan::plan(
                                specs, config, model, renderer, root, out_dir, &previous,
                            )?;
                            let decision = planned
                                .decisions
                                .iter()
                                .find(|d| d.spec().path == artifact.path);
                            Ok(render_artifact(&artifact, decision))
                        }
                        None => Err(BuildError::Model {
                            message: format!("unknown asset {raw_target:?}"),
                        }),
                    }
                }
            }
        }
        Err(other) => Err(other),
    }
}

/// Convenience: ingest from disk and explain one asset in one call.
pub fn explain_asset_from_disk(
    root: &Path,
    out_dir: &Path,
    raw_target: &str,
) -> Result<String, BuildError> {
    let loaded = crate::pipeline::load_validated_site(root, out_dir)?;
    explain_asset(
        root,
        out_dir,
        &loaded.config,
        &loaded.model,
        &loaded.validated.plan.specs,
        &loaded.validated.renderer,
        raw_target,
    )
}

/// Render one measured asset plus its plan decision as deterministic text.
///
/// Relative paths only, no timestamps, no colors. The `Size` section
/// reports source bytes; planned derivatives (A2) render with actual
/// output dimensions beside each output path. A6 diagnostics, when any
/// apply to this asset, render last and are omitted entirely for a clean
/// asset (so pre-A6 output is unchanged).
fn render_asset(
    asset: &crate::assets::ExplainedAsset,
    decision: Option<&crate::build_plan::BuildDecision>,
    diagnostics: &[crate::diagnostics::Diagnostic],
) -> String {
    let mut out = String::new();
    out.push_str("Asset\n");
    out.push_str("=====\n");
    out.push_str(&format!("  path: {}\n", asset.path));
    out.push_str("\nSource:\n");
    out.push_str(&format!("  {}\n", asset.source));
    out.push_str("\nType:\n");
    out.push_str(&format!("  {}\n", asset.mime));
    out.push_str("\nSize:\n");
    out.push_str(&format!("  {} bytes\n", asset.size));
    out.push_str("\nDependency:\n");
    if asset.referrers.is_empty() {
        out.push_str("  (no referencing entries)\n");
    } else {
        for route in &asset.referrers {
            out.push_str(&format!("  {route}\n"));
        }
    }
    out.push_str("\nOutput:\n");
    out.push_str(&format!("  {}\n", asset.output));
    out.push_str("\nAction:\n");
    out.push_str("  copy\n");
    if !asset.derivatives.is_empty() {
        out.push_str("\nDerivatives:\n");
        for deriv in &asset.derivatives {
            out.push_str(&format!(
                "  {} ({}w → {} × {})\n",
                deriv.output, deriv.width, deriv.actual_width, deriv.actual_height
            ));
        }
    }
    out.push_str("\nDecision:\n");
    match decision {
        Some(crate::build_plan::BuildDecision::Reuse { .. }) => {
            out.push_str("  reuse\n");
        }
        Some(crate::build_plan::BuildDecision::Rebuild { reason, .. }) => {
            out.push_str(&format!("  rebuild: {reason}\n"));
        }
        None => out.push_str("  (not planned)\n"),
    }
    render_diagnostics(&mut out, diagnostics);
    out
}

/// Append one explained subject's A6 diagnostics, if any, as a deterministic
/// section. Omitted entirely when there is nothing to report, so a clean
/// asset's record is byte-identical to its pre-A6 form.
fn render_diagnostics(out: &mut String, diagnostics: &[crate::diagnostics::Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    out.push_str("\nDiagnostics:\n");
    for diagnostic in diagnostics {
        out.push_str(&format!(
            "  {}: {}\n",
            diagnostic.severity().as_str(),
            diagnostic.message()
        ));
    }
}

/// Explain one requested image derivative (A2, ADR 0029): source, input
/// and output dimensions, output path, dependencies, and the build
/// decision for its `DerivedImage` artifact.
///
/// Read-only like [`render_plan`]: the same plan execution would use,
/// then one derivative's record. `raw_target` accepts the source path
/// (`images/hero.jpg` and its `/`-prefixed / `static/`-prefixed forms)
/// or the derivative output path; `width` and `format_name` (`None`
/// defaults to `"webp"`) select the request. Unplanned requests fail
/// with a model diagnostic naming the configured widths.
#[allow(clippy::too_many_arguments)]
pub fn explain_derivative(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    specs: &[signal_core::ArtifactSpec],
    renderer: &signal_render::MiniJinjaRenderer,
    raw_target: &str,
    width: u32,
    format_name: Option<&str>,
) -> Result<String, BuildError> {
    let deriv = crate::assets::explain_derivative_data(
        root,
        specs,
        config,
        model,
        raw_target,
        width,
        format_name,
    )?;
    let previous = crate::manifest::load_previous(out_dir);
    let planned =
        crate::build_plan::plan(specs, config, model, renderer, root, out_dir, &previous)?;
    let decision = planned
        .decisions
        .iter()
        .find(|d| d.spec().path == deriv.output);
    // A6: diagnostics belong to the derivative's *source* (oversized
    // source, redundant requested widths), so filter by that subject.
    let diagnostics = crate::diagnostics::for_subject(
        &crate::diagnostics::analyze(root, config, model, specs),
        &deriv.spec.source,
    );
    Ok(render_derivative(&deriv, decision, &diagnostics))
}

/// Convenience: ingest from disk and explain one derivative in one call.
pub fn explain_derivative_from_disk(
    root: &Path,
    out_dir: &Path,
    raw_target: &str,
    width: u32,
    format_name: Option<&str>,
) -> Result<String, BuildError> {
    let loaded = crate::pipeline::load_validated_site(root, out_dir)?;
    explain_derivative(
        root,
        out_dir,
        &loaded.config,
        &loaded.model,
        &loaded.validated.plan.specs,
        &loaded.validated.renderer,
        raw_target,
        width,
        format_name,
    )
}

/// Render one derivative request plus its plan decision as deterministic
/// text. Relative paths only, no timestamps, no colors. A6 diagnostics for
/// the derivative's source render last when any apply.
fn render_derivative(
    deriv: &crate::assets::ExplainedDerivative,
    decision: Option<&crate::build_plan::BuildDecision>,
    diagnostics: &[crate::diagnostics::Diagnostic],
) -> String {
    let mut out = String::new();
    out.push_str("Derivative\n");
    out.push_str("==========\n");
    out.push_str(&format!("  path: {}\n", deriv.output));
    out.push_str("\nSource:\n");
    out.push_str(&format!("  static/{}\n", deriv.spec.source));
    out.push_str("\nInput:\n");
    out.push_str(&format!(
        "  {} × {}\n  {}\n",
        deriv.source_dimensions.0, deriv.source_dimensions.1, deriv.source_format
    ));
    out.push_str("\nDerivative:\n");
    out.push_str(&format!(
        "  width: {}\n  height: {}\n  format: {}\n",
        deriv.output_dimensions.0, deriv.output_dimensions.1, deriv.spec.format,
    ));
    out.push_str("\nOutput:\n");
    out.push_str(&format!("  {}\n", deriv.output));
    out.push_str("\nDependencies:\n");
    out.push_str(&format!("  source: {}\n", deriv.spec.source));
    if deriv.referrers.is_empty() {
        out.push_str("  (no consuming entries)\n");
    } else {
        for route in &deriv.referrers {
            out.push_str(&format!("  page: {route}\n"));
        }
    }
    // The responsive representation rendering embeds, selected by the
    // same function the rewriter and hero contexts use: fallback first,
    // then one group per planned format in `<source>` order.
    if let Some(responsive) = deriv.responsive.as_ref() {
        out.push_str("\nResponsive:\n");
        out.push_str(&format!("  fallback: {}\n", responsive.default_src));
        out.push_str(&format!("  sizes: {}\n", responsive.sizes));
        for group in &responsive.sources {
            out.push_str(&format!("  {}:\n", group.format));
            for candidate in &group.candidates {
                out.push_str(&format!("    {} {}w\n", candidate.url, candidate.width));
            }
        }
    }
    out.push_str("\nAction:\n");
    out.push_str("  generate\n");
    out.push_str("\nDecision:\n");
    match decision {
        Some(crate::build_plan::BuildDecision::Reuse { .. }) => {
            out.push_str("  reuse\n");
        }
        Some(crate::build_plan::BuildDecision::Rebuild { reason, .. }) => {
            out.push_str(&format!("  rebuild: {reason}\n"));
        }
        None => out.push_str("  (not planned)\n"),
    }
    render_diagnostics(&mut out, diagnostics);
    out
}

/// Render one page's social-image record plus its plan decision as
/// deterministic text (A5, ADR 0032).
///
/// Relative paths only, no timestamps, no colors. The inputs section names
/// exactly what the generator consumes, in the same order the artifact
/// declares them: page metadata (title, optional description, optional
/// author), the optional composited hero, and configuration. Non-planned
/// states render a fixed decision line instead of a plan lookup.
fn render_social(
    social: &crate::assets::ExplainedSocial,
    decision: Option<&crate::build_plan::BuildDecision>,
) -> String {
    use crate::assets::SocialExplainState;
    let mut out = String::new();
    out.push_str("Social image\n");
    out.push_str("============\n");
    out.push_str(&format!("  path: {}\n", social.path));
    out.push_str("\nPage:\n");
    out.push_str(&format!("  {}\n", social.route));
    out.push_str(&format!("  {}\n", social.source));
    match social.dimensions {
        Some((width, height)) => {
            out.push_str("\nDimensions:\n");
            out.push_str(&format!("  {width} × {height}\n"));
        }
        None => out.push_str("\nDimensions:\n  (not configured)\n"),
    }
    out.push_str("\nInputs:\n");
    out.push_str(&format!("  title: {}\n", social.title));
    if let Some(description) = &social.description {
        out.push_str(&format!("  description: {description}\n"));
    }
    if let Some(author) = &social.author {
        out.push_str(&format!("  author: {author}\n"));
    }
    if let Some(hero) = &social.hero {
        out.push_str(&format!("  hero: {hero}\n"));
    }
    out.push_str("  configuration: [social]\n");
    out.push_str("\nOutput:\n");
    out.push_str(&format!("  {}\n", social.path));
    out.push_str("\nAction:\n");
    out.push_str(match social.state {
        SocialExplainState::Planned => "  generate\n",
        _ => "  (none)\n",
    });
    out.push_str("\nDecision:\n");
    match social.state {
        SocialExplainState::Disabled => out.push_str("  disabled\n"),
        SocialExplainState::OptedOut => {
            out.push_str("  opted out (front matter social_image: false)\n")
        }
        SocialExplainState::Listing => out.push_str("  not planned (section roots are listings)\n"),
        SocialExplainState::Planned => match decision {
            Some(crate::build_plan::BuildDecision::Reuse { .. }) => out.push_str("  reuse\n"),
            Some(crate::build_plan::BuildDecision::Rebuild { reason, .. }) => {
                out.push_str(&format!("  rebuild: {reason}\n"));
            }
            None => out.push_str("  (not planned)\n"),
        },
    }
    out
}

/// Render one planned artifact's record plus its plan decision as
/// deterministic text (A7.1).
///
/// Relative paths only, no timestamps, no colors. The `Inputs` section
/// names exactly what the artifact declares, in declaration order, using
/// the same [`artifact_inputs`](crate::build_plan::artifact_inputs)
/// derivation the build plans from; the `Documents` section appears only
/// for the search index, whose document count is part of its contract.
fn render_artifact(
    artifact: &crate::assets::ExplainedArtifact,
    decision: Option<&crate::build_plan::BuildDecision>,
) -> String {
    let mut out = String::new();
    out.push_str("Artifact\n");
    out.push_str("========\n");
    out.push_str(&format!("  path: {}\n", artifact.path));
    out.push_str("\nKind:\n");
    out.push_str(&format!("  {:?}\n", artifact.kind));
    out.push_str("\nInputs:\n");
    for input in &artifact.inputs {
        out.push_str(&format!("  {}\n", render_input(input)));
    }
    if let Some(documents) = artifact.documents {
        out.push_str("\nDocuments:\n");
        out.push_str(&format!("  {documents}\n"));
    }
    out.push_str("\nDecision:\n");
    match decision {
        Some(crate::build_plan::BuildDecision::Reuse { .. }) => out.push_str("  reuse\n"),
        Some(crate::build_plan::BuildDecision::Rebuild { reason, .. }) => {
            out.push_str(&format!("  rebuild: {reason}\n"));
        }
        None => out.push_str("  (not planned)\n"),
    }
    out
}

/// Render one declared input reference in the CLI's compact vocabulary:
/// `Query(search_documents)`, `Entry(/posts/alpha/)`, `TemplateSet`,
/// `Config`, `Static(images/a.svg)`, `DerivedImage(images/a.jpg, 640w, webp)`.
fn render_input(input: &crate::manifest::InputRef) -> String {
    use crate::manifest::InputRef;
    match input {
        InputRef::Entry { route } => format!("Entry({route})"),
        InputRef::Query { key } => format!("Query({key})"),
        InputRef::Template { name } => format!("Template({name})"),
        InputRef::TemplateSet => "TemplateSet".to_string(),
        InputRef::Config => "Config".to_string(),
        InputRef::Static { path } => format!("Static({path})"),
        InputRef::DerivedImage {
            source,
            width,
            format,
        } => format!("DerivedImage({source}, {width}w, {format})"),
    }
}

/// Convenience: ingest from disk and explain in one call (used by the CLI).
pub fn explain_site_from_disk(root: &Path, out_dir: &Path) -> Result<String, BuildError> {
    let loaded = crate::pipeline::load_validated_site(root, out_dir)?;
    explain_validated_site(
        root,
        out_dir,
        &loaded.config,
        &loaded.model,
        loaded.validated.plan,
        loaded.validated.renderer,
    )
}

/// Explain an already-validated site state without revalidating.
///
/// Exists so the CLI loads once; behavior is identical to [`explain_site`].
#[allow(clippy::too_many_arguments)]
pub fn explain_validated_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    plan: crate::build::SpecPlan,
    renderer: signal_render::MiniJinjaRenderer,
) -> Result<String, BuildError> {
    let previous = crate::manifest::load_previous(out_dir);
    let planned = crate::build_plan::plan(
        &plan.specs,
        config,
        model,
        &renderer,
        root,
        out_dir,
        &previous,
    )?;
    Ok(render_plan(&planned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_plan::RebuildReason;

    fn write_site(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, content).expect("write");
        }
    }

    fn plan_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Explain\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[taxonomy]\nroute_prefix = \"/topics/\"\n[feed]\n",
                ),
                (
                    "content/posts/alpha.md",
                    "---\ntitle: Alpha\ndate: 2026-02-01\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
                ),
                (
                    "content/posts/beta.md",
                    "---\ntitle: Beta\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                (
                    "templates/topics.html",
                    "<html><body>topics</body></html>",
                ),
                (
                    "templates/topic.html",
                    "<html><body>topic</body></html>",
                ),
                ("static/asset.txt", "asset-1"),
            ],
        );
    }

    fn snapshot_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut entries = Vec::new();
        if !dir.is_dir() {
            return entries;
        }
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let mut children: Vec<_> = std::fs::read_dir(&current)
                .expect("read_dir")
                .map(|e| e.expect("entry").path())
                .collect();
            children.sort();
            for child in children {
                if child.is_dir() {
                    stack.push(child);
                } else {
                    let relative = child
                        .strip_prefix(dir)
                        .expect("prefix")
                        .to_string_lossy()
                        .replace('\\', "/");
                    entries.push((relative, std::fs::read(&child).expect("read")));
                }
            }
        }
        entries.sort();
        entries
    }

    #[test]
    fn every_rebuild_reason_renders() {
        let cases: Vec<(RebuildReason, &str)> = vec![
            (RebuildReason::NoUsableManifest, "no usable manifest"),
            (RebuildReason::GenerationMismatch, "generation changed"),
            (RebuildReason::MissingRecord, "manifest record missing"),
            (RebuildReason::KindChanged, "artifact kind changed"),
            (RebuildReason::InputsChanged, "inputs changed"),
            (
                RebuildReason::EntryChanged {
                    route: "/blog/foo/".to_string(),
                },
                "entry changed: /blog/foo/",
            ),
            (
                RebuildReason::QueryChanged {
                    key: "summaries:posts".to_string(),
                },
                "query changed: summaries:posts",
            ),
            (RebuildReason::TemplateSetChanged, "template set changed"),
            (
                RebuildReason::TemplateChanged {
                    name: "post.html".to_string(),
                },
                "template changed: post.html",
            ),
            (RebuildReason::ConfigChanged, "configuration changed"),
            (
                RebuildReason::StaticChanged {
                    path: "images/foo.png".to_string(),
                },
                "static file changed: images/foo.png",
            ),
            (
                RebuildReason::DerivativeChanged {
                    source: "images/hero.jpg".to_string(),
                },
                "derivative source changed: images/hero.jpg",
            ),
            (RebuildReason::OutputMissing, "output missing"),
            (RebuildReason::OutputChanged, "output changed"),
            (
                RebuildReason::InputError {
                    message: "boom".to_string(),
                },
                "input error: boom",
            ),
        ];
        assert_eq!(cases.len(), 15);
        for (reason, expected) in &cases {
            assert_eq!(reason.to_string(), *expected, "reason {reason:?}");
        }
    }

    #[test]
    fn unchanged_build_explains_all_reuse() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");
        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert!(text.contains("rebuild:   0\n"), "got:\n{text}");
        assert!(text.contains("stale:     0\n"), "got:\n{text}");
        assert!(text.contains("Reuse:\n"), "got:\n{text}");
        assert!(text.contains("Rebuild:\n  (none)\n"), "got:\n{text}");
        assert!(text.contains("Prune:\n  (none)\n"), "got:\n{text}");
        // Deterministic: explaining twice is byte-identical.
        let again = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert_eq!(text, again);
    }

    #[test]
    fn mixed_plan_renders_reuse_rebuild_and_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");

        // One content edit (rebuild with reason) plus one deletion (stale).
        std::fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta changed\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        std::fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");
        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        // Edited entry rebuilds with its entry reason…
        assert!(
            text.contains("  posts/beta/index.html\n    reason: entry changed: /posts/beta/\n"),
            "got:\n{text}"
        );
        // …the deleted entry's output is reported stale…
        assert!(text.contains("  posts/alpha/index.html\n"), "got:\n{text}");
        assert!(text.contains("Prune:\n"), "got:\n{text}");
        let prune = text.split("Prune:\n").nth(1).expect("prune section");
        assert!(
            prune.contains("posts/alpha/index.html"),
            "stale must list alpha: {prune}"
        );
        // …and untouched artifacts still reuse.
        assert!(text.contains("  asset.txt\n"), "got:\n{text}");
        // Deterministic rendering of the same state.
        let again = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert_eq!(text, again);
    }

    #[test]
    fn explain_is_read_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");

        // Create pending work: an edit (rebuild) and a deletion (stale).
        std::fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta changed\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        std::fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");

        let before_outputs = snapshot_dir(&out);
        let before_manifest =
            std::fs::read(out.join(".signal/manifest.json")).expect("manifest exists");
        // The stale output is still on disk before explaining.
        assert!(out.join("posts/alpha/index.html").exists());

        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert!(text.contains("rebuild:"), "got:\n{text}");

        // Nothing was written, removed, or persisted.
        assert_eq!(snapshot_dir(&out), before_outputs);
        assert_eq!(
            std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
            before_manifest
        );
        assert!(out.join("posts/alpha/index.html").exists());
        // The edited output still holds the previous build's bytes.
        let beta = std::fs::read_to_string(out.join("posts/beta/index.html")).expect("beta");
        assert!(!beta.contains("Beta changed"), "got: {beta}");
    }
}
