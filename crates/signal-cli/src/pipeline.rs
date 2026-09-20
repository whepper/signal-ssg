//! One canonical pre-execution pipeline shared by every command.
//!
//! ```text
//! load config
//!   → ingest + freeze SiteModel
//!   → validated specs (config gates, spec generation, output-path and
//!     route validation, template loading)
//!   → reference validation
//! ```
//!
//! [`build_site`](crate::build::build_site) continues from here into
//! incremental planning, execution, pruning, and manifest persistence.
//! [`explain`](crate::explain) and [`link_check::check`](crate::link_check)
//! stop at different points but never reimplement the prefix: a command
//! must not call a site valid while skipping validation a real build
//! requires.
//!
//! No framework machinery: plain functions over explicit values, no traits,
//! no dynamic dispatch, no async. Small enough to read in one sitting.

use std::path::Path;

use signal_core::{SignalConfig, SiteModel};
use signal_render::MiniJinjaRenderer;

use crate::build::SpecPlan;
use crate::errors::BuildError;
use crate::link_check::ReferenceReport;

/// Parse `signal.toml` under `root` with build-quality diagnostics.
///
/// Shared by `build`, `explain`, and `check` so all three reject the same
/// malformed configuration with the same error.
pub fn load_config_from_disk(root: &Path) -> Result<SignalConfig, BuildError> {
    let config_path = root.join("signal.toml");
    let text = crate::errors::read_file(&config_path)?;
    toml::from_str(&text).map_err(|e| crate::config::config_parse_error(&config_path, &text, e))
}

/// Ingest `root` into a frozen model plus the draft count for summaries.
///
/// Config-route prefixes are validated inside
/// [`ingest_site`](crate::ingest::ingest_site) before any entry normalizes,
/// so every command fails fast on the same invalid configuration.
pub fn ingest_from_disk(
    root: &Path,
    config: &SignalConfig,
) -> Result<(SiteModel, usize), BuildError> {
    crate::ingest::ingest_site(root, config)
}

/// Everything every command needs before deciding anything:
///
/// Validated specs, the loaded renderer, and the reference report.
///
/// Construction order mirrors [`build_site`](crate::build::build_site):
/// [`validated_plan`](crate::build::validated_plan) first (config gates,
/// spec generation, output-path and route validation, template loading),
/// then [`validate_references`](crate::link_check::validate_references).
/// Reference validation runs before incremental planning on purpose: a
/// broken reference fails closed before any execution, and read-only
/// commands describe or check exactly the site a build would validate.
pub struct ValidatedBuild {
    /// Validated specs plus site/output roots.
    pub plan: SpecPlan,
    /// Templates loaded from `<root>/templates` as in-memory strings.
    pub renderer: MiniJinjaRenderer,
    /// Reference validation activity (counts only; failures are errors).
    pub references: ReferenceReport,
}

/// Run the canonical pre-execution pipeline over an already-loaded
/// config and model.
pub fn validate_current_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<ValidatedBuild, BuildError> {
    let (plan, renderer) = crate::build::validated_plan(root, out_dir, config, model)?;
    let references = crate::link_check::validate_references(config, model, &plan.specs)?;
    Ok(ValidatedBuild {
        plan,
        renderer,
        references,
    })
}

/// Run the canonical read-only validation for `signal check`: the same
/// config gates, spec generation, logical output-path and route validation,
/// and template loading as [`validate_current_site`], plus the same
/// reference validation — but without requiring an output directory and
/// without the output-filesystem alias probe.
///
/// Returns the specs, renderer, and reference report so `check` reports
/// from exactly what it validated.
pub struct ValidatedCheck {
    /// Validated specs in sorted plan order.
    pub specs: Vec<signal_core::ArtifactSpec>,
    /// Templates loaded from `<root>/templates` as in-memory strings.
    pub renderer: MiniJinjaRenderer,
    /// Reference validation activity (counts only; failures are errors).
    pub references: ReferenceReport,
}

/// Run the check validation over an already-loaded config and model.
pub fn validate_current_check(
    root: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<ValidatedCheck, BuildError> {
    let (specs, renderer) = crate::build::validated_plan_for_check(root, config, model)?;
    let references = crate::link_check::validate_references(config, model, &specs)?;
    // The loaded renderer is part of validation (unknown-template and
    // invalid-template failures surface here, as in a build). It is
    // otherwise unused by `check`, which resolves nothing.
    let _ = &renderer;
    Ok(ValidatedCheck {
        specs,
        renderer,
        references,
    })
}

/// Load config, ingest, and run the canonical pre-execution pipeline.
///
/// Returns the config and model alongside the validated build state so
/// callers that need them for planning or reporting do not reload.
pub struct LoadedSite {
    /// Parsed configuration.
    pub config: SignalConfig,
    /// Frozen site model.
    pub model: SiteModel,
    /// Draft sources skipped during ingest.
    pub drafts_skipped: usize,
    /// Validated specs, renderer, and reference report.
    pub validated: ValidatedBuild,
}

/// One call covering the whole read-only prefix: config → ingest →
/// structural validation → reference validation.
pub fn load_validated_site(root: &Path, out_dir: &Path) -> Result<LoadedSite, BuildError> {
    let config = load_config_from_disk(root)?;
    let (model, drafts_skipped) = ingest_from_disk(root, &config)?;
    let validated = validate_current_site(root, out_dir, &config, &model)?;
    Ok(LoadedSite {
        config,
        model,
        drafts_skipped,
        validated,
    })
}

/// One call covering the whole `check` prefix: config → ingest →
/// structural validation → reference validation, without an output
/// directory.
pub struct LoadedCheck {
    /// Parsed configuration.
    pub config: SignalConfig,
    /// Frozen site model.
    pub model: SiteModel,
    /// Validated specs, renderer, and reference report.
    pub validated: ValidatedCheck,
}

/// Load config, ingest, and run the check validation.
pub fn load_validated_check(root: &Path) -> Result<LoadedCheck, BuildError> {
    let config = load_config_from_disk(root)?;
    let (model, _) = ingest_from_disk(root, &config)?;
    let validated = validate_current_check(root, &config, &model)?;
    Ok(LoadedCheck {
        config,
        model,
        validated,
    })
}
