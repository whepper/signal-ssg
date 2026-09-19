//! Lightweight artifact specifications.
//!
//! Generators emit [`ArtifactSpec`] values (path + kind). Rendering and
//! artifact resolution happen later, one artifact at a time, so large sites
//! never need a giant in-memory `Artifact { content: Vec<u8> }` map.
//!
//! Content bytes and build dependency tracking live in the build manifest,
//! not here; future content-representation variants (`Bytes` / `Source` /
//! `Staged`) would too.

use serde::{Deserialize, Serialize};

use crate::ids::Route;

/// Kind of output a generator plans to produce.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ArtifactKind {
    /// A single content page.
    Page,
    /// A collection section listing.
    CollectionIndex,
    /// The home page.
    Home,
    /// A taxonomy projection (e.g. tag archive).
    Taxonomy,
    /// RSS feed.
    Rss,
    /// Sitemap.
    Sitemap,
    /// Full-text search index (JSON).
    SearchIndex,
    /// Static passthrough (copied verbatim from `static/`).
    Static,
}

/// Lightweight plan for one output file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSpec {
    /// Output path relative to the output root, e.g. `posts/hello-world/index.html`.
    pub path: String,
    /// What kind of projection this is.
    pub kind: ArtifactKind,
    /// Route this artifact was derived from, if any.
    #[serde(default)]
    pub route: Option<Route>,
}

impl ArtifactSpec {
    /// Create a spec for an output path and kind.
    pub fn new(path: impl Into<String>, kind: ArtifactKind) -> Self {
        Self {
            path: path.into(),
            kind,
            route: None,
        }
    }

    /// Attach the originating route.
    pub fn with_route(mut self, route: Route) -> Self {
        self.route = Some(route);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_carries_no_content_bytes() {
        let spec = ArtifactSpec::new("posts/hello-world/index.html", ArtifactKind::Page);
        // Compile-time guarantee by construction: there is no `content` field.
        // Runtime check keeps the invariant visible: serialized form has no bytes.
        let json = serde_json::to_string(&spec).expect("serializes");
        assert!(!json.contains("content"));
    }
}
