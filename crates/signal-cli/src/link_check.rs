//! Structured internal reference validation.
//!
//! Validates author-controlled references against the in-memory inventory
//! of what the current build will generate — no rendered HTML parsing, no
//! filesystem scans for targets, no network requests:
//!
//! ```text
//! SiteModel + planned ArtifactSpecs + config
//!     → target inventory (routes, files, static assets, heading ids)
//!     → resolve RenderedBody.links/images, front-matter images, menu URLs
//!     → BuildError::Reference on the first broken reference
//! ```
//!
//! Covered: inline Markdown links/images (root-relative, document-relative,
//! fragment-only, query-only), front-matter images, internal menu targets.
//! External destinations (`http(s)`, `mailto:`, `tel:`, protocol-relative,
//! other schemes) are classified and skipped — validation and sanitization
//! are related but not identical concerns, and the build must never depend
//! on the network.
//!
//! Deliberate limitations (documented, not oversights):
//!
//! - Reference-style, shortcut, bare-URL, and `<autolink>` Markdown forms
//!   are rendered by Comrak but absent from `RenderedBody.links/images`
//!   (extraction is inline-form only by design); they are invisible here.
//! - Literal URLs inside MiniJinja templates are never parsed; only
//!   model-represented menu URLs are checked.
//! - Author raw HTML is detached at Markdown parse, so it contributes no
//!   references to this model.
//! - Front-matter `image` values are literal paths: `#`/`?` encode as path
//!   characters (see `image_src_url`), so no fragment/query split applies.
//! - Fragments are validated only against entry heading ids. Generated
//!   listing/taxonomy/feed pages may carry renderer-introduced anchors the
//!   model cannot see, so fragments there are not checked.
//! - `404.html` exists as an artifact but is routeless; referencing it is
//!   reported invalid rather than silently accepted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use signal_core::{ArtifactKind, ArtifactSpec, SignalConfig, SiteModel};

use crate::errors::BuildError;

/// Why one internal reference is broken. Stable strings for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceReason {
    /// The target is neither a known route nor a known generated file.
    TargetMissing,
    /// The reference cannot resolve (escapes the site root, or targets the
    /// routeless `404.html` artifact).
    InvalidReference,
    /// The target page exists but carries no such heading id.
    FragmentMissing,
}

impl std::fmt::Display for ReferenceReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceReason::TargetMissing => write!(f, "target does not exist"),
            ReferenceReason::InvalidReference => write!(f, "invalid internal reference"),
            ReferenceReason::FragmentMissing => write!(f, "fragment does not exist"),
        }
    }
}

/// What validation checked. Returned so `signal check` can report activity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceReport {
    /// Internal references resolved against the inventory.
    pub checked: usize,
    /// External destinations skipped without network access.
    pub external_skipped: usize,
}

/// The authoritative target inventory for one site state: every route and
/// file the current plan will generate, plus entry heading ids.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Inventory {
    /// Canonical logical routes (`/posts/a/`, `/`, `/topics/rust/`).
    routes: BTreeSet<String>,
    /// Output-relative file paths (`posts/a/index.html`, `index.xml`).
    files: BTreeSet<String>,
    /// Output-relative static asset paths (subset of `files`).
    static_files: BTreeSet<String>,
    /// Entry route → heading ids in document order.
    headings: BTreeMap<String, BTreeSet<String>>,
}

/// Build the inventory from the frozen model and the planned specs.
/// Pure in-memory union; no filesystem access.
fn build_inventory(model: &SiteModel, specs: &[ArtifactSpec]) -> Inventory {
    let mut routes = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut static_files = BTreeSet::new();
    for entry in model.entries() {
        routes.insert(entry.route.0.clone());
    }
    for spec in specs {
        files.insert(spec.path.clone());
        if let Some(route) = spec.route.as_ref() {
            routes.insert(route.0.clone());
        }
        if spec.kind == ArtifactKind::Static {
            static_files.insert(spec.path.clone());
        }
    }
    let mut headings = BTreeMap::new();
    for entry in model.entries() {
        headings.insert(
            entry.route.0.clone(),
            entry.body.headings.iter().map(|h| h.id.clone()).collect(),
        );
    }
    Inventory {
        routes,
        files,
        static_files,
        headings,
    }
}

/// A schemeless destination split into path and optional fragment.
/// Queries are dropped: they never affect target existence.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SplitTarget {
    path: String,
    fragment: Option<String>,
}

