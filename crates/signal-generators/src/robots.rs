//! `robots.txt` projection from configuration.
//!
//! Content is a fixed allow-all policy for every crawler, plus a `Sitemap:`
//! reference when the sitemap is actually planned — the sitemap needs
//! `site.base_url`, so the reference is emitted under exactly that
//! condition and never points at an artifact that does not exist. No
//! timestamps, no request-time behavior, no per-agent rules (none are
//! exercised by any real site; a genuine requirement extends the
//! `[robots]` table first).

use signal_core::ArtifactKind;
use signal_core::ArtifactSpec;

use crate::{GenerateError, Generator};

/// Render the complete robots.txt body (always newline-terminated).
pub fn robots_txt(base_url: Option<&str>) -> String {
    let mut out = String::from("User-agent: *\nAllow: /\n");
    if let Some(base) = base_url.filter(|b| !b.trim().is_empty()) {
        out.push_str(&format!(
            "Sitemap: {}/sitemap.xml\n",
            base.trim_end_matches('/')
        ));
    }
    out
}

/// Projection: the single `robots.txt` artifact.
pub struct Robots;

impl Robots {
    /// Create the robots projection.
    pub fn new() -> Self {
        Self
    }
}

impl Default for Robots {
    fn default() -> Self {
        Self::new()
    }
}

impl Generator for Robots {
    fn name(&self) -> &str {
        "robots"
    }

    fn generate(
        &self,
        _model: &signal_core::SiteModel,
    ) -> Result<Vec<ArtifactSpec>, GenerateError> {
        Ok(vec![ArtifactSpec::new("robots.txt", ArtifactKind::Robots)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_all_policy_with_sitemap_when_base_url_set() {
        assert_eq!(
            robots_txt(Some("https://example.com/")),
            "User-agent: *\nAllow: /\nSitemap: https://example.com/sitemap.xml\n"
        );
        // Trailing-slash variants collapse to one canonical reference.
        assert_eq!(
            robots_txt(Some("https://example.com")),
            robots_txt(Some("https://example.com/"))
        );
    }

    #[test]
    fn no_sitemap_line_without_base_url() {
        // The sitemap is only planned with a base URL; robots must not
        // reference a missing artifact. Whitespace-only counts as absent.
        assert_eq!(robots_txt(None), "User-agent: *\nAllow: /\n");
        assert_eq!(robots_txt(Some("   ")), "User-agent: *\nAllow: /\n");
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(
            robots_txt(Some("https://example.com/")),
            robots_txt(Some("https://example.com/"))
        );
    }

    #[test]
    fn generator_plans_single_robots_artifact() {
        let model = signal_core::SiteModelBuilder::new().build().expect("empty");
        let specs = Robots::new().generate(&model).expect("generates");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "robots.txt");
        assert_eq!(specs[0].kind, ArtifactKind::Robots);
        assert!(specs[0].route.is_none(), "robots.txt is not a route");
    }
}
