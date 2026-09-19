//! `signal-cli`: CLI boundary, config loading, and build orchestration.
//!
//! Filesystem access lives here, never in `signal-core`. The build pipeline
//! lifecycle is fixed:
//!
//! ```text
//! ingest -> validate/normalize -> freeze SiteModel -> generate ArtifactSpecs
//!   -> validate routes -> plan/validate outputs -> reuse or resolve+write
//!   -> prune stale outputs -> atomically replace the manifest
//! ```
//!
//! Incremental reuse, generation compatibility, template-set invalidation,
//! and stale pruning are documented in `ARCHITECTURE.md` §13 and
//! `docs/adr/0016`–`0020`.

#![forbid(unsafe_code)]

pub mod build;
pub mod config;
pub mod discover;
pub mod errors;
pub mod git;
pub mod ingest;
pub mod manifest;

pub use build::{
    build_site, build_site_from_disk, load_templates, resolve_artifact, BuildPlan, BuildSummary,
    PipelineStage,
};
pub use config::load_config_from_file;
pub use errors::BuildError;