fn split_target(value: &str) -> SplitTarget {
    let (before_fragment, fragment) = match value.find('#') {
        Some(index) => (&value[..index], Some(value[index + 1..].to_string())),
        None => (value, None),
    };
    let path = match before_fragment.find('?') {
        Some(index) => &before_fragment[..index],
        None => before_fragment,
    };
    SplitTarget {
        path: path.to_string(),
        // An empty fragment (`/foo/#`) is no fragment per URL semantics.
        fragment: fragment.filter(|fragment| !fragment.is_empty()),
    }
}

/// Whether `value` starts with a URI scheme (`scheme:` before any `/`, `?`,
/// or `#`), mirroring the strictness of `split_scheme` in `signal-core`.
fn scheme_of(value: &str) -> Option<String> {
    let mut end = None;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b':' => {
                end = Some(index);
                break;
            }
            b'/' | b'?' | b'#' => break,
            _ => {}
        }
    }
    let end = end?;
    let scheme = &value[..end];
    if scheme.is_empty()
        || !scheme.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphabetic()
                || (i > 0 && (b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.'))
        })
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Percent-decode for comparison only (route matching, traversal
/// detection). Never re-encoded or executed: decoding cannot create a
/// scheme, and decoded `.`/`..` segments are rejected, not resolved.
fn percent_decode(value: &str) -> Option<String> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hex = |b: u8| (b as char).to_digit(16);
                out.push((hex(bytes[i + 1])? * 16 + hex(bytes[i + 2])?) as u8);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// How one destination classifies for validation.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Classification {
    /// Any `scheme:` destination, plus protocol-relative `//host/…`.
    /// Skipped: external, or neutralized before rendering.
    External,
    /// Schemeless destination: resolve against the inventory.
    Internal(SplitTarget),
}

fn classify(value: &str) -> Classification {
    let value = value.trim();
    // Protocol-relative URLs are external even though the HTML sanitizer
    // treats schemeless values as relative: validation and sanitization
    // answer different questions.
    if value.starts_with("//") {
        return Classification::External;
    }
    if scheme_of(value).is_some() {
        return Classification::External;
    }
    Classification::Internal(split_target(value))
}

/// Resolve an internal path to a canonical route. `source_route` is the
/// containing entry's route (`/docs/x/`); document-relative targets resolve
/// against its directory. `.` segments are ignored, `..` pops, and popping
/// above the site root is an error. Percent-decoded `.`/`..` are rejected
/// the same way (fail closed). URLs are logical site routes, never OS paths.
fn resolve_route(source_route: &str, target: &str) -> Result<String, ReferenceReason> {
    // Canonical Signal routes end in `/` (or are `/` itself), so the
    // route already names a directory in URL space: document-relative
    // targets resolve against its full segment list.
    let mut segments: Vec<String> = if target.starts_with('/') {
        Vec::new()
    } else {
        source_route
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect()
    };
    let absolute = target.starts_with('/');
    let rest = if absolute { &target[1..] } else { target };
    for segment in rest.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.pop().is_none() {
                return Err(ReferenceReason::InvalidReference);
            }
            continue;
        }
        // Traversal is checked on the decoded segment so `%2e%2e` cannot
        // smuggle a pop past the check; resolution itself uses the raw
        // segment to match Signal's verbatim route identity. A decoded-only
        // dot segment is rejected outright rather than resolved.
        if let Some(decoded) = percent_decode(segment) {
            if decoded != segment && (decoded == "." || decoded == "..") {
                return Err(ReferenceReason::InvalidReference);
            }
        }
        segments.push(segment.to_string());
    }
    let mut route = String::from("/");
    route.push_str(&segments.join("/"));
    if target.ends_with('/') && route != "/" {
        route.push('/');
    }
    Ok(route)
}

/// Whether a canonical route is generated. Tries the verbatim form first,
/// then the percent-decoded form, so authored encoded routes (`/caf%C3%A9/`)
/// match logical routes (`/café/`) without inventing normalization.
fn is_known_route(inventory: &Inventory, route: &str) -> bool {
    if inventory.routes.contains(route) {
        return true;
    }
    match percent_decode(route) {
        Some(decoded) => decoded != route && inventory.routes.contains(&decoded),
        None => false,
    }
}

