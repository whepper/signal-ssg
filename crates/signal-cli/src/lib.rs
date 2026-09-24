//! `signal-cli`: CLI boundary, config loading, and build orchestration.
//!
//! Filesystem access lives here, never in `signal-core`. The canonical
//! pre-execution pipeline is fixed (see [`pipeline`]):
//!
//! ```text
//! load config -> ingest -> freeze SiteModel -> validated specs
//!   -> reference validation -> incremental BuildPlan decisions
//!   -> execute (reuse or resolve+write) -> prune stale outputs
//!   -> atomically replace the manifest
//! ```
//!
//! `build` runs the whole pipeline; `serve` reuses it per rebuild;
//! `check` and `build --explain` stop early but never skip an upstream
//! validation stage.
//!
//! Planning (`build_plan`) determines what should happen; `build` executes
//! those decisions without redefining the dependency contract.
//!
//! Incremental reuse, generation compatibility, template-set invalidation,
//! and stale pruning are documented in `ARCHITECTURE.md` §13 and
//! `docs/adr/0016`–`0020`.

#![forbid(unsafe_code)]

pub mod assets;
pub(crate) mod body_html;
pub mod build;
pub mod build_plan;
pub mod config;
pub mod diagnostics;
pub mod discover;
pub mod errors;
pub mod explain;
pub mod git;
pub mod images;
pub mod ingest;
pub mod inspect;
pub mod link_check;
pub mod manifest;
pub mod pipeline;
pub mod responsive;
pub mod serve;
pub mod social;

pub use build::{
    build_site, build_site_from_disk, load_templates, resolve_artifact, BuildSummary,
    PipelineStage, SpecPlan,
};
pub use build_plan::{BuildDecision, BuildPlan, RebuildReason};
pub use config::load_config_from_file;
pub use errors::BuildError;
