//! Build-time errors with source-location diagnostics.
//!
//! Every variant carries the offending file path; parse failures additionally
//! carry the file contents as [`NamedSource`] so `miette` renders a snippet.

use miette::{Diagnostic, NamedSource};
use std::path::Path;
use thiserror::Error;

/// Failure while ingesting, generating, rendering, or writing a site.
#[derive(Debug, Error, Diagnostic)]
pub enum BuildError {
    /// `signal.toml` did not parse.
    #[error("invalid config {path:?}: {message}")]
    #[diagnostic(code(signal::build::config))]
    Config {
        /// Config path.
        path: String,
        /// Owned source for snippet display.
        #[source_code]
        src: NamedSource<String>,
        /// Parser message.
        message: String,
    },

    /// A source or template file could not be read.
    #[error("could not read {path:?}: {message}")]
    #[diagnostic(code(signal::build::read))]
    Read {
        /// Offending path.
        path: String,
        /// OS message.
        message: String,
    },

    /// Front matter was missing or invalid.
    #[error("invalid front matter in {path:?}: {message}")]
    #[diagnostic(code(signal::build::front_matter))]
    FrontMatter {
        /// Offending path.
        path: String,
        /// Owned source for snippet display.
        #[source_code]
        src: NamedSource<String>,
        /// Parser message.
        message: String,
    },

    /// A required normalized field is absent or malformed.
    #[error("invalid content in {path:?}: {message}")]
    #[diagnostic(code(signal::build::content))]
    Content {
        /// Offending path.
        path: String,
        /// Owned source for snippet display.
        #[source_code]
        src: NamedSource<String>,
        /// What is wrong.
        message: String,
    },

    /// The frozen model failed validation (e.g. route collision).
    #[error("invalid site model: {message}")]
    #[diagnostic(code(signal::build::model))]
    Model {
        /// Validation message.
        message: String,
    },

    /// Template loading or rendering failed.
    #[error("render failed: {message}")]
    #[diagnostic(code(signal::build::render))]
    Render {
        /// What failed.
        message: String,
    },

    /// An output file could not be written.
    #[error("could not write {path:?}: {message}")]
    #[diagnostic(code(signal::build::write))]
    Write {
        /// Destination path.
        path: String,
        /// OS message.
        message: String,
    },

    /// Two artifacts target the same output path.
    #[error("output path collision: {first:?} and {second:?} resolve to the same output")]
    #[diagnostic(code(signal::build::collision))]
    OutputCollision {
        /// First colliding output path.
        first: String,
        /// Second colliding output path. Equal to `first` for an exact
        /// logical-path duplicate; a distinct logical path for a filesystem
        /// alias (case-insensitive or Unicode-normalizing collision).
        second: String,
    },

    /// An internal reference points nowhere Signal generates.
    #[error("broken internal reference in {source_file:?} (route {route}): {target:?}: {reason}")]
    #[diagnostic(code(signal::build::reference))]
    Reference {
        /// Source file relative to the site root, e.g. `content/posts/a.md`.
        source_file: String,
        /// Canonical route of the containing entry, or the menu owner.
        route: String,
        /// The referenced target as authored.
        target: String,
        /// Why the target does not resolve.
        reason: String,
    },
}

/// Read a file, mapping IO failures to [`BuildError::Read`].
pub fn read_file(path: &Path) -> Result<String, BuildError> {
    std::fs::read_to_string(path).map_err(|e| BuildError::Read {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

/// Map a source/template discovery IO failure to [`BuildError::Read`].
///
/// Discovery is fail-closed: a directory that cannot be read must abort the
/// build rather than be mistaken for an empty source tree, because an
/// incomplete inventory would otherwise make current artifacts look stale and
/// trigger pruning of published output.
pub(crate) fn discovery_error(path: &Path, error: std::io::Error) -> BuildError {
    BuildError::Read {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