/// Whether an output-relative file path is generated (same raw/decoded rule).
fn is_known_file(inventory: &Inventory, path: &str) -> bool {
    if inventory.files.contains(path) {
        return true;
    }
    match percent_decode(path) {
        Some(decoded) => decoded != path && inventory.files.contains(&decoded),
        None => false,
    }
}

/// Whether an output-relative static asset path exists.
fn is_known_static(inventory: &Inventory, path: &str) -> bool {
    if inventory.static_files.contains(path) {
        return true;
    }
    match percent_decode(path) {
        Some(decoded) => decoded != path && inventory.static_files.contains(&decoded),
        None => false,
    }
}

/// One broken reference, ready to become a `BuildError::Reference`.
struct BrokenReference {
    target: String,
    reason: ReferenceReason,
}

/// Check one Markdown link destination from `source_route`.
/// Returns the skip/valid/broken outcome; external destinations are skipped.
fn check_link(
    inventory: &Inventory,
    source_route: &str,
    raw: &str,
    report: &mut ReferenceReport,
) -> Option<BrokenReference> {
    let target = raw.trim().to_string();
    let split = match classify(&target) {
        Classification::External => {
            report.external_skipped += 1;
            return None;
        }
        Classification::Internal(split) => split,
    };
    // An empty destination (`[]()` renders a self link) trivially resolves,
    // as does a bare fragment or query against the source entry itself.
    let route = if split.path.is_empty() {
        source_route.to_string()
    } else {
        match resolve_route(source_route, &split.path) {
            Ok(route) => route,
            Err(reason) => {
                return Some(BrokenReference { target, reason });
            }
        }
    };
    report.checked += 1;
    if is_known_route(inventory, &route) {
        if let Some(fragment) = split.fragment {
            match inventory.headings.get(&route) {
                // Entry pages carry structural heading ids: strict check.
                Some(ids) if !ids.contains(&fragment) => {
                    return Some(BrokenReference {
                        target,
                        reason: ReferenceReason::FragmentMissing,
                    });
                }
                // Generated listing/taxonomy/feed pages may carry
                // renderer-introduced anchors the model cannot see:
                // fragments there are not checked.
                _ => {}
            }
        }
        return None;
    }
    let file = route.trim_start_matches('/');
    if is_known_file(inventory, file) {
        // `404.html` is an artifact but not a routable page: referencing it
        // is invalid rather than silently accepted.
        if file == "404.html" || percent_decode(file).as_deref() == Some("404.html") {
            return Some(BrokenReference {
                target,
                reason: ReferenceReason::InvalidReference,
            });
        }
        return None;
    }
    Some(BrokenReference {
        target,
        reason: ReferenceReason::TargetMissing,
    })
}

/// Check one Markdown image destination: images must resolve to static
/// assets, not pages. Fragments on asset paths are not checked.
fn check_image(
    inventory: &Inventory,
    source_route: &str,
    raw: &str,
    report: &mut ReferenceReport,
) -> Option<BrokenReference> {
    let target = raw.trim().to_string();
    let split = match classify(&target) {
        Classification::External => {
            report.external_skipped += 1;
            return None;
        }
        Classification::Internal(split) => split,
    };
    if split.path.is_empty() {
        report.checked += 1;
        return None;
    }
    let route = match resolve_route(source_route, &split.path) {
        Ok(route) => route,
        Err(reason) => {
            return Some(BrokenReference { target, reason });
        }
    };
    report.checked += 1;
    let path = route.trim_start_matches('/');
    if is_known_static(inventory, path) {
        return None;
    }
    Some(BrokenReference {
        target,
        reason: ReferenceReason::TargetMissing,
    })
}

/// Check one normalized front-matter image (`/images/a.svg` site-root form
/// or absolute URL). `#`/`?` are literal path characters here (they encode
/// via `image_src_url`), so no fragment/query split applies.
fn check_front_matter_image(
    inventory: &Inventory,
    image: &str,
    report: &mut ReferenceReport,
) -> Option<BrokenReference> {
    let target = image.trim().to_string();
    if target.starts_with("//") || scheme_of(&target).is_some() {
        report.external_skipped += 1;
        return None;
    }
    report.checked += 1;
    let path = target.trim_start_matches('/');
    if is_known_static(inventory, path) {
        return None;
    }
    Some(BrokenReference {
        target,
        reason: ReferenceReason::TargetMissing,
    })
}

