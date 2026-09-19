//! Configuration loading boundary (TOML + Serde).
//!
//! Pure parsing lives in `signal-core::SignalConfig`; this module adds the
//! filesystem read. Diagnostics surface as [`CliError`] with `miette`.

use miette::{Diagnostic, NamedSource};
use std::path::Path;
use thiserror::Error;

use crate::errors::BuildError;

/// CLI-facing errors with diagnostic presentation.
#[derive(Debug, Error, Diagnostic)]
pub enum CliError {
    /// `signal.toml` could not be read.
    #[error("could not read config {path:?}: {message}")]
    #[diagnostic(code(signal::config::read))]
    ConfigRead {
        /// Config path.
        path: String,
        /// OS message.
        message: String,
    },

    /// `signal.toml` did not parse.
    #[error("invalid config {path:?}: {message}")]
    #[diagnostic(code(signal::config::parse))]
    ConfigParse {
        /// Config path (or `<inline>` for strings).
        path: String,
        /// Owned source for snippet display.
        #[source_code]
        src: NamedSource<String>,
        /// Parser message.
        message: String,
    },
}

/// Load and parse `signal.toml` from disk.
pub fn load_config_from_file(path: &Path) -> Result<signal_core::SignalConfig, CliError> {
    let text = std::fs::read_to_string(path).map_err(|e| CliError::ConfigRead {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    parse_config_str(&text, &path.display().to_string())
}

/// Build a [`BuildError::Config`] from a TOML parse failure (shared with
/// the build orchestration so `signal.toml` errors are identical everywhere).
pub fn config_parse_error(path: &Path, text: &str, err: toml::de::Error) -> BuildError {
    BuildError::Config {
        path: path.display().to_string(),
        src: NamedSource::new(path.display().to_string(), text.to_string()),
        message: err.to_string(),
    }
}

/// Parse configuration from a string (used by tests and by file loading).
pub fn parse_config_str(text: &str, origin: &str) -> Result<signal_core::SignalConfig, CliError> {
    toml::from_str(text).map_err(|e| CliError::ConfigParse {
        path: origin.to_string(),
        src: NamedSource::new(origin, text.to_string()),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_string() {
        let cfg = parse_config_str("[site]\ntitle = \"T\"\n", "<inline>").expect("parses");
        assert_eq!(cfg.site.title, "T");
    }

    #[test]
    fn invalid_config_is_diagnostic() {
        let err = parse_config_str("not = [valid", "<inline>").expect_err("must fail");
        let msg = format!("{err}");
        assert!(msg.contains("invalid config"), "got: {msg}");
    }

    #[test]
    fn loads_fixture_config_from_disk() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/minimal-site/signal.toml"
        );
        let path = Path::new(fixture);
        // The fixture is expected to exist. Missing fixture fails loudly so
        // scaffolding gaps surface.
        let cfg = load_config_from_file(path).expect("fixture config loads");
        assert!(!cfg.site.title.is_empty());
    }

    #[test]
    fn missing_file_reports_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("signal.toml");
        let err = load_config_from_file(&missing).expect_err("must fail");
        assert!(matches!(err, CliError::ConfigRead { .. }));
    }
}
