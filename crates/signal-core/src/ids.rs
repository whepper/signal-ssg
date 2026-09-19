//! Typed identities for Signal.
//!
//! `ContentId`, source path, slug, and route are deliberately distinct types.
//! See `docs/adr/0001-site-model.md` for rationale.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque, builder-assigned identifier for a normalized content entry.
///
/// Boring by design: the builder assigns monotonically
/// increasing `u32` values in deterministic insertion order. The default
/// *stable* identity concept is `(collection, source-relative-path)`;
/// `ContentId` is the in-memory handle, not a persistent hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentId(pub u32);

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "content-{}", self.0)
    }
}

/// Collection membership, e.g. `posts`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CollectionId(pub String);

impl CollectionId {
    /// Create a collection id from anything string-like.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// URL-friendly human-readable label, e.g. `hello-world`.
///
/// A slug is not an identity and not a route.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Slug(pub String);

impl Slug {
    /// Create a slug from anything string-like.
    pub fn new(slug: impl Into<String>) -> Self {
        Self(slug.into())
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Output route, e.g. `/posts/hello-world/`.
///
/// Routes must be unique across the frozen [`crate::SiteModel`].
/// Route collisions are a validation error, not silently resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Route(pub String);

impl Route {
    /// Create a route from anything string-like.
    pub fn new(route: impl Into<String>) -> Self {
        Self(route.into())
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Source identity: `(collection, source-relative-path)`.
///
/// This is the default stable identity concept. It is distinct from
/// [`ContentId`] (in-memory handle), [`Slug`], and [`Route`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceRef {
    /// Owning collection.
    pub collection: CollectionId,
    /// Path relative to the collection source root, using `/` separators.
    pub relative_path: String,
}

impl SourceRef {
    /// Create a source reference.
    pub fn new(collection: CollectionId, relative_path: impl Into<String>) -> Self {
        Self {
            collection,
            relative_path: relative_path.into(),
        }
    }
}

/// Normalize a taxonomy display label to its URL slug.
///
/// Rules (verified against Hugo's `urlize` for every live term on the
/// migrated site): lowercase ASCII alphanumerics kept verbatim; every other
/// run of characters becomes a single `-`; leading/trailing `-` trimmed.
/// Display labels keep their original case; only the URL form is folded.
/// An empty result stays empty (callers decide whether that is an error).
///
/// ```rust
/// # use signal_core::slugify;
/// assert_eq!(slugify("Hello World"), "hello-world");
/// assert_eq!(slugify("MixedCASE  Words"), "mixedcase-words");
/// ```
pub fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_dash = false;
    for ch in value.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch);
        } else if !out.is_empty() {
            pending_dash = true;
        }
    }
    out
}

/// Validate a single route segment as a safe path component.
///
/// A segment must be non-empty and must not be `.` or `..`, contain `/` or
/// `\`, or contain whitespace/control characters. Unicode letters are fine —
/// fragments like `über-den-tellerrand` are valid segments.
///
/// This guards the semantic boundary: every segment that can become part of
/// an output filesystem path must be a safe, normalized component. Invalid
/// input is rejected with a diagnostic, never silently rewritten.
pub fn validate_route_segment(segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Err("route segment must not be empty".to_string());
    }
    if segment == "." || segment == ".." {
        return Err(format!("route segment must not be {segment:?}"));
    }
    if segment.contains('/') {
        return Err("route segment must not contain `/`".to_string());
    }
    if segment.contains('\\') {
        return Err("route segment must not contain `\\`".to_string());
    }
    if segment.chars().any(char::is_whitespace) {
        return Err("route segment must not contain whitespace".to_string());
    }
    if segment.chars().any(|c| c.is_control()) {
        return Err("route segment must not contain control characters".to_string());
    }
    Ok(())
}

/// Validate a full internal route (`/a/b/`).
///
/// The route must start with `/`, must not contain empty interior segments
/// (`//`) or backslashes, and every segment must pass
/// [`validate_route_segment`]. A single trailing slash is the canonical
/// form and is always allowed.
pub fn validate_route(route: &str) -> Result<(), String> {
    if !route.starts_with('/') {
        return Err(format!("route {route:?} must start with `/`"));
    }
    if route.contains('\\') {
        return Err(format!("route {route:?} must not contain `\\`"));
    }
    if route.contains("//") {
        return Err(format!("route {route:?} must not contain empty segments"));
    }
    for segment in route.split('/').filter(|s| !s.is_empty()) {
        validate_route_segment(segment)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn identity_types_are_distinct() {
        // ContentId != Slug != Route must hold at the type level, not just
        // by string value. If someone collapses these into `String`, this fails.
        assert_ne!(TypeId::of::<ContentId>(), TypeId::of::<Slug>());
        assert_ne!(TypeId::of::<ContentId>(), TypeId::of::<Route>());
        assert_ne!(TypeId::of::<Slug>(), TypeId::of::<Route>());
        assert_ne!(TypeId::of::<SourceRef>(), TypeId::of::<Route>());
    }

    #[test]
    fn display_does_not_conflate_identities() {
        let id = ContentId(1);
        let slug = Slug::new("hello-world");
        let route = Route::new("/posts/hello-world/");
        assert_ne!(id.to_string(), slug.to_string());
        assert_ne!(id.to_string(), route.to_string());
        assert_ne!(slug.to_string(), route.to_string());
    }

    #[test]
    fn slugify_folds_labels_to_url_slugs() {
        // Representative shapes: multi-word mixed case, single word,
        // already-slug input.
        let cases = [
            ("Hello World", "hello-world"),
            ("MixedCASE Words", "mixedcase-words"),
            ("Identity", "identity"),
            ("already-slug", "already-slug"),
        ];
        for (label, slug) in cases {
            assert_eq!(slugify(label), slug, "label {label:?}");
        }
    }

    #[test]
    fn slugify_folds_runs_and_trims() {
        assert_eq!(slugify("  Hello,  World!  "), "hello-world");
        assert_eq!(slugify("a--b__c"), "a-b-c");
        assert_eq!(slugify("already-slug"), "already-slug");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn route_segments_accept_safe_names() {
        for ok in [
            "posts",
            "hello-world",
            "2026-09-02-my-post",
            "über-alles",
            "_index",
        ] {
            assert_eq!(validate_route_segment(ok), Ok(()), "segment {ok:?}");
        }
    }

    #[test]
    fn route_segments_reject_traversal_and_separators() {
        for bad in [
            "", ".", "..", "a/../b", "a/b", "a\\b", "a b", "a\tb", "a\nb",
        ] {
            assert!(
                validate_route_segment(bad).is_err(),
                "segment {bad:?} must be rejected"
            );
        }
        // `...` is a legal, contained filename on all platforms — allowed.
        assert_eq!(validate_route_segment("..."), Ok(()));
    }

    #[test]
    fn routes_validate_full_paths() {
        for ok in [
            "/",
            "/posts/",
            "/posts/hello/",
            "/docs/guides/setup/",
            "/posts/über/",
        ] {
            assert_eq!(validate_route(ok), Ok(()), "route {ok:?}");
        }
        for bad in [
            "posts/",
            "/posts//",
            "/posts/../",
            "/posts/./",
            "/a\\b/",
            "/posts/hello world/",
            "https://example.com/",
        ] {
            assert!(
                validate_route(bad).is_err(),
                "route {bad:?} must be rejected"
            );
        }
    }
}