/// Validate every structured internal reference in the current site state:
/// entry links/images, front-matter images, and internal menu targets.
/// Whole-site, deterministic (ContentId order, authored vec order, sorted
/// menu keys), first failure wins. Pure in-memory set membership.
pub fn validate_references(
    config: &SignalConfig,
    model: &SiteModel,
    specs: &[ArtifactSpec],
) -> Result<ReferenceReport, BuildError> {
    let inventory = build_inventory(model, specs);
    let mut report = ReferenceReport {
        checked: 0,
        external_skipped: 0,
    };
    for entry in model.entries() {
        let source = format!(
            "{}/{}",
            config.source_dir_for(&entry.collection.0),
            entry.source.relative_path
        );
        for link in &entry.body.links {
            if let Some(broken) = check_link(&inventory, &entry.route.0, link, &mut report) {
                return Err(BuildError::Reference {
                    source_file: source.clone(),
                    route: entry.route.0.clone(),
                    target: broken.target,
                    reason: broken.reason.to_string(),
                });
            }
        }
        for image in &entry.body.images {
            if let Some(broken) = check_image(&inventory, &entry.route.0, image, &mut report) {
                return Err(BuildError::Reference {
                    source_file: source.clone(),
                    route: entry.route.0.clone(),
                    target: broken.target,
                    reason: broken.reason.to_string(),
                });
            }
        }
        if let Some(image) = entry.image.as_deref() {
            if let Some(broken) = check_front_matter_image(&inventory, image, &mut report) {
                return Err(BuildError::Reference {
                    source_file: source.clone(),
                    route: entry.route.0.clone(),
                    target: broken.target,
                    reason: broken.reason.to_string(),
                });
            }
        }
    }
    // Internal menu targets. Shape validity is guaranteed by
    // validated_plan's resolve_main_menu gate on the build path; the same
    // pure resolution is reused here so stored URLs get identical
    // normalization (trailing slash) and encoding. Only existence is
    // checked. Only the rendered `main` menu has targets that can break;
    // non-main menus have no rendered representation.
    let resolved =
        signal_core::resolve_main_menu(&config.menus, &signal_core::Route::new("/".to_string()))
            .map_err(|err| BuildError::Model {
                message: format!("invalid menu: {err}"),
            })?;
    if let Some(items) = resolved {
        for item in &items {
            let url = item.url.trim();
            if url.starts_with("//") || scheme_of(url).is_some() {
                report.external_skipped += 1;
                continue;
            }
            report.checked += 1;
            if !is_known_route(&inventory, url) {
                return Err(BuildError::Reference {
                    source_file: "signal.toml".to_string(),
                    route: "menu:main".to_string(),
                    target: url.to_string(),
                    reason: ReferenceReason::TargetMissing.to_string(),
                });
            }
        }
    }
    Ok(report)
}

/// What `signal check` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckReport {
    /// Site title from configuration.
    pub title: String,
    /// Configured collection ids.
    pub collections: Vec<String>,
    /// Reference validation activity.
    pub references: ReferenceReport,
    /// Asset inventory activity (pure derivation over specs + model).
    pub assets: crate::assets::AssetReport,
    /// Advisory diagnostics over the publishing model (A6): evidence-based
    /// observations only. Never errors — a site that fails validation never
    /// reaches this report.
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
}

