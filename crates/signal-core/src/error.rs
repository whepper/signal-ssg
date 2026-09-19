//! Core diagnostics for `signal-core`.

use thiserror::Error;

/// Errors produced while building or validating the [`crate::SiteModel`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    /// Two entries claimed the same [`crate::ContentId`].
    #[error("duplicate content id: {0}")]
    DuplicateContentId(u32),

    /// Two entries claimed the same output route.
    #[error("route collision on {route:?}: {first} and {second}")]
    RouteCollision {
        /// Colliding route.
        route: String,
        /// First claimant.
        first: String,
        /// Second claimant.
        second: String,
    },

    /// Two entries claimed the same `(collection, source-relative-path)`.
    #[error("duplicate source {relative_path:?} in collection {collection:?}")]
    DuplicateSource {
        /// Owning collection.
        collection: String,
        /// Relative source path.
        relative_path: String,
    },

    /// A semantic reference points at an unknown [`crate::ContentId`].
    #[error("unknown reference from content {from} to content {to}")]
    UnknownReference {
        /// Referencing entry.
        from: u32,
        /// Missing target.
        to: u32,
    },

    /// Generic validation failure.
    #[error("invalid content: {0}")]
    Invalid(String),
}
