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
    /// Generated image derivative (A2, ADR 0029): a resized WebP rendering
    /// of a content-referenced raster source, e.g. `images/hero-640.webp`
    /// derived from `static/images/hero.jpg`.
    ///
    /// Unlike [`ArtifactKind::Static`], output bytes never equal source
    /// bytes: the spec's `(source, width, format)` identity resolves
    /// through the image producer (`signal-cli::images`), and reuse
    /// compares the *source* digest (via the manifest `assets` map) while
    /// parameters ride the input reference itself.
    DerivedImage,
    /// Generated social card (A5, ADR 0032): one deterministic PNG per
    /// participating entry page, rendered from page metadata plus an
    /// optional hero image, e.g. `social/posts/example.png`.
    ///
    /// Identity is the page's route, inverted from the output path
    /// (`signal_core::social_image_route`), exactly as `DerivedImage`
    /// inverts its own path. Inputs are the page's entry digest, the
    /// configuration (dimensions, site identity), and the hero source
    /// bytes when one is composited — never the page's HTML digest.
    SocialImage,
    /// `robots.txt`, derived from configuration (base URL). Not a route:
    /// crawlers fetch it at a fixed path, so it is planned and resolved
    /// like the sitemap but with its own content rules.
    Robots,
    /// The themed not-found page (`404.html`). Rendered through the
    /// template pipeline from site-level context, but deliberately routeless:
    /// the hosting layer serves it for unknown paths, so it carries no
    /// canonical URL and no active menu state.
    NotFound,
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