/// Load, ingest, structurally validate, and reference-validate without
/// building: the same config gates, spec generation, logical output-path
/// and route validation, and template loading as a build, plus the same
/// reference validation — but no artifacts resolved or written, nothing
/// pruned, manifest untouched.
///
/// The output-filesystem alias probe stays build/`--explain` territory: it
/// answers whether a specific output filesystem can represent the plan,
/// not whether the site is valid. Every other build rejection (invalid
/// config routes, menu shape, logical output collisions, route collisions,
/// template failures, broken references) surfaces here identically.
pub fn check_site_from_disk(root: &Path) -> Result<CheckReport, BuildError> {
    let loaded = crate::pipeline::load_validated_check(root)?;
    let assets = crate::assets::build_asset_report(&loaded.validated.specs, &loaded.model);
    // A6 diagnostics: the same analysis `explain` renders, derived from the
    // specs and model `check` just validated. Advisory and infallible.
    let diagnostics =
        crate::diagnostics::analyze(root, &loaded.config, &loaded.model, &loaded.validated.specs);
    Ok(CheckReport {
        title: loaded.config.site.title.clone(),
        collections: loaded
            .config
            .collection_ids()
            .iter()
            .map(|id| id.0.clone())
            .collect(),
        references: loaded.validated.references,
        assets,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{CollectionId, ContentId, Route, Slug, SourceRef};

    #[allow(clippy::too_many_arguments)]
    fn entry(
        id: u32,
        collection: &str,
        slug: &str,
        route: &str,
        links: &[&str],
        images: &[&str],
        headings: &[&str],
        image: Option<&str>,
    ) -> signal_core::ContentEntry {
        let mut entry = signal_core::ContentEntry::new(
            ContentId(id),
            CollectionId::new(collection),
            SourceRef::new(CollectionId::new(collection), format!("{slug}.md")),
            Slug::new(slug),
            Route::new(route.to_string()),
            format!("Title {slug}"),
        );
        entry.body.links = links.iter().map(|s| s.to_string()).collect();
        entry.body.images = images.iter().map(|s| s.to_string()).collect();
        entry.body.headings = headings
            .iter()
            .enumerate()
            .map(|(i, id)| signal_core::Heading {
                level: 2,
                text: format!("Heading {i}"),
                id: id.to_string(),
            })
            .collect();
        entry.image = image.map(str::to_string);
        entry
    }

    fn model() -> SiteModel {
        let mut builder = signal_core::SiteModelBuilder::new();
        builder.add_entry(entry(
            1,
            "posts",
            "alpha",
            "/posts/alpha/",
            &["/posts/beta/", "https://example.com/"],
            &["/images/a.svg"],
            &["install", "usage"],
            None,
        ));
        builder.add_entry(entry(
            2,
            "posts",
            "beta",
            "/posts/beta/",
            &[],
            &[],
            &[],
            None,
        ));
        builder.build().expect("builds")
    }

    fn specs() -> Vec<ArtifactSpec> {
        use signal_core::ArtifactKind;
        vec![
            ArtifactSpec::new("posts/alpha/index.html", ArtifactKind::Page)
                .with_route(Route::new("/posts/alpha/".to_string())),
            ArtifactSpec::new("posts/beta/index.html", ArtifactKind::Page)
                .with_route(Route::new("/posts/beta/".to_string())),
            ArtifactSpec::new("posts/index.html", ArtifactKind::CollectionIndex)
                .with_route(Route::new("/posts/".to_string())),
            ArtifactSpec::new("index.html", ArtifactKind::Home)
                .with_route(Route::new("/".to_string())),
            ArtifactSpec::new("topics/index.html", ArtifactKind::Taxonomy)
                .with_route(Route::new("/topics/".to_string())),
            ArtifactSpec::new("topics/rust/index.html", ArtifactKind::Taxonomy)
                .with_route(Route::new("/topics/rust/".to_string())),
            ArtifactSpec::new("index.xml", ArtifactKind::Rss),
            ArtifactSpec::new("posts/index.xml", ArtifactKind::Rss),
            ArtifactSpec::new("sitemap.xml", ArtifactKind::Sitemap),
            ArtifactSpec::new("robots.txt", ArtifactKind::Robots),
            ArtifactSpec::new("index.json", ArtifactKind::SearchIndex),
            ArtifactSpec::new("404.html", ArtifactKind::NotFound),
            ArtifactSpec::new("images/a.svg", ArtifactKind::Static),
        ]
    }

    fn config() -> SignalConfig {
        SignalConfig::from_toml_str(
            "[site]\ntitle = \"T\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        )
        .expect("config parses")
    }

    fn inventory() -> Inventory {
        build_inventory(&model(), &specs())
    }

    #[test]
    fn target_resolution_covers_relative_forms() {
        // Root-relative.
        assert_eq!(
            resolve_route("/posts/alpha/", "/posts/beta/"),
            Ok("/posts/beta/".to_string())
        );
        // Document-relative against the route directory.
        assert_eq!(
            resolve_route("/docs/x/", "../concepts/"),
            Ok("/docs/concepts/".to_string())
        );
        assert_eq!(
            resolve_route("/posts/alpha/", "beta/"),
            Ok("/posts/alpha/beta/".to_string())
        );
        // Parent-relative collapsing.
        assert_eq!(
            resolve_route("/a/b/c/", "../../d/"),
            Ok("/a/d/".to_string())
        );
        // Dot segments are ignored.
        assert_eq!(
            resolve_route("/posts/alpha/", "./beta/"),
            Ok("/posts/alpha/beta/".to_string())
        );
        // Traversal above the root fails closed.
        assert_eq!(
            resolve_route("/posts/alpha/", "../../../x/"),
            Err(ReferenceReason::InvalidReference)
        );
        // Encoded traversal fails closed without resolving.
        assert_eq!(
            resolve_route("/posts/alpha/", "%2e%2e/%2e%2e/x/"),
            Err(ReferenceReason::InvalidReference)
        );
        // File-like targets keep no trailing slash.
        assert_eq!(
            resolve_route("/posts/alpha/", "/ARCHITECTURE.md"),
            Ok("/ARCHITECTURE.md".to_string())
        );
    }

    #[test]
    fn classification_separates_internal_from_external() {
        assert!(matches!(classify("/posts/a/"), Classification::Internal(_)));
        assert!(matches!(classify("../a/"), Classification::Internal(_)));
        assert!(matches!(classify("#sec"), Classification::Internal(_)));
        assert!(matches!(classify("?x=1"), Classification::Internal(_)));
        for external in [
            "https://example.com/x/",
            "http://example.com/",
            "mailto:a@b.c",
            "tel:+3112345678",
            "//cdn.example.com/f.js",
            "javascript:alert(1)",
            "data:text/html,x",
            "ftp://host/f",
        ] {
            assert!(
                matches!(classify(external), Classification::External),
                "{external}"
            );
        }
    }

    #[test]
    fn target_query_and_fragment_split() {
        let split = split_target("/foo/?page=2#sec");
        assert_eq!(split.path, "/foo/");
        assert_eq!(split.fragment.as_deref(), Some("sec"));
        // Queries alone never affect existence.
        let split = split_target("?page=2");
        assert_eq!(split.path, "");
        assert_eq!(split.fragment, None);
        // An empty fragment is no fragment.
        let split = split_target("/foo/#");
        assert_eq!(split.path, "/foo/");
        assert_eq!(split.fragment, None);
    }

    #[test]
    fn known_routes_files_and_static_resolve() {
        let inventory = inventory();
        for route in ["/posts/alpha/", "/posts/", "/", "/topics/", "/topics/rust/"] {
            assert!(is_known_route(&inventory, route), "{route}");
        }
        // Encoded form matches the logical route.
        assert!(is_known_route(&inventory, "/topics/rust/"));
        assert!(!is_known_route(&inventory, "/nope/"));
        // Non-canonical missing slash is not a route…
        assert!(!is_known_route(&inventory, "/posts/beta"));
        // …but generated files resolve as files.
        assert!(is_known_file(&inventory, "posts/beta/index.html"));
        assert!(is_known_file(&inventory, "index.xml"));
        assert!(is_known_file(&inventory, "sitemap.xml"));
        assert!(is_known_file(&inventory, "robots.txt"));
        assert!(is_known_file(&inventory, "index.json"));
        assert!(!is_known_file(&inventory, "missing.html"));
        assert!(is_known_static(&inventory, "images/a.svg"));
        assert!(!is_known_static(&inventory, "images/missing.svg"));
        // Pages are not static assets.
        assert!(!is_known_static(&inventory, "posts/alpha/index.html"));
    }

    #[test]
    fn links_validate_against_routes_and_files() {
        let inventory = inventory();
        let mut report = ReferenceReport {
            checked: 0,
            external_skipped: 0,
        };
        // Content page, section, home, taxonomy, feed, sitemap, static file.
        for target in [
            "/posts/beta/",
            "/posts/",
            "/",
            "/topics/rust/",
            "/index.xml",
            "/sitemap.xml",
            "/images/a.svg",
            "/posts/beta/?page=2",
            "https://example.com/",
            "mailto:a@b.c",
        ] {
            assert!(
                check_link(&inventory, "/posts/alpha/", target, &mut report).is_none(),
                "{target}"
            );
        }
        // Missing route.
        assert_eq!(
            check_link(&inventory, "/posts/alpha/", "/nope/", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::TargetMissing
        );
        // Non-canonical form is not silently normalized.
        assert_eq!(
            check_link(&inventory, "/posts/alpha/", "/posts/beta", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::TargetMissing
        );
        // The routeless 404 artifact is invalid as a reference.
        assert_eq!(
            check_link(&inventory, "/posts/alpha/", "/404.html", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::InvalidReference
        );
        // Escape above the root is invalid.
        assert_eq!(
            check_link(&inventory, "/posts/alpha/", "../../../x/", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::InvalidReference
        );
    }

    #[test]
    fn fragments_validate_against_entry_headings() {
        let inventory = inventory();
        let mut report = ReferenceReport {
            checked: 0,
            external_skipped: 0,
        };
        assert!(check_link(
            &inventory,
            "/posts/alpha/",
            "/posts/alpha/#install",
            &mut report
        )
        .is_none());
        assert_eq!(
            check_link(
                &inventory,
                "/posts/alpha/",
                "/posts/alpha/#missing",
                &mut report
            )
            .expect("broken")
            .reason,
            ReferenceReason::FragmentMissing
        );
        // Local fragments resolve against the source entry.
        assert!(check_link(&inventory, "/posts/alpha/", "#usage", &mut report).is_none());
        assert_eq!(
            check_link(&inventory, "/posts/alpha/", "#missing", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::FragmentMissing
        );
        // Entries without headings reject fragments.
        assert_eq!(
            check_link(&inventory, "/posts/beta/", "#anything", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::FragmentMissing
        );
        // Fragments on generated listing pages are not checked (renderer
        // may introduce anchors the model cannot see).
        assert!(check_link(
            &inventory,
            "/posts/alpha/",
            "/topics/#anything",
            &mut report
        )
        .is_none());
    }

    #[test]
    fn images_must_be_static_assets() {
        let inventory = inventory();
        let mut report = ReferenceReport {
            checked: 0,
            external_skipped: 0,
        };
        assert!(check_image(&inventory, "/posts/alpha/", "/images/a.svg", &mut report).is_none());
        assert!(check_image(
            &inventory,
            "/posts/alpha/",
            "https://example.com/a.png",
            &mut report
        )
        .is_none());
        assert_eq!(
            check_image(
                &inventory,
                "/posts/alpha/",
                "/images/missing.svg",
                &mut report
            )
            .expect("broken")
            .reason,
            ReferenceReason::TargetMissing
        );
        // Pages are not image targets.
        assert_eq!(
            check_image(&inventory, "/posts/alpha/", "/posts/beta/", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::TargetMissing
        );
    }

    #[test]
    fn front_matter_images_are_literal_paths() {
        let inventory = inventory();
        let mut report = ReferenceReport {
            checked: 0,
            external_skipped: 0,
        };
        // `#`/`?` are literal path characters for front-matter images.
        assert!(check_front_matter_image(&inventory, "/images/a.svg", &mut report).is_none());
        assert!(check_front_matter_image(&inventory, "images/a.svg", &mut report).is_none());
        assert!(
            check_front_matter_image(&inventory, "https://example.com/a.png", &mut report)
                .is_none()
        );
        assert_eq!(
            check_front_matter_image(&inventory, "/images/missing.svg", &mut report)
                .expect("broken")
                .reason,
            ReferenceReason::TargetMissing
        );
    }

    #[test]
    fn whole_site_validation_reports_first_failure_deterministically() {
        // Alpha's references all resolve: valid site passes with counts.
        let report = validate_references(&config(), &model(), &specs()).expect("valid");
        assert!(report.checked > 0);
        assert!(report.external_skipped > 0);
        // Deterministic: same inputs, same report.
        let again = validate_references(&config(), &model(), &specs()).expect("valid");
        assert_eq!(report, again);
    }

    #[test]
    fn whole_site_validation_fails_with_actionable_error() {
        let mut builder = signal_core::SiteModelBuilder::new();
        builder.add_entry(entry(
            1,
            "posts",
            "alpha",
            "/posts/alpha/",
            &["/projects/foo/"],
            &[],
            &[],
            None,
        ));
        let model = builder.build().expect("builds");
        let err = validate_references(&config(), &model, &specs()).expect_err("broken");
        match err {
            BuildError::Reference {
                source_file,
                route,
                target,
                reason,
            } => {
                assert_eq!(source_file, "content/posts/alpha.md");
                assert_eq!(route, "/posts/alpha/");
                assert_eq!(target, "/projects/foo/");
                assert_eq!(reason, "target does not exist");
            }
            other => panic!("wrong error: {other:?}"),
        }
    }
}
