//! Build orchestration: ingest → freeze → generate → plan → resolve/write →
//! prune → manifest.
//!
//! The lifecycle is intentionally a plain ordered enum, not a state machine:
//! ingest, validate/normalize, freeze, generate, validate routes,
//! render/resolve, write output. Reuse and stale pruning run inside the
//! render/write phase and are documented in `ARCHITECTURE.md` §13.
//!
//! Generators stay pure (`&SiteModel -> Vec<ArtifactSpec>`). Rendering and
//! filesystem writes happen here in [`build_site`], one artifact at a time,
//! so rendered pages never accumulate in memory. Templates are read from the
//! configured template root (`<root>/templates`) into
//! [`MiniJinjaRenderer`] as in-memory strings — the renderer itself never
//! touches the filesystem. RSS and sitemap artifacts bypass MiniJinja by
//! design (fixed schemas serialize cleaner through `quick-xml` event
//! writers); the generator-plans / resolve-writes boundary is unchanged.
//! A `static/` directory under the site root is planned as
//! [`ArtifactKind::Static`] specs and resolved through the same pipeline.
//!
//! Every output file corresponds to an [`ArtifactSpec`], every spec can be
//! resolved independently via [`resolve_artifact`], and all output paths are
//! contained within the output directory (see `docs/adr/0014-*.md`).

use signal_core::{ArtifactKind, ArtifactSpec, Route, SignalConfig, SiteModel};
use signal_generators::{
    collection_summaries, reading_time_minutes, topic_terms, EntryPages, EntrySummary, Generator,
    Home, MainFeed, Search, SectionIndex, Sitemap, TaxonomyFeeds, TopicSummary, TopicTerms,
    TopicsIndex,
};
use signal_render::{MiniJinjaRenderer, RenderContext, Renderer};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::discover::write_artifact;
use crate::errors::{discovery_error, read_file, BuildError};
use crate::ingest::{ingest_site, path_relative_unix};
use crate::manifest::{
    collection_for_section_route, feed_limit, home_template, section_template_for_collection,
    taxonomy_root, taxonomy_title, template_for_collection, topic_term_template,
    topics_index_template, HOME_RECENT_LIMIT,
};

/// Fixed pipeline lifecycle (ordered, no transitions-encoded state machine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipelineStage {
    /// Read sources from disk.
    Ingest,
    /// Validate front-matter and normalize into owned data.
    ValidateNormalize,
    /// Freeze the immutable [`signal_core::SiteModel`].
    Freeze,
    /// Run generators to plan [`ArtifactSpec`] values.
    Generate,
    /// Ensure routes are unique and well-formed.
    ValidateRoutes,
    /// Render templates and resolve artifact contents.
    RenderResolve,
    /// Write output files.
    WriteOutput,
}

impl PipelineStage {
    /// All stages in execution order.
    pub fn ordered() -> Vec<PipelineStage> {
        use PipelineStage::{
            Freeze, Generate, Ingest, RenderResolve, ValidateNormalize, ValidateRoutes, WriteOutput,
        };
        vec![
            Ingest,
            ValidateNormalize,
            Freeze,
            Generate,
            ValidateRoutes,
            RenderResolve,
            WriteOutput,
        ]
    }
}

/// Planned build: site root, output dir, and artifact specs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildPlan {
    /// Site root (contains `signal.toml`).
    pub root: PathBuf,
    /// Output directory.
    pub out_dir: PathBuf,
    /// Planned artifacts.
    pub specs: Vec<ArtifactSpec>,
}

impl BuildPlan {
    /// Create a plan. Specs are sorted for deterministic output.
    pub fn new(root: PathBuf, out_dir: PathBuf, mut specs: Vec<ArtifactSpec>) -> Self {
        specs.sort_by(|a, b| a.path.cmp(&b.path));
        Self {
            root,
            out_dir,
            specs,
        }
    }

    /// Validate that no two specs target the same output path.
    ///
    /// Two checks, in order. First, exact logical-path duplicates are
    /// rejected without touching the filesystem. Second, the planned tree is
    /// replicated into a transient probe directory on the output filesystem
    /// ([`validate_filesystem_aliases`]): two distinct logical paths that
    /// resolve to the same filesystem object — a case-insensitive or
    /// Unicode-normalizing alias — are rejected before anything is written.
    /// Logical route identity is unchanged; the build simply refuses an
    /// output filesystem that cannot represent the plan distinctly.
    pub fn validate_output_paths(&self) -> Result<(), BuildError> {
        let mut seen = BTreeSet::new();
        for spec in &self.specs {
            if !seen.insert(&spec.path) {
                return Err(BuildError::OutputCollision {
                    first: spec.path.clone(),
                    second: spec.path.clone(),
                });
            }
        }
        validate_filesystem_aliases(&self.out_dir, &self.specs)
    }

    /// Validate that routes backing the specs are unique.
    pub fn validate_routes(&self) -> Result<(), signal_core::CoreError> {
        let mut seen = BTreeSet::new();
        for spec in &self.specs {
            if let Some(route) = &spec.route {
                if !seen.insert(route.clone()) {
                    return Err(signal_core::CoreError::RouteCollision {
                        route: route.0.clone(),
                        first: "artifact".to_string(),
                        second: "artifact".to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Convenience: check route uniqueness for an explicit route list.
pub fn validate_route_list(routes: &[Route]) -> Result<(), signal_core::CoreError> {
    let mut seen = BTreeSet::new();
    for route in routes {
        if !seen.insert(route.clone()) {
            return Err(signal_core::CoreError::RouteCollision {
                route: route.0.clone(),
                first: "route".to_string(),
                second: "route".to_string(),
            });
        }
    }
    Ok(())
}

/// Reject planned artifacts that alias on the output filesystem (slice 15B,
/// `docs/adr/0022-filesystem-alias-collisions.md`).
///
/// Logical output paths stay distinct — Signal never folds case or
/// normalizes Unicode in routes — but a build must fail when the configured
/// output filesystem cannot represent two planned paths distinctly
/// (`posts/Foo/index.html` vs `posts/foo/index.html` on a case-insensitive
/// volume, or NFC vs NFD `café` on a normalizing one). Writing both would
/// silently lose one artifact and record a manifest digest the filesystem
/// contradicts.
///
/// The check replicates the planned tree, in sorted order, into a transient
/// probe directory on the same filesystem as the outputs and reports the
/// first pair that resolves to one object. The filesystem itself is the
/// oracle, so no case-folding or Unicode-normalization tables are needed and
/// pre-existing hard links in the output tree can never confuse it: the
/// probe directory starts empty, so only planned paths participate.
/// Validation runs before any artifact is written, pruned, or recorded.
fn validate_filesystem_aliases(out_dir: &Path, specs: &[ArtifactSpec]) -> Result<(), BuildError> {
    if specs.len() < 2 {
        return Ok(());
    }
    let mut ordered: Vec<&str> = specs.iter().map(|spec| spec.path.as_str()).collect();
    ordered.sort();
    // The outputs will live here; ensuring the root exists creates empty
    // directories only (the first write does the same).
    std::fs::create_dir_all(out_dir).map_err(|e| BuildError::Write {
        path: out_dir.display().to_string(),
        message: e.to_string(),
    })?;
    let probe = AliasProbe::new(out_dir)?;
    // Planned paths already materialized in the probe, in probe order.
    let mut created: Vec<&str> = Vec::new();
    for path in ordered {
        // Absolute paths never reach the probe: `Path::join` would escape
        // it. They fail later at the contained write boundary, as before.
        if Path::new(path).is_absolute() {
            continue;
        }
        let target = probe.path().join(path);
        if let Some(parent) = target.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                // Directory creation is blocked when an earlier planned
                // path created one of these directories as a file.
                if let Some(partner) = blocking_owner(&probe, path, &created) {
                    return Err(filesystem_collision(partner, path));
                }
                return Err(BuildError::Write {
                    path: parent.display().to_string(),
                    message: "could not create probe directory".to_string(),
                });
            }
        }
        if target.exists() {
            // In a fresh probe directory this object was created by an
            // earlier planned path: a filesystem alias.
            let identity = crate::discover::file_identity(&target);
            let partner = created.iter().find(|earlier| {
                crate::discover::file_identity(&probe.path().join(earlier)) == identity
            });
            match partner {
                Some(partner) => return Err(filesystem_collision(partner, path)),
                None => {
                    return Err(BuildError::Write {
                        path: target.display().to_string(),
                        message: "probe object has no planned owner".to_string(),
                    })
                }
            }
        }
        std::fs::File::create(&target).map_err(|e| BuildError::Write {
            path: target.display().to_string(),
            message: e.to_string(),
        })?;
        created.push(path);
    }
    Ok(())
}

/// Find the earlier planned path whose probe object blocks directory
/// creation for `path`: walk `path`'s parent prefixes through the probe,
/// letting the filesystem resolve each one, and return the owner of the
/// first prefix that exists as a non-directory. Filesystem-resolved, so a
/// folded name (`A` blocking `a/b.txt`) is found exactly like an exact one.
fn blocking_owner<'a>(probe: &AliasProbe, path: &str, created: &[&'a str]) -> Option<&'a str> {
    let components: Vec<&str> = path.split('/').collect();
    for len in 1..components.len() {
        let prefix = components[..len].join("/");
        let probe_prefix = probe.path().join(&prefix);
        if probe_prefix.exists() && !probe_prefix.is_dir() {
            let identity = crate::discover::file_identity(&probe_prefix);
            return created
                .iter()
                .find(|earlier| {
                    crate::discover::file_identity(&probe.path().join(earlier)) == identity
                })
                .copied();
        }
    }
    None
}

/// A filesystem-alias collision between two distinct planned paths, ordered
/// deterministically (sorted) so the diagnostic is stable.
fn filesystem_collision(first: &str, second: &str) -> BuildError {
    let (first, second) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    BuildError::OutputCollision {
        first: first.to_string(),
        second: second.to_string(),
    }
}

/// Transient probe directory for filesystem-alias validation.
///
/// Created inside the output directory — hence on the same filesystem as the
/// outputs — and removed when validation finishes, including on early
/// return: [`Drop`] runs on every exit path. The name is hidden and unique
/// per process so it can never collide with a planned artifact, and probe
/// names never appear in diagnostics or build outputs.
struct AliasProbe {
    path: PathBuf,
}

impl AliasProbe {
    fn new(out_dir: &Path) -> Result<Self, BuildError> {
        // `create_dir`, not `create_dir_all`: a taken name must fail rather
        // than be silently reused.
        let path = out_dir.join(format!(
            ".signal-alias-probe-{}-{}",
            std::process::id(),
            PROBE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).map_err(|e| BuildError::Write {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AliasProbe {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static PROBE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Summary of a completed build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSummary {
    /// Pages rendered and written.
    pub pages_written: usize,
    /// Draft sources skipped during ingest.
    pub drafts_skipped: usize,
    /// Static files copied verbatim.
    pub static_files: usize,
    /// The complete set of artifacts the build produced. Every output file
    /// corresponds to one of these specs — this is the enumerable output
    /// contract the build manifest records.
    pub specs: Vec<ArtifactSpec>,
    /// Artifacts reused from the previous manifest without resolving.
    pub reused: usize,
    /// Artifacts resolved and written by this build.
    pub rebuilt: usize,
    /// Paths resolved and written by this build, in plan order (sorted).
    /// Reused artifacts are absent: this is the observable rebuild set.
    pub rebuilt_paths: Vec<String>,
    /// Stale artifacts pruned during this build.
    pub pruned: usize,
    /// Paths pruned during this build, sorted. Present in the previous
    /// manifest but absent from the current plan; runtime-only, never
    /// persisted in the manifest.
    pub pruned_paths: Vec<String>,
}

/// Load every `.html` file under `<root>/templates` into the renderer.
///
/// Template names are root-relative with `/` separators (e.g. `post.html`),
/// sorted for determinism. A missing directory means no templates (rendering
/// then fails with a useful unknown-template error); an unreadable directory
/// fails the build (see [`collect_templates`]). Public so tests can build a
/// renderer for isolated artifact resolution.
pub fn load_templates(root: &Path) -> Result<MiniJinjaRenderer, BuildError> {
    let mut renderer = MiniJinjaRenderer::new();
    let dir = root.join("templates");
    let mut files = Vec::new();
    collect_templates(&dir, &dir, &mut files)?;
    files.sort();
    for (name, path) in files {
        let source = read_file(&path)?;
        renderer
            .add_template(name.clone(), source)
            .map_err(|e| BuildError::Render {
                message: format!("invalid template {name:?}: {e}"),
            })?;
    }
    Ok(renderer)
}

/// Recursively collect `.html` templates, fail-closed.
///
/// Slice 15A (`docs/adr/0021-fail-closed-discovery.md`): a template directory
/// that cannot be read aborts the build instead of silently shrinking the
/// template set. The set digest is part of incremental invalidation, so a
/// partial walk must never look like a legitimate smaller set. A missing
/// template root is allowed (it is an empty set; rendering then fails with an
/// explicit unknown-template error).
///
/// Slice 15E: entry classification is equally fail-closed. `Path::is_dir()`
/// reports `false` when metadata cannot be obtained, which would silently
/// drop the entry; metadata is therefore obtained explicitly and its failure
/// is fatal. Symlinks under `templates/` are followed (a link to a
/// directory is traversed, a link to an `.html` file is collected); an
/// unresolvable link fails the build instead of vanishing from the set.
fn collect_templates(
    dir: &Path,
    base: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), BuildError> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(discovery_error(dir, e)),
    };
    let mut paths = Vec::new();
    for entry in read {
        paths.push(entry.map_err(|e| discovery_error(dir, e))?.path());
    }
    paths.sort();
    for path in paths {
        let file_type = std::fs::metadata(&path)
            .map(|metadata| metadata.file_type())
            .map_err(|e| discovery_error(&path, e))?;
        if file_type.is_dir() {
            collect_templates(&path, base, out)?;
        } else if path.extension().is_some_and(|ext| ext == "html") {
            let name = path
                .strip_prefix(base)
                .map(|rel| {
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .unwrap_or_else(|_| path.display().to_string());
            out.push((name, path));
        }
    }
    Ok(())
}

/// Rendering context for one entry page: explicit, no ambient state.
///
/// Pre-rendered body HTML is passed as `content`; templates must mark it
/// `| safe` (MiniJinja auto-escapes `.html` templates, so raw `{{ content }}`
/// would double-escape). Dates appear twice: raw `date`/`last_modified`
/// (`YYYY-MM-DD`, for `<time datetime>`) and `date_formatted`/
/// `last_modified_formatted` per `site.date_format`. `canonical_url`,
/// OpenGraph `og_*`, and `json_ld` keys are only present when their inputs
/// exist — never fabricated.
fn entry_context(
    config: &SignalConfig,
    root: &Path,
    entry: &signal_core::ContentEntry,
) -> Result<RenderContext, BuildError> {
    use signal_core::{absolute_url, canonical_url, format_date};
    use signal_generators::json_ld::article_json_ld;

    let site_title = &config.site.title;
    let base_url = config.site.base_url.as_deref();
    let date_format = config.date_format_str();
    let mut ctx = RenderContext::new();
    ctx.insert("site_title", site_title);
    if let Some(url) = base_url {
        ctx.insert("base_url", url);
    }
    ctx.insert("title", &entry.title);
    if let Some(description) = &entry.description {
        ctx.insert("description", description);
    }
    if let Some(date) = &entry.date {
        ctx.insert("date", date);
        if let Some(formatted) = format_date(date, date_format) {
            ctx.insert("date_formatted", formatted);
        }
    }
    if let Some(last_modified) = &entry.last_modified {
        ctx.insert("last_modified", last_modified);
        if let Some(formatted) = format_date(last_modified, date_format) {
            ctx.insert("last_modified_formatted", formatted);
        }
    }
    let author = entry
        .author
        .clone()
        .filter(|a| !a.trim().is_empty())
        .or_else(|| config.site.author.clone().filter(|a| !a.trim().is_empty()));
    if let Some(author) = &author {
        ctx.insert("author", author);
    }
    if let Some(image) = &entry.image {
        // URL-path form, matching `og:image`/JSON-LD (which flow through
        // `absolute_url` → `canonical_url`): a resolved site-root image path
        // is a path, never an authored URL, so `#`/`?` encode here exactly
        // as they do in metadata. Absolute (`http(s)://…`) and
        // protocol-relative (`//…`) values pass through untouched —
        // encoding them would destroy the scheme.
        let src = if image.starts_with('/') && !image.starts_with("//") {
            signal_core::encode_url_path(image)
        } else {
            image.clone()
        };
        ctx.insert("image", src);
        if let Some(alt) = &entry.image_alt {
            ctx.insert("image_alt", alt);
        }
    }
    ctx.insert("content", &entry.body.html);
    // URL-path form: templates render `route` directly into links, so
    // URL-significant characters arrive already encoded. The logical route
    // stays typed (`&Route`) everywhere identity matters (menus, lookups,
    // manifest keys).
    ctx.insert("route", signal_core::encode_route_path(&entry.route));
    ctx.insert("collection", &entry.collection.0);
    ctx.insert("reading_time", reading_time_minutes(entry.body.word_count));
    let tags: Vec<&String> = entry.tags.iter().collect();
    ctx.insert("tags", &tags);
    // TOC projection over the normalized headings: hierarchy with fragment
    // ids, no HTML involved. Omitted for pages without listable headings so
    // templates render no empty markup.
    let toc = signal_core::Toc::build(&entry.body.headings);
    if !toc.is_empty() {
        ctx.insert("toc", &toc);
    }
    // Diagram gate for templates: behaves like `toc` — present only when a
    // `mermaid` fenced block exists, so pages without diagrams load no
    // renderer. Derived from normalized code metadata, never from HTML.
    if entry.body.code_blocks.iter().any(|block| {
        block
            .language
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
    }) {
        ctx.insert("has_mermaid", true);
    }

    let canonical = base_url.map(|base| canonical_url(base, &entry.route));
    if let Some(canonical) = &canonical {
        ctx.insert("canonical_url", canonical);
    }
    let image_absolute = entry
        .image
        .as_deref()
        .zip(base_url)
        .map(|(image, base)| absolute_url(base, image))
        .or_else(|| {
            entry.image.clone().filter(|image| {
                image.starts_with("http://")
                    || image.starts_with("https://")
                    || image.starts_with("//")
            })
        });
    if let Some(image_absolute) = &image_absolute {
        ctx.insert("image_absolute_url", image_absolute);
    }

    ctx.insert("og_title", &entry.title);
    if let Some(description) = &entry.description {
        ctx.insert("og_description", description);
    }
    if let Some(canonical) = &canonical {
        ctx.insert("og_url", canonical);
    }
    ctx.insert("og_type", "article");
    if let Some(image_absolute) = &image_absolute {
        ctx.insert("og_image", image_absolute);
    }
    ctx.insert(
        "json_ld",
        article_json_ld(
            &entry.title,
            entry.description.as_deref(),
            entry.date.as_deref(),
            entry.last_modified.as_deref(),
            canonical.as_deref(),
            image_absolute.as_deref(),
            author.as_deref(),
        ),
    );
    insert_menus(&mut ctx, config, root, &entry.route)?;
    Ok(ctx)
}

/// Rendering context for one section page: title, optional description and
/// body (from the section-root entry when present, else collection config),
/// plus the listing summaries. Never the model itself.
fn section_context(
    site_title: &str,
    base_url: Option<&str>,
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    collection: &str,
    route: &Route,
) -> Result<RenderContext, BuildError> {
    let section_root = model.lookup_by_route(route);
    let title = section_root
        .map(|e| e.title.clone())
        .or_else(|| {
            config
                .collections
                .get(collection)
                .and_then(|c| c.title.clone())
        })
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| collection.to_string());
    let description = section_root
        .and_then(|e| e.description.clone())
        .or_else(|| {
            config
                .collections
                .get(collection)
                .and_then(|c| c.description.clone())
        })
        .filter(|d| !d.trim().is_empty());
    let content = section_root
        .map(|e| e.body.html.clone())
        .unwrap_or_default();

    let mut ctx = RenderContext::new();
    ctx.insert("site_title", site_title);
    if let Some(url) = base_url {
        ctx.insert("base_url", url);
    }
    ctx.insert("title", &title);
    if let Some(description) = &description {
        ctx.insert("description", description);
    }
    ctx.insert("content", &content);
    // URL-path form, like entry pages: safe to render into links directly.
    ctx.insert("route", signal_core::encode_route_path(route));
    ctx.insert("collection", collection);
    let entries: Vec<EntrySummary> = collection_summaries(
        model,
        &signal_core::CollectionId::new(collection.to_string()),
        config.date_format_str(),
    );
    ctx.insert("entries", &entries);
    page_metadata(&mut ctx, config, &title, description.as_deref(), route);
    insert_menus(&mut ctx, config, root, route)?;
    Ok(ctx)
}

/// Shared metadata for non-entry pages (sections, home, taxonomy):
/// canonical URL, website-type OpenGraph tags, and `WebPage` JSON-LD.
/// Keys appear only when their inputs exist.
fn page_metadata(
    ctx: &mut RenderContext,
    config: &SignalConfig,
    title: &str,
    description: Option<&str>,
    route: &Route,
) {
    use signal_core::canonical_url;
    use signal_generators::json_ld::webpage_json_ld;

    let canonical = config
        .site
        .base_url
        .as_deref()
        .map(|base| canonical_url(base, route));
    if let Some(canonical) = &canonical {
        ctx.insert("canonical_url", canonical);
    }
    ctx.insert("og_title", title);
    if let Some(description) = description {
        ctx.insert("og_description", description);
    }
    if let Some(canonical) = &canonical {
        ctx.insert("og_url", canonical);
    }
    ctx.insert("og_type", "website");
    ctx.insert(
        "json_ld",
        webpage_json_ld(title, canonical.as_deref(), description),
    );
}

/// Build a `Config` diagnostic for a menu validation failure.
///
/// Reads `signal.toml` best-effort for snippet display; an unreadable file
/// still yields a clear message since the menu error itself is owned.
fn menu_config_error(root: &Path, err: signal_core::MenuError) -> BuildError {
    use miette::NamedSource;
    let path = root.join("signal.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    BuildError::Config {
        path: path.display().to_string(),
        src: NamedSource::new(path.display().to_string(), text),
        message: err.to_string(),
    }
}

/// Insert the resolved `menus.main` navigation when configured.
///
/// Pure projection of validated configuration plus the current route: no
/// filesystem, no generated HTML, no `SiteModel`. Absent entirely when no
/// main menu is configured, so templates omit navigation with `{% if menus %}`.
fn insert_menus(
    ctx: &mut RenderContext,
    config: &SignalConfig,
    root: &Path,
    route: &Route,
) -> Result<(), BuildError> {
    use std::collections::BTreeMap;
    match signal_core::resolve_main_menu(&config.menus, route) {
        Ok(None) => Ok(()),
        Ok(Some(items)) => {
            ctx.insert("menus", BTreeMap::from([("main".to_string(), items)]));
            Ok(())
        }
        Err(err) => Err(menu_config_error(root, err)),
    }
}

/// Rendering context for the home page: hero plus recent summaries from the
/// configured home collection. Never the model itself.
///
/// No `title` key is set: the base template falls back to the bare site
/// title, mirroring Hugo's `IsHome` title branch.
fn home_context(
    site_title: &str,
    base_url: Option<&str>,
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    home: &Home,
) -> Result<RenderContext, BuildError> {
    let date_format = config.date_format_str();
    let mut ctx = RenderContext::new();
    ctx.insert("site_title", site_title);
    if let Some(url) = base_url {
        ctx.insert("base_url", url);
    }
    if let Some(featured) = home.featured(model, date_format) {
        ctx.insert("featured", &featured);
    }
    ctx.insert("recent", home.recent(model, date_format));
    page_metadata(
        &mut ctx,
        config,
        site_title,
        None,
        &Route::new("/".to_string()),
    );
    insert_menus(&mut ctx, config, root, &Route::new("/".to_string()))?;
    Ok(ctx)
}

/// Rendering context for the taxonomy index: title plus one row per term
/// (label, route, member count). Member entries are one click away; the
/// index itself carries no bodies. Never the model itself.
fn topics_index_context(
    site_title: &str,
    base_url: Option<&str>,
    config: &SignalConfig,
    root: &Path,
    title: &str,
    route: &Route,
    terms: &[TopicSummary],
) -> Result<RenderContext, BuildError> {
    #[derive(serde::Serialize)]
    struct TopicRow<'a> {
        label: &'a str,
        slug: &'a str,
        // URL-path form: taxonomy roots come from configuration, so the
        // encoding boundary applies here exactly as for entry routes.
        route: String,
        count: usize,
    }
    let mut ctx = RenderContext::new();
    ctx.insert("site_title", site_title);
    if let Some(url) = base_url {
        ctx.insert("base_url", url);
    }
    ctx.insert("title", title);
    ctx.insert("route", signal_core::encode_route_path(route));
    let rows: Vec<TopicRow<'_>> = terms
        .iter()
        .map(|t| TopicRow {
            label: &t.label,
            slug: &t.slug,
            route: signal_core::encode_route_path(&signal_core::Route::new(t.route.clone())),
            count: t.count,
        })
        .collect();
    ctx.insert("topics", &rows);
    page_metadata(&mut ctx, config, title, None, route);
    insert_menus(&mut ctx, config, root, route)?;
    Ok(ctx)
}

/// Rendering context for one term page: label, route, and member summaries
/// newest-first (shared [`EntrySummary`] representation). Never the model.
fn topic_context(
    site_title: &str,
    base_url: Option<&str>,
    config: &SignalConfig,
    root: &Path,
    term: &TopicSummary,
) -> Result<RenderContext, BuildError> {
    let mut ctx = RenderContext::new();
    ctx.insert("site_title", site_title);
    if let Some(url) = base_url {
        ctx.insert("base_url", url);
    }
    ctx.insert("title", &term.label);
    ctx.insert("topic", &term.label);
    // URL-path form (term routes are slug-safe today; encoded uniformly so
    // configuration-provided roots encode exactly like entry routes).
    ctx.insert(
        "route",
        signal_core::encode_route_path(&Route::new(term.route.clone())),
    );
    ctx.insert("entries", &term.entries);
    page_metadata(
        &mut ctx,
        config,
        &term.label,
        None,
        &Route::new(term.route.clone()),
    );
    insert_menus(&mut ctx, config, root, &Route::new(term.route.clone()))?;
    Ok(ctx)
}

/// Plan static assets as first-class [`ArtifactSpec`]s.
///
/// Recursively enumerates `static/` (sorted, deterministic), skipping
/// symlinks so discovery cannot escape the static root. Specs are
/// path-oriented: no bytes are read during planning.
fn static_artifacts(root: &Path) -> Result<Vec<ArtifactSpec>, BuildError> {
    let static_dir = root.join("static");
    if !static_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut specs = Vec::new();
    collect_static_files(&static_dir, &static_dir, &mut specs)?;
    Ok(specs)
}

fn collect_static_files(
    dir: &Path,
    base: &Path,
    specs: &mut Vec<ArtifactSpec>,
) -> Result<(), BuildError> {
    let read_dir = std::fs::read_dir(dir).map_err(|e| BuildError::Read {
        path: dir.display().to_string(),
        message: e.to_string(),
    })?;
    // Every directory-entry failure propagates: a dropped entry would read
    // as a deleted asset and prune published output (same invariant as
    // Markdown/template discovery). Only successfully inspected symlinks
    // are intentionally skipped.
    let mut entries: Vec<std::fs::DirEntry> = Vec::new();
    for entry in read_dir {
        entries.push(entry.map_err(|e| discovery_error(dir, e))?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        // Symlinks are skipped: `file_type` does not follow them, so a
        // link pointing outside the static root is never traversed.
        let file_type = entry.file_type().map_err(|e| BuildError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_static_files(&path, base, specs)?;
        } else {
            let relative = path_relative_unix(base, &path);
            specs.push(ArtifactSpec::new(relative, ArtifactKind::Static));
        }
    }
    Ok(())
}

/// Resolve one static artifact: read the corresponding source file from
/// `static/` so it flows through the same resolve/write pipeline as
/// generated outputs. Bytes stay bytes — the spec never carried them.
fn resolve_static_artifact(root: &Path, spec: &ArtifactSpec) -> Result<Vec<u8>, BuildError> {
    let source = root.join("static").join(&spec.path);
    std::fs::read(&source).map_err(|e| BuildError::Read {
        path: source.display().to_string(),
        message: e.to_string(),
    })
}

/// Run the full vertical slice: ingest → freeze → generate → render → write.
///
/// Entry pages, one section page per configured collection, and (when
/// `site.home_collection` is set) the home page are planned as
/// [`ArtifactSpec`] values, then each artifact is rendered and written
/// individually; rendered pages never accumulate in memory. Output is
/// deterministic for identical inputs.
pub fn build_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<BuildSummary, BuildError> {
    // Fail fast on invalid configuration before anything is planned:
    // route prefixes become output path components, so they must be
    // validated segments; menus are validated through the same pure
    // resolution every page will use.
    validate_config_routes(config)?;
    if let Err(err) = signal_core::resolve_main_menu(&config.menus, &Route::new("/")) {
        return Err(menu_config_error(root, err));
    }

    let mut specs = EntryPages::new()
        .generate(model)
        .map_err(|e| BuildError::Model {
            message: e.to_string(),
        })?;

    let mut names: Vec<&String> = config.collections.keys().collect();
    names.sort();
    let mut homes: Vec<Home> = Vec::new();
    for name in &names {
        let section = SectionIndex::new(
            signal_core::CollectionId::new((*name).clone()),
            config.route_prefix_for(name),
        );
        specs.extend(section.generate(model).map_err(|e| BuildError::Model {
            message: e.to_string(),
        })?);
    }
    if let Some(home_collection) = config.site.home_collection.clone() {
        let home = Home::new(
            signal_core::CollectionId::new(home_collection),
            HOME_RECENT_LIMIT,
        );
        specs.extend(home.generate(model).map_err(|e| BuildError::Model {
            message: e.to_string(),
        })?);
        homes.push(home);
    }
    if let Some(root) = taxonomy_root(config) {
        specs.extend(
            TopicsIndex::new(root.clone())
                .generate(model)
                .map_err(|e| BuildError::Model {
                    message: e.to_string(),
                })?,
        );
        specs.extend(TopicTerms::new(root.clone()).generate(model).map_err(|e| {
            BuildError::Model {
                message: e.to_string(),
            }
        })?);
    }

    // Feeds are explicit opt-in (`[feed]`) because item selection and caps
    // are editorial choices. They additionally require `site.base_url` for
    // absolute URLs; an explicit-but-unsatisfiable configuration fails the
    // build rather than silently dropping promised feeds. The sitemap needs
    // only `base_url` and follows it quietly, like `static/`.
    let limit = feed_limit(config);
    if config.feed.is_some() && config.site.base_url.is_none() {
        return Err(BuildError::Model {
            message: "feeds require site.base_url for absolute item URLs".to_string(),
        });
    }
    if config.feed.is_some() {
        specs.extend(
            MainFeed::new(limit)
                .generate(model)
                .map_err(|e| BuildError::Model {
                    message: e.to_string(),
                })?,
        );
        if let Some(root) = taxonomy_root(config) {
            specs.extend(
                TaxonomyFeeds::new(root, limit)
                    .generate(model)
                    .map_err(|e| BuildError::Model {
                        message: e.to_string(),
                    })?,
            );
        }
    }
    if config.site.base_url.is_some() {
        specs.extend(
            Sitemap::new()
                .generate(model)
                .map_err(|e| BuildError::Model {
                    message: e.to_string(),
                })?,
        );
    }
    // The search index needs no configuration: routes are relative, every
    // site gets the same schema, and there is nothing editorial to tune.
    specs.extend(
        Search::new()
            .generate(model)
            .map_err(|e| BuildError::Model {
                message: e.to_string(),
            })?,
    );
    // Static assets are part of the plan: every output file Signal produces
    // corresponds to an ArtifactSpec, so collision validation covers them
    // and the manifest can enumerate the complete output.
    specs.extend(static_artifacts(root)?);

    let plan = BuildPlan::new(root.to_path_buf(), out_dir.to_path_buf(), specs);
    plan.validate_output_paths()?;
    plan.validate_routes().map_err(|e| BuildError::Model {
        message: e.to_string(),
    })?;

    let renderer = load_templates(root)?;
    // Reuse inputs, computed once: the previous manifest (untrusted until
    // every check passes), plus current template and config digests shared
    // across all per-artifact decisions.
    let previous = crate::manifest::load_previous(out_dir);
    let usable_prev = match &previous {
        crate::manifest::PreviousManifest::Usable(manifest) => Some(manifest),
        _ => None,
    };
    let current_templates = crate::manifest::template_digests(&renderer)?;
    let current_config_digest = crate::manifest::config_digest(config);
    let mut pages_written = 0;
    let mut reused = 0;
    let mut rebuilt = 0;
    let mut rebuilt_paths = Vec::new();
    let mut outputs = std::collections::BTreeMap::new();
    for spec in &plan.specs {
        // Every artifact resolves independently from (spec, config, model):
        // structured serializers for data artifacts, the renderer for HTML,
        // and a source read for statics. No generated HTML is ever read
        // back, and no loop-local state is consulted. Reusable artifacts
        // skip resolve and write entirely; their recorded output digest
        // carries forward into the new manifest.
        let reusable = match usable_prev {
            Some(prev) => crate::manifest::artifact_reusable(
                spec,
                config,
                root,
                model,
                &current_templates,
                &current_config_digest,
                prev,
                out_dir,
            ),
            None => false,
        };
        if reusable {
            let record = usable_prev
                .and_then(|prev| prev.artifacts.get(&spec.path))
                .expect("reusable implies a previous record");
            outputs.insert(spec.path.clone(), record.output_digest.clone());
            reused += 1;
            continue;
        }
        let bytes = resolve_artifact(spec, config, root, model, &renderer)?;
        outputs.insert(spec.path.clone(), crate::manifest::digest_bytes(&bytes));
        write_artifact(out_dir, &spec.path, &bytes).map_err(|e| BuildError::Write {
            path: out_dir.join(&spec.path).display().to_string(),
            message: e.to_string(),
        })?;
        pages_written += 1;
        rebuilt += 1;
        rebuilt_paths.push(spec.path.clone());
    }

    // The manifest records the completed build only: every artifact above
    // resolved and wrote successfully, so this is a truthful record, never
    // a partial one. It does not influence this build in any way.
    //
    // Stale-output reconciliation happens first: artifacts in the previous
    // manifest but absent from the current plan are deleted, so the new
    // manifest describes exactly the output tree it accompanies. Pruning
    // runs only against a usable previous manifest — never from corrupt
    // metadata — and only after all current artifacts wrote successfully.
    // Invalid stale paths are skipped, never followed; anything else that
    // fails safe-deletion fails the build, leaving the old manifest intact.
    let mut pruned = 0;
    let mut pruned_paths = Vec::new();
    if let Some(prev) = usable_prev {
        // Filesystem identities of every artifact the current plan claims.
        // Current artifacts were just written (or validated for reuse), so
        // they exist. A stale path that resolves to the same filesystem
        // object is an alias of a current artifact — case-insensitive or
        // Unicode-normalizing volumes make distinct logical paths name one
        // file — and must never be deleted.
        let current_identities: BTreeSet<String> = plan
            .specs
            .iter()
            .filter_map(|spec| crate::discover::file_identity(&out_dir.join(&spec.path)))
            .collect();
        for stale in crate::manifest::stale_artifact_paths(prev, &plan.specs) {
            // Untrusted manifest metadata must never fail — or redirect — a
            // valid build: invalid stale paths are skipped, and the record
            // drops out of the new manifest below.
            if !crate::discover::is_safe_artifact_path(&stale) {
                continue;
            }
            // Refuse a stale path that is the same filesystem object as any
            // current artifact. On an ambiguous identity we skip rather than
            // risk deleting a current artifact.
            if let Some(identity) = crate::discover::file_identity(&out_dir.join(&stale)) {
                if current_identities.contains(&identity) {
                    continue;
                }
            }
            match crate::discover::remove_artifact(out_dir, &stale) {
                Ok(true) => {
                    pruned += 1;
                    pruned_paths.push(stale);
                }
                Ok(false) => {
                    // Already absent: filesystem already reconciled, and the
                    // stale record simply drops out of the new manifest.
                }
                Err(e) => {
                    return Err(BuildError::Write {
                        path: out_dir.join(&stale).display().to_string(),
                        message: format!("could not remove stale artifact: {e}"),
                    });
                }
            }
        }
    }
    let manifest =
        crate::manifest::build_manifest(config, model, &renderer, &plan.specs, &outputs)?;
    crate::manifest::write_manifest(out_dir, &manifest)?;

    let static_files = plan
        .specs
        .iter()
        .filter(|spec| spec.kind == ArtifactKind::Static)
        .count();
    Ok(BuildSummary {
        pages_written,
        drafts_skipped: 0,
        static_files,
        specs: plan.specs,
        reused,
        rebuilt,
        rebuilt_paths,
        pruned,
        pruned_paths,
    })
}

/// Validate configured route prefixes (collections + taxonomy).
///
/// These become route and output path components, so every segment must be
/// a safe, normalized route segment. Called before ingestion and again
/// before planning so both entry points fail fast.
pub(crate) fn validate_config_routes(config: &SignalConfig) -> Result<(), BuildError> {
    let mut names: Vec<&String> = config.collections.keys().collect();
    names.sort();
    for name in names {
        let prefix = config.route_prefix_for(name);
        if let Err(reason) = signal_core::validate_route(&prefix) {
            return Err(BuildError::Model {
                message: format!(
                    "collection {name:?} has an invalid route_prefix {prefix:?}: {reason}"
                ),
            });
        }
    }
    if let Some(root) = taxonomy_root(config) {
        if let Err(reason) = signal_core::validate_route(&root) {
            return Err(BuildError::Model {
                message: format!("taxonomy route_prefix is invalid: {reason}"),
            });
        }
    }
    Ok(())
}

/// Resolve a single HTML artifact (entry, section, home, taxonomy) from
/// the model, config, and renderer alone. Cheap projections (,
/// home picks) are recomputed inside so this function is self-contained:
/// one artifact can be regenerated without the full build loop.
fn resolve_html_artifact(
    spec: &ArtifactSpec,
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    renderer: &MiniJinjaRenderer,
) -> Result<String, BuildError> {
    let route = spec.route.as_ref().ok_or_else(|| BuildError::Model {
        message: format!("artifact {:?} has no route", spec.path),
    })?;
    let site_title = config.site.title.clone();
    let base_url = config.site.base_url.clone();
    let (template, ctx) = match spec.kind {
        ArtifactKind::Page => {
            let entry = model
                .lookup_by_route(route)
                .ok_or_else(|| BuildError::Model {
                    message: format!("no entry for route {route}"),
                })?;
            (
                template_for_collection(config, &entry.collection.0),
                entry_context(config, root, entry)?,
            )
        }
        ArtifactKind::CollectionIndex => {
            let collection =
                collection_for_section_route(config, model, route).ok_or_else(|| {
                    BuildError::Model {
                        message: format!("no collection for section route {route}"),
                    }
                })?;
            (
                section_template_for_collection(config, &collection),
                section_context(
                    &site_title,
                    base_url.as_deref(),
                    config,
                    root,
                    model,
                    &collection,
                    route,
                )?,
            )
        }
        ArtifactKind::Home => {
            let home_collection =
                config
                    .site
                    .home_collection
                    .clone()
                    .ok_or_else(|| BuildError::Model {
                        message: "home artifact without home configuration".to_string(),
                    })?;
            let home = Home::new(
                signal_core::CollectionId::new(home_collection),
                HOME_RECENT_LIMIT,
            );
            (
                home_template(config),
                home_context(&site_title, base_url.as_deref(), config, root, model, &home)?,
            )
        }
        ArtifactKind::Taxonomy => {
            let tax_root = taxonomy_root(config).ok_or_else(|| BuildError::Model {
                message: "taxonomy artifact without taxonomy configuration".to_string(),
            })?;
            let terms = topic_terms(model, &tax_root, config.date_format_str()).map_err(|e| {
                BuildError::Model {
                    message: e.to_string(),
                }
            })?;
            if route.0 == tax_root {
                (
                    topics_index_template(config),
                    topics_index_context(
                        &site_title,
                        base_url.as_deref(),
                        config,
                        root,
                        &taxonomy_title(config),
                        route,
                        &terms,
                    )?,
                )
            } else {
                let term =
                    terms
                        .iter()
                        .find(|t| t.route == route.0)
                        .ok_or_else(|| BuildError::Model {
                            message: format!("no topic for route {route}"),
                        })?;
                (
                    topic_term_template(config),
                    topic_context(&site_title, base_url.as_deref(), config, root, term)?,
                )
            }
        }
        _ => {
            return Err(BuildError::Model {
                message: format!("unsupported artifact kind for {:?}", spec.path),
            });
        }
    };
    renderer
        .render(&template, &ctx)
        .map_err(|e| BuildError::Render {
            message: format!("template {template:?} for route {route}: {e}"),
        })
}

/// Resolve one artifact independently: the single dispatch point from an
/// [`ArtifactSpec`] to its bytes.
///
/// HTML kinds render through the renderer; RSS, sitemap, and the search
/// index serialize from the model through structured serializers; static
/// artifacts read their source. No generated HTML is ever read back, and
/// no loop-local state is consulted.
pub fn resolve_artifact(
    spec: &ArtifactSpec,
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    renderer: &MiniJinjaRenderer,
) -> Result<Vec<u8>, BuildError> {
    match spec.kind {
        ArtifactKind::Rss => resolve_feed(config, model, spec).map(String::into_bytes),
        ArtifactKind::Sitemap => {
            let home = config.site.home_collection.is_some();
            resolve_sitemap(config, model, home).map(String::into_bytes)
        }
        ArtifactKind::SearchIndex => {
            Ok(signal_generators::search::search_index_json(model).into_bytes())
        }
        ArtifactKind::Static => resolve_static_artifact(root, spec),
        _ => resolve_html_artifact(spec, config, root, model, renderer).map(String::into_bytes),
    }
}

/// Resolve one RSS artifact from the model (never from generated HTML).
///
/// `index.xml` is the main feed; `<root>/<slug>/index.xml` is the matching
/// term feed, located through the same [`TopicSummary`] routes the term
/// pages use. Feed projections are reconstructed from config, so this
/// function is self-contained.
fn resolve_feed(
    config: &SignalConfig,
    model: &SiteModel,
    spec: &ArtifactSpec,
) -> Result<String, BuildError> {
    use signal_core::canonical_url;
    use signal_generators::rss::{
        channel_xml, main_channel_meta, newest_pub_date, term_channel_meta,
    };

    let base = config
        .site
        .base_url
        .as_deref()
        .ok_or_else(|| BuildError::Model {
            message: "feeds require site.base_url for absolute item URLs".to_string(),
        })?;
    let limit = feed_limit(config);
    // The `self` link is a URL, not a filesystem path: `spec.path` segments
    // encode exactly like route segments (identity for ordinary ASCII
    // paths; correct for configuration-provided roots with URL-significant
    // characters). The base URL itself is concatenated, never encoded.
    let self_url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        signal_core::encode_url_path(&spec.path)
    );
    if spec.path == "index.xml" {
        let items = MainFeed::new(limit).items(model, base);
        let (title, description) = main_channel_meta(&config.site.title);
        let link = canonical_url(base, &Route::new("/".to_string()));
        return Ok(channel_xml(
            &title,
            &link,
            &description,
            &self_url,
            newest_pub_date(&items),
            &items,
        ));
    }
    let tax_root = taxonomy_root(config).ok_or_else(|| BuildError::Model {
        message: format!(
            "feed artifact {:?} without taxonomy feed configuration",
            spec.path
        ),
    })?;
    let terms = topic_terms(model, &tax_root, signal_core::DEFAULT_DATE_FORMAT).map_err(|e| {
        BuildError::Model {
            message: e.to_string(),
        }
    })?;
    let term = terms
        .iter()
        .find(|t| format!("{}/index.xml", t.route.trim_matches('/')) == spec.path)
        .ok_or_else(|| BuildError::Model {
            message: format!("no topic for feed {:?}", spec.path),
        })?;
    let items = TaxonomyFeeds::new(tax_root, limit).term_items(model, &term.label, base);
    let (title, description) = term_channel_meta(&term.label, &config.site.title);
    let link = canonical_url(base, &Route::new(term.route.clone()));
    Ok(channel_xml(
        &title,
        &link,
        &description,
        &self_url,
        newest_pub_date(&items),
        &items,
    ))
}

/// Resolve the sitemap from the explicit public route inventory: entry
/// routes, collection prefixes, the home route when generated, and taxonomy
/// routes. Drafts, feeds, and static assets never enter the inventory, so
/// they cannot leak in.
fn resolve_sitemap(
    config: &SignalConfig,
    model: &SiteModel,
    home: bool,
) -> Result<String, BuildError> {
    use signal_generators::sitemap::{sitemap_urls, sitemap_xml};

    let base = config
        .site
        .base_url
        .as_deref()
        .ok_or_else(|| BuildError::Model {
            message: "sitemap requires site.base_url for absolute URLs".to_string(),
        })?;
    let mut sections: Vec<String> = config
        .collections
        .keys()
        .map(|name| config.route_prefix_for(name))
        .collect();
    sections.sort();
    Ok(sitemap_xml(&sitemap_urls(
        model,
        base,
        home,
        &sections,
        taxonomy_root(config).as_deref(),
    )))
}

/// Convenience: ingest from disk and build in one call (used by the CLI and
/// the golden test).
pub fn build_site_from_disk(root: &Path, out_dir: &Path) -> Result<BuildSummary, BuildError> {
    let config_path = root.join("signal.toml");
    let text = read_file(&config_path)?;
    let config: SignalConfig = toml::from_str(&text)
        .map_err(|e| crate::config::config_parse_error(&config_path, &text, e))?;
    let (model, drafts_skipped) = ingest_site(root, &config)?;
    let mut summary = build_site(root, out_dir, &config, &model)?;
    summary.drafts_skipped = drafts_skipped;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{ArtifactKind, Route};
    use std::fs;

    #[test]
    fn pipeline_order_is_fixed() {
        let stages = PipelineStage::ordered();
        assert_eq!(stages.len(), 7);
        assert_eq!(stages[0], PipelineStage::Ingest);
        assert_eq!(stages[stages.len() - 1], PipelineStage::WriteOutput);
        let mut sorted = stages.clone();
        sorted.sort();
        // Ordered() must already be in enum order; guards accidental reorder.
        assert_eq!(stages, sorted);
    }

    #[test]
    fn output_collisions_are_detected() {
        let plan = BuildPlan::new(
            PathBuf::from("/root"),
            PathBuf::from("/out"),
            vec![
                ArtifactSpec::new("a/index.html", ArtifactKind::Page),
                ArtifactSpec::new("a/index.html", ArtifactKind::Page),
            ],
        );
        assert!(matches!(
            plan.validate_output_paths(),
            Err(BuildError::OutputCollision { .. })
        ));
    }

    #[test]
    fn route_list_validation() {
        let ok = vec![Route::new("/a/"), Route::new("/b/")];
        assert!(validate_route_list(&ok).is_ok());
        let dup = vec![Route::new("/a/"), Route::new("/a/")];
        assert!(validate_route_list(&dup).is_err());
    }

    #[test]
    fn entry_pages_render_heading_ids_and_toc() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: A\n---\n\n## Install\n\nText.\n\n## Install\n",
                ),
                ("content/posts/b.md", "---\ntitle: B\n---\n\nNo headings here.\n"),
                (
                    "templates/post.html",
                    "<html><body>{% if toc %}<nav>{% for item in toc.items recursive %}<a href=\"#{{ item.id }}\">{{ item.text }}</a>{{ loop(item.children) }}{% endfor %}</nav>{% endif %}{{ content | safe }}</body></html>",
                ),
                ("templates/section.html", "<html><body>section</body></html>"),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/a/index.html")).expect("output");
        // Duplicate headings get deterministic unique ids in both the
        // rendered HTML and the TOC, from the single assignment.
        assert!(html.contains("<h2 id=\"install\">"), "got: {html}");
        assert!(html.contains("<h2 id=\"install-1\">"), "got: {html}");
        assert!(html.contains("href=\"#install\""), "got: {html}");
        assert!(html.contains("href=\"#install-1\""), "got: {html}");
        // Pages without listable headings omit the TOC entirely.
        let plain = fs::read_to_string(out.join("posts/b/index.html")).expect("output");
        assert!(!plain.contains("<nav>"), "got: {plain}");
    }

    fn write_site(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, content).expect("write");
        }
    }

    fn minimal_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/hello.md",
                    "---\ntitle: Hello\ndescription: Greeting.\n---\n\nHello body.\n",
                ),
                (
                    "templates/post.html",
                    "<html><head><title>{{ title }}</title></head><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><head><title>{{ title }}</title></head><body>{% for e in entries %}<a href=\"{{ e.route }}\">{{ e.title }}</a>{% endfor %}</body></html>",
                ),
            ],
        );
    }

    #[test]
    fn builds_one_page_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        // One entry page plus one section page for the posts collection,
        // plus the always-on search index.
        assert_eq!(summary.pages_written, 3);
        let html = fs::read_to_string(out.join("posts/hello/index.html")).expect("output");
        assert!(html.contains("<title>Hello</title>"), "got: {html}");
        assert!(html.contains("Hello body."), "got: {html}");
        // Pre-rendered HTML must not be double-escaped.
        assert!(!html.contains("&lt;p&gt;"), "got: {html}");
        let section = fs::read_to_string(out.join("posts/index.html")).expect("section");
        // No _index.md and no configured title: the collection name is used.
        assert!(section.contains("<title>posts</title>"), "got: {section}");
        assert!(section.contains(">Hello</a>"), "got: {section}");
    }

    #[test]
    fn section_lists_entries_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\ntitle = \"Posts\"\n",
                ),
                ("content/posts/b-old.md", "---\ntitle: Old\ndate: 2026-01-01\n---\n\nOld.\n"),
                ("content/posts/a-new.md", "---\ntitle: New\ndate: 2026-09-01\n---\n\nNew.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<h1>{{ title }}</h1>{% for e in entries %}[{{ e.title }}]{% endfor %}",
                ),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let section = fs::read_to_string(out.join("posts/index.html")).expect("section");
        assert!(section.contains("<h1>Posts</h1>"), "got: {section}");
        let new_pos = section.find("[New]").expect("new listed");
        let old_pos = section.find("[Old]").expect("old listed");
        assert!(new_pos < old_pos, "newest first, got: {section}");
    }

    #[test]
    fn home_renders_featured_hero_and_recent_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\nhome_collection = \"posts\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: Picked\nfeatured: true\ndate: 2026-09-01\n---\n\nA.\n",
                ),
                ("content/posts/b.md", "---\ntitle: Other\ndate: 2026-01-01\n---\n\nB.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                (
                    "templates/home.html",
                    "<html><body>{% if featured %}HERO {{ featured.title }}{% endif %}{% for e in recent %}R{{ e.title }}{% endfor %}</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        assert_eq!(summary.pages_written, 5);
        let home = fs::read_to_string(out.join("index.html")).expect("home");
        assert!(home.contains("HERO Picked"), "got: {home}");
        assert!(home.contains("RPicked"), "got: {home}");
        assert!(home.contains("ROther"), "got: {home}");
    }

    #[test]
    fn missing_template_is_a_render_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        fs::remove_file(dir.path().join("templates/post.html")).expect("remove");
        let out = dir.path().join("out");
        let err = build_site_from_disk(dir.path(), &out).expect_err("fails");
        assert!(matches!(err, BuildError::Render { .. }), "got: {err:?}");
    }

    #[test]
    fn static_dir_is_copied_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(dir.path(), &[("static/css/main.css", "body{}\n")]);
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        assert_eq!(summary.static_files, 1);
        assert_eq!(
            fs::read_to_string(out.join("css/main.css")).expect("css"),
            "body{}\n"
        );
    }

    #[test]
    fn per_collection_template_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\ntemplate = \"custom.html\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n"),
                ("templates/custom.html", "CUSTOM {{ title }}"),
                ("templates/post.html", "DEFAULT"),
                ("templates/section.html", "SECTION"),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/a/index.html")).expect("output");
        assert!(html.contains("CUSTOM A"), "got: {html}");
    }

    fn metadata_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\nbase_url = \"https://example.com/\"\nauthor = \"Site Author\"\ndate_format = \"%-d %B %Y\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: A Post\ndescription: About A.\ndate: 2026-09-02\nlastmod: 2026-09-10\nauthor: Guest Writer\nimage: images/a.svg\nimage_alt: Alt A\n---\n\nA body.\n",
                ),
                (
                    "content/posts/b.md",
                    "---\ntitle: B Post\ndate: 2026-01-01\n---\n\nB body.\n",
                ),
                (
                    "templates/post.html",
                    "<html><head><title>{{ title }}</title>{% if canonical_url %}<link rel=\"canonical\" href=\"{{ canonical_url }}\" />{% endif %}{% if og_title %}<meta property=\"og:title\" content=\"{{ og_title }}\" />{% endif %}{% if og_image %}<meta property=\"og:image\" content=\"{{ og_image }}\" />{% endif %}{% if json_ld %}<script type=\"application/ld+json\">{{ json_ld | safe }}</script>{% endif %}</head><body>{% if date_formatted %}<time datetime=\"{{ date }}\">{{ date_formatted }}</time>{% endif %}{% if last_modified_formatted %}<span>Updated {{ last_modified_formatted }}</span>{% endif %}{% if author %}<span>{{ author }}</span>{% endif %}{% if image %}<img src=\"{{ image }}\" alt=\"{{ image_alt }}\" />{% endif %}{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    #[test]
    fn entry_metadata_is_explicit_and_formatted() {
        let dir = tempfile::tempdir().expect("tempdir");
        metadata_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/a/index.html")).expect("output");
        // MiniJinja escapes `/` as `&#x2f;` in markup; browsers decode it
        // identically. JSON-LD below carries the raw URLs.
        assert!(
            html.contains(r#"<link rel="canonical" href="https:&#x2f;&#x2f;example.com&#x2f;posts&#x2f;a&#x2f;" />"#),
            "got: {html}"
        );
        assert!(
            html.contains(r#"<meta property="og:title" content="A Post" />"#),
            "got: {html}"
        );
        assert!(
            html.contains(r#"<meta property="og:image" content="https:&#x2f;&#x2f;example.com&#x2f;images&#x2f;a.svg" />"#),
            "got: {html}"
        );
        assert!(
            html.contains(r#"<time datetime="2026-09-02">2 September 2026</time>"#),
            "got: {html}"
        );
        assert!(html.contains("Updated 10 September 2026"), "got: {html}");
        assert!(html.contains("<span>Guest Writer</span>"), "got: {html}");
        assert!(
            html.contains(r#"<img src="&#x2f;images&#x2f;a.svg" alt="Alt A" />"#),
            "got: {html}"
        );
        assert!(html.contains(r#""@type":"Article""#), "got: {html}");
        assert!(
            html.contains(r#""dateModified":"2026-09-10""#),
            "got: {html}"
        );
        // Entry author overrides the site author, including in JSON-LD.
        assert!(html.contains(r#""name":"Guest Writer""#), "got: {html}");
    }

    #[test]
    fn missing_optional_metadata_is_omitted_not_fabricated() {
        let dir = tempfile::tempdir().expect("tempdir");
        metadata_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/b/index.html")).expect("output");
        // No description, image, lastmod, or author override on B.
        assert!(!html.contains("og:image"), "got: {html}");
        assert!(!html.contains("Updated"), "got: {html}");
        assert!(!html.contains("<img"), "got: {html}");
        // Site author fills the byline but the entry stays dateless-imageless.
        assert!(html.contains("<span>Site Author</span>"), "got: {html}");
        assert!(html.contains(r#""name":"Site Author""#), "got: {html}");
        assert!(!html.contains("dateModified"), "got: {html}");
        // Raw date is still present for machines; formatted for humans.
        assert!(
            html.contains(r#"<time datetime="2026-01-01">1 January 2026</time>"#),
            "got: {html}"
        );
    }

    #[test]
    fn metadata_values_are_escaped_and_json_is_safe() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\nbase_url = \"https://example.com\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/x.md",
                    "---\ntitle: 'A </script><b>bold</b>'\ndescription: 'Q \"quoted\"'\n---\n\nX.\n",
                ),
                (
                    "templates/post.html",
                    "<html><head><title>{{ title }}</title><meta property=\"og:title\" content=\"{{ og_title }}\" /><script type=\"application/ld+json\">{{ json_ld | safe }}</script></head><body></body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/x/index.html")).expect("output");
        // Template auto-escaping applies to metadata in markup (slashes
        // surface as `&#x2f;`) …
        assert!(!html.contains("<b>bold</b>"), "got: {html}");
        assert!(html.contains("&lt;b&gt;bold"), "got: {html}");
        // … while the JSON-LD payload stays parseable and script-safe.
        assert!(!html.contains("</script><b>"), "got: {html}");
        let start = html.find(r#"{"@context""#).expect("json-ld present");
        let end = html[start..].find("</script>").expect("script close");
        let payload: serde_json::Value =
            serde_json::from_str(&html[start..start + end]).expect("valid JSON-LD");
        assert_eq!(
            payload["headline"],
            serde_json::Value::from("A </script><b>bold</b>")
        );
        // No-trailing-slash base URLs still join cleanly.
        assert!(
            html.contains(r#""url":"https://example.com/posts/x/""#),
            "got: {html}"
        );
    }

    #[test]
    fn taxonomy_builds_index_and_terms_with_drafts_excluded() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[taxonomy]\nroute_prefix = \"/topics/\"\ntitle = \"Topics\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: A\ndate: 2026-09-02\ntopics: [\"Beta Tools\"]\n---\n\nA.\n",
                ),
                (
                    "content/posts/b.md",
                    "---\ntitle: B\ndate: 2026-09-03\ntopics: [\"Beta Tools\", \"alpha Guides\"]\n---\n\nB.\n",
                ),
                (
                    "content/posts/draft.md",
                    "---\ntitle: D\ndraft: true\ntopics: [\"Beta Tools\", \"Unseen\"]\n---\n\nD.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                (
                    "templates/topics.html",
                    "<h1>{{ title }}</h1>{% for t in topics %}[{{ t.label }}:{{ t.count }}]{% endfor %}",
                ),
                (
                    "templates/topic.html",
                    "<h1>{{ topic }}</h1>{% for e in entries %}[{{ e.title }}]{% endfor %}",
                ),
            ],
        );
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        // 2 entry pages + 1 section + 1 topics index + 2 term pages + search.
        // The draft (and its unique "Unseen" topic) never enters the model.
        assert_eq!(summary.pages_written, 7);
        assert_eq!(summary.drafts_skipped, 1);

        let index = fs::read_to_string(out.join("topics/index.html")).expect("index");
        assert!(index.contains("<h1>Topics</h1>"), "got: {index}");
        assert!(index.contains("[Beta Tools:2]"), "got: {index}");
        assert!(index.contains("[alpha Guides:1]"), "got: {index}");
        assert!(
            !index.contains("Unseen"),
            "draft topics must not appear: {index}"
        );

        let term = fs::read_to_string(out.join("topics/beta-tools/index.html")).expect("term");
        let new_pos = term.find("[B]").expect("newer listed");
        let old_pos = term.find("[A]").expect("older listed");
        assert!(new_pos < old_pos, "newest first, got: {term}");
        assert!(!term.contains("[D]"), "drafts must not appear: {term}");
    }

    fn feed_site(dir: &Path, extra_config: &str) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    &format!(
                        "[site]\ntitle = \"T\"\n{extra_config}\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n"
                    ),
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: A\ndate: 2026-09-02\n---\n\nA body words here.\n",
                ),
                (
                    "content/posts/b.md",
                    "---\ntitle: B\ndate: 2026-09-03\ndraft: true\n---\n\nB draft.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    #[test]
    fn no_feed_table_means_no_feeds_but_sitemap_follows_base_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        feed_site(dir.path(), "base_url = \"https://example.com/\"");
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        // 1 entry + 1 section + sitemap + search; drafts never surface.
        assert_eq!(summary.pages_written, 4);
        assert!(!out.join("index.xml").exists(), "no [feed], no feed");
        let sitemap = fs::read_to_string(out.join("sitemap.xml")).expect("sitemap");
        assert!(
            sitemap.contains("https://example.com/posts/a/"),
            "got: {sitemap}"
        );
        assert!(!sitemap.contains("/b/"), "drafts excluded: {sitemap}");
    }

    #[test]
    fn feed_without_base_url_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        feed_site(dir.path(), "[feed]\nlimit = 5");
        let out = dir.path().join("out");
        let err = build_site_from_disk(dir.path(), &out).expect_err("fails");
        assert!(
            matches!(err, BuildError::Model { .. }),
            "explicit feeds need absolute URLs, got: {err:?}"
        );
    }

    #[test]
    fn feed_limit_truncates_and_excludes_drafts() {
        let dir = tempfile::tempdir().expect("tempdir");
        feed_site(
            dir.path(),
            "base_url = \"https://example.com\"\n[feed]\nlimit = 1",
        );
        write_site(
            dir.path(),
            &[(
                "content/posts/c.md",
                "---\ntitle: C\n---\n\nC has no date.\n",
            )],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let xml = fs::read_to_string(out.join("index.xml")).expect("feed");
        // Limit 1 keeps only the dated entry; the undated one is cut.
        // (<pubDate> omission for undated items is covered in rss unit tests.)
        assert!(xml.contains("<title>A</title>"), "got: {xml}");
        assert!(!xml.contains("<title>C</title>"), "limit truncates: {xml}");
        assert!(!xml.contains("B draft"), "drafts excluded: {xml}");
        // One item, one pubDate, content-derived lastBuildDate, no build time.
        assert_eq!(xml.matches("<item>").count(), 1);
        assert!(
            xml.contains("<pubDate>Wed, 02 Sep 2026 00:00:00 +0000</pubDate>"),
            "got: {xml}"
        );
        // No-trailing-slash base URLs join cleanly.
        assert!(xml.contains("https://example.com/posts/a/"), "got: {xml}");
    }

    fn menu_site(dir: &Path, menu_toml: &str) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    &format!(
                        "[site]\ntitle = \"T\"\n{menu_toml}\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n"
                    ),
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n"),
                (
                    "templates/post.html",
                    "<html><body>{% if menus %}<nav>{% for item in menus.main %}[{{ item.label }}:{{ item.url }}:{{ item.active }}]{% endfor %}</nav>{% endif %}{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>{% if menus %}<nav>{% for item in menus.main %}[{{ item.label }}:{{ item.url }}:{{ item.active }}]{% endfor %}</nav>{% endif %}section</body></html>",
                ),
            ],
        );
    }

    #[test]
    fn menu_renders_with_per_page_active_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        menu_site(
            dir.path(),
            "[menus.main]\nitems = [\n  { label = \"Home\", url = \"/\" },\n  { label = \"Posts\", url = \"/posts\" },\n  { label = \"Ext\", url = \"https://example.org/\" },\n]",
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        // Section page: its own item active, order preserved, external kept.
        // (MiniJinja escapes `/` as `&#x2f;` and renders bools as
        // `True`/`False`; both are engine behavior, not menu semantics.)
        let section = fs::read_to_string(out.join("posts/index.html")).expect("section");
        for expected in [
            "[Home:&#x2f;:False]",
            "[Posts:&#x2f;posts&#x2f;:True]",
            "[Ext:https:&#x2f;&#x2f;example.org&#x2f;:False]",
        ] {
            assert!(section.contains(expected), "missing {expected}: {section}");
        }
        // Entry page: exact-match semantics, so no item is active here.
        let entry = fs::read_to_string(out.join("posts/a/index.html")).expect("entry");
        for expected in [
            "[Home:&#x2f;:False]",
            "[Posts:&#x2f;posts&#x2f;:False]",
            "[Ext:https:&#x2f;&#x2f;example.org&#x2f;:False]",
        ] {
            assert!(entry.contains(expected), "missing {expected}: {entry}");
        }
    }

    #[test]
    fn menu_absent_without_configuration() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n"),
                (
                    "templates/post.html",
                    "<html><body>{% if menus %}NAV{% endif %}{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let entry = fs::read_to_string(out.join("posts/a/index.html")).expect("entry");
        assert!(!entry.contains("NAV"), "menus key must be absent: {entry}");
    }

    #[test]
    fn invalid_menu_fails_build_with_config_diagnostic() {
        let dir = tempfile::tempdir().expect("tempdir");
        menu_site(
            dir.path(),
            "[menus.main]\nitems = [\n  { label = \"Bad\", url = \"javascript:alert(1)\" },\n]",
        );
        let out = dir.path().join("out");
        let err = build_site_from_disk(dir.path(), &out).expect_err("must fail");
        match err {
            BuildError::Config { message, .. } => {
                assert!(message.contains("menu item"), "got: {message}");
                assert!(message.contains("javascript"), "got: {message}");
            }
            other => panic!("expected Config diagnostic, got: {other:?}"),
        }
    }

    #[test]
    fn static_assets_are_planned_and_resolved_through_the_pipeline() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(
            dir.path(),
            &[
                ("static/css/main.css", "body{}\n"),
                ("static/js/deep/app.js", "console.log(1);\n"),
            ],
        );
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");

        // Static assets are planned as specs: deterministic, nested, sorted.
        let static_paths: Vec<&str> = summary
            .specs
            .iter()
            .filter(|spec| spec.kind == ArtifactKind::Static)
            .map(|spec| spec.path.as_str())
            .collect();
        assert_eq!(static_paths, vec!["css/main.css", "js/deep/app.js"]);

        // Contents are copied byte-for-byte through the standard pipeline.
        assert_eq!(
            fs::read(out.join("css/main.css")).expect("css"),
            b"body{}\n"
        );
        assert_eq!(
            fs::read(out.join("js/deep/app.js")).expect("js"),
            b"console.log(1);\n"
        );
    }

    #[test]
    fn static_generated_path_collision_fails_before_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        // The search index always writes `index.json`; a static file at the
        // same path must fail validation, not depend on write order.
        write_site(dir.path(), &[("static/index.json", "[]")]);
        let out = dir.path().join("out");
        let err = build_site_from_disk(dir.path(), &out).expect_err("must collide");
        assert!(
            matches!(err, BuildError::OutputCollision { .. }),
            "got: {err:?}"
        );
        // Validation fails before the write loop: nothing was written.
        assert!(
            !out.exists() || fs::read_dir(&out).expect("out").next().is_none(),
            "no artifacts may be written when validation fails"
        );
    }

    #[test]
    fn removing_a_static_source_removes_its_planned_artifact() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(
            dir.path(),
            &[("static/a.css", "a{}\n"), ("static/b.css", "b{}\n")],
        );

        // Establish the baseline: both assets planned.
        let out1 = dir.path().join("out1");
        let first = build_site_from_disk(dir.path(), &out1).expect("builds");
        let static_count = |summary: &BuildSummary| {
            summary
                .specs
                .iter()
                .filter(|spec| spec.kind == ArtifactKind::Static)
                .count()
        };
        assert_eq!(static_count(&first), 2);

        // Deleting a source changes the planned artifact set — the plan
        // carries enough information to support deletion detection later.
        fs::remove_file(dir.path().join("static/a.css")).expect("remove");
        let out2 = dir.path().join("out2");
        let second = build_site_from_disk(dir.path(), &out2).expect("builds");
        assert_eq!(static_count(&second), 1);
        assert!(out2.join("b.css").exists());
        assert!(!out2.join("a.css").exists());
    }

    #[test]
    fn every_output_file_corresponds_to_an_artifact_spec() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(dir.path(), &[("static/css/main.css", "body{}\n")]);
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");

        let planned: std::collections::BTreeSet<String> =
            summary.specs.iter().map(|spec| spec.path.clone()).collect();
        let mut actual = std::collections::BTreeSet::new();
        for entry in walk_files(&out) {
            let rel = entry
                .strip_prefix(&out)
                .expect("relative")
                .to_string_lossy()
                .into_owned();
            // `.signal/` is build state, not a planned artifact — excluded
            // here by the same rule that keeps it out of sitemap and search.
            if rel == ".signal/manifest.json" || rel.starts_with(".signal/") {
                continue;
            }
            actual.insert(rel);
        }
        assert_eq!(actual, planned, "plan must describe the complete output");
    }

    #[test]
    fn empty_site_produces_valid_minimal_manifest() {
        use crate::manifest::{parse_manifest, MANIFEST_FILE};
        let dir = tempfile::tempdir().expect("tempdir");
        // No content, no static assets, no taxonomy, no feeds, no menus:
        // one empty collection with its templates.
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Empty\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        // Section page + search index only.
        assert_eq!(summary.pages_written, 2);

        let manifest = parse_manifest(
            &fs::read_to_string(out.join(".signal").join(MANIFEST_FILE)).expect("manifest"),
        )
        .expect("manifest parses");
        assert_eq!(manifest.schema_version, 1);
        assert!(manifest.entries.is_empty());
        assert!(manifest.queries.contains_key("summaries:posts"));
        assert!(manifest.queries.contains_key("search_documents"));
        let paths: Vec<&str> = manifest.artifacts.keys().map(String::as_str).collect();
        assert_eq!(paths, vec!["index.json", "posts/index.html"]);
        for record in manifest.artifacts.values() {
            assert!(!record.output_digest.as_str().is_empty());
        }
    }

    fn walk_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current)
                .expect("read dir")
                .filter_map(Result::ok)
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// Synthetic two-collection site for incremental tests: dated, tagged,
    /// and described entries plus one description-less entry (so body
    /// excerpts reach feeds), a static asset, feeds, taxonomy, and home.
    fn reuse_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Reuse\"\nbase_url = \"https://example.com/\"\nhome_collection = \"posts\"\n[taxonomy]\nroute_prefix = \"/topics/\"\n[feed]\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[collections.notes]\nsource = \"content/notes\"\nroute_prefix = \"/notes/\"\n",
                ),
                (
                    "content/posts/alpha.md",
                    "---\ntitle: Alpha\ndate: 2026-02-01\ndescription: About A.\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
                ),
                (
                    "content/posts/beta.md",
                    "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
                ),
                (
                    "content/notes/n.md",
                    "---\ntitle: Note\ndate: 2026-03-01\ntopics: [\"Misc\"]\n---\n\nNote body.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section {{ title }}</body></html>",
                ),
                ("templates/home.html", "<html><body>home</body></html>"),
                (
                    "templates/topics.html",
                    "<html><body>topics</body></html>",
                ),
                ("templates/topic.html", "<html><body>topic</body></html>"),
                ("static/asset.txt", "static-bytes\n"),
            ],
        );
    }

    #[test]
    fn isolated_resolution_matches_full_build_bytes() {
        // The core incremental-build invariant: any single ArtifactSpec can
        // be resolved independently and its bytes equal the full-build
        // output — across HTML, generated non-HTML, and static kinds.
        let root = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/sample-site"
        ));
        let out = tempfile::tempdir().expect("tempdir");
        let summary = build_site_from_disk(&root, out.path()).expect("builds");

        let config_path = root.join("signal.toml");
        let config: SignalConfig =
            toml::from_str(&fs::read_to_string(&config_path).expect("config"))
                .expect("config parses");
        let (model, _) = ingest_site(&root, &config).expect("model");
        let renderer = load_templates(&root).expect("renderer");

        // One artifact of each resolution family.
        let targets = [
            "index.html",
            "posts/alpha/index.html",
            "index.xml",
            "index.json",
            "images/alpha.svg",
        ];
        for path in targets {
            let spec = summary
                .specs
                .iter()
                .find(|spec| spec.path == path)
                .unwrap_or_else(|| panic!("spec {path} missing"));
            let isolated =
                resolve_artifact(spec, &config, &root, &model, &renderer).expect("resolves");
            let full = fs::read(out.path().join(path)).expect("full-build output");
            assert_eq!(isolated, full, "isolated bytes differ for {path}");
        }
    }

    #[test]
    fn static_resolution_is_isolated_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(dir.path(), &[("static/data/blob.bin", "binary-ish")]);
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        let config_path = dir.path().join("signal.toml");
        let config: SignalConfig =
            toml::from_str(&fs::read_to_string(&config_path).expect("config")).expect("parses");
        let (model, _) = ingest_site(dir.path(), &config).expect("model");
        let renderer = load_templates(dir.path()).expect("renderer");

        let spec = summary
            .specs
            .iter()
            .find(|spec| spec.path == "data/blob.bin")
            .expect("static spec");
        let bytes =
            resolve_artifact(spec, &config, dir.path(), &model, &renderer).expect("resolves");
        assert_eq!(bytes, b"binary-ish".to_vec());
    }

    #[test]
    fn identical_second_build_reuses_everything() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        let first = build_site_from_disk(dir.path(), &out).expect("first builds");
        assert_eq!(first.reused, 0);
        assert_eq!(first.rebuilt, first.specs.len());

        let second = build_site_from_disk(dir.path(), &out).expect("second builds");
        assert_eq!(second.rebuilt, 0, "nothing changed");
        assert_eq!(second.reused, second.specs.len());
        assert_eq!(second.pages_written, 0, "nothing rewritten");
        assert!(second.rebuilt_paths.is_empty());

        // Same inputs → same manifest bytes, not merely equivalent JSON.
        let first_manifest = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        build_site_from_disk(dir.path(), &out).expect("third builds");
        let third_manifest = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        assert_eq!(first_manifest, third_manifest);
    }

    #[test]
    fn missing_manifest_means_full_build() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        assert_eq!(summary.reused, 0);
        assert_eq!(summary.rebuilt, summary.specs.len());
    }

    #[test]
    fn corrupt_manifest_falls_back_to_full_build() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        fs::write(out.join(".signal/manifest.json"), "{not json").expect("corrupt");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds anyway");
        assert_eq!(summary.reused, 0);
        assert_eq!(summary.rebuilt, summary.specs.len());
        // A valid manifest replaces the corrupt one.
        assert!(crate::manifest::parse_manifest(
            &fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest")
        )
        .is_ok());
    }

    #[test]
    fn unsupported_schema_falls_back_to_full_build() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let mut text = fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest");
        text = text.replacen("\"schema_version\": 1", "\"schema_version\": 999", 1);
        fs::write(out.join(".signal/manifest.json"), text).expect("rewrite");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds anyway");
        assert_eq!(summary.reused, 0, "newer schemas are never read as v1");
        assert_eq!(summary.rebuilt, summary.specs.len());
    }

    /// Rewrite the on-disk manifest as compact JSON after mutating it; the
    /// build only cares about semantics, not formatting.
    fn rewrite_manifest(out: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
        let path = out.join(".signal/manifest.json");
        let text = fs::read_to_string(&path).expect("manifest");
        let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
        mutate(&mut value);
        fs::write(&path, serde_json::to_string(&value).expect("json")).expect("write manifest");
    }

    #[test]
    fn generation_behavior_mismatch_forces_full_rebuild() {
        use crate::manifest::GENERATION_BEHAVIOR_VERSION;
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // No source/config/template/static change at all: only the recorded
        // generation behavior is made incompatible.
        rewrite_manifest(&out, |value| {
            value["generation"]["behavior_version"] =
                serde_json::json!(GENERATION_BEHAVIOR_VERSION + 1);
        });
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.reused, 0, "generation mismatch must not reuse");
        assert_eq!(summary.rebuilt, summary.specs.len());
        assert!(summary.pruned_paths.is_empty(), "the plan is unchanged");
        // The rewritten manifest carries the current identity again, so the
        // next build returns to full reuse.
        assert_eq!(
            read_manifest(&out).generation,
            Some(crate::manifest::current_generation())
        );
        let third = build_site_from_disk(dir.path(), &out).expect("third builds");
        assert_eq!(third.rebuilt, 0);
        assert_eq!(third.reused, third.specs.len());
    }

    #[test]
    fn engine_version_mismatch_forces_full_rebuild() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        rewrite_manifest(&out, |value| {
            value["generation"]["engine_version"] = serde_json::json!("0.0.0-other");
        });
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.reused, 0);
        assert_eq!(summary.rebuilt, summary.specs.len());
    }

    #[test]
    fn missing_generation_identity_forces_full_rebuild() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        // A pre-field manifest: schema-compatible, but with no generation
        // identity, so compatibility cannot be established.
        rewrite_manifest(&out, |value| {
            value.as_object_mut().expect("object").remove("generation");
        });
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds safely");
        assert_eq!(summary.reused, 0);
        assert_eq!(summary.rebuilt, summary.specs.len());
        // Rebuilding records the identity, so the next build is full reuse.
        assert!(read_manifest(&out).generation.is_some());
        let third = build_site_from_disk(dir.path(), &out).expect("third builds");
        assert_eq!(third.rebuilt, 0);
        assert_eq!(third.reused, third.specs.len());
    }

    #[test]
    fn legacy_manifest_disables_reuse_but_still_prunes_safely() {
        // A pre-field manifest cannot prove reuse compatibility, but its
        // artifact inventory is still valid for stale reconciliation.
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        rewrite_manifest(&out, |value| {
            value.as_object_mut().expect("object").remove("generation");
        });
        fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete source");

        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds safely");
        assert_eq!(summary.reused, 0, "no identity, no reuse");
        assert!(
            summary
                .pruned_paths
                .contains(&"posts/alpha/index.html".to_string()),
            "stale inventory must still reconcile: {:?}",
            summary.pruned_paths
        );
        assert!(!out.join("posts/alpha/index.html").exists());
    }

    #[test]
    fn title_change_rebuilds_manifest_dependents() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Alpha's title changes: entry digest, summaries, home, topic
        // terms, feeds, and search all embed it. The sitemap records only
        // routes and lastmod dates, so it must stay reusable.
        fs::write(
            dir.path().join("content/posts/alpha.md"),
            "---\ntitle: Alpha Renamed\ndate: 2026-02-01\ndescription: About A.\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        assert_eq!(
            rebuilt,
            vec![
                "index.html",
                "index.json",
                "index.xml",
                "posts/alpha/index.html",
                "posts/index.html",
                "topics/index.html",
                "topics/rust/index.html",
                "topics/rust/index.xml",
            ]
        );
    }

    #[test]
    fn body_change_rebuilds_page_search_and_excerpt_feeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Beta has no description, so feeds carry its body excerpt: the body
        // edit must reach the page, search, and both feeds containing beta —
        // but summaries embed no body text (reading time stays 1 minute), so
        // listings, terms, topics index, and sitemap stay reusable.
        fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta body with extra words here.\n",
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        assert_eq!(
            rebuilt,
            vec![
                "index.json",
                "index.xml",
                "posts/beta/index.html",
                "topics/rust/index.xml",
            ]
        );
    }

    #[test]
    fn tag_change_rebuilds_taxonomy_without_touching_main_feed() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Beta gains Systems: its page, posts listing, home, topics index,
        // both affected term pages, the new Systems term feed, search (tags
        // are indexed), and the sitemap (the new term route joins the
        // inventory) rebuild. The main feed carries no tags, so it stays
        // reusable — precision the manifest model gives for free.
        fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\", \"Systems\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        assert_eq!(
            rebuilt,
            vec![
                "index.html",
                "index.json",
                "posts/beta/index.html",
                "posts/index.html",
                "sitemap.xml",
                "topics/index.html",
                "topics/rust/index.html",
                "topics/systems/index.html",
                "topics/systems/index.xml",
            ]
        );
        // And the new term feed actually exists with beta in it.
        let systems = fs::read_to_string(out.join("topics/systems/index.xml")).expect("feed");
        assert!(systems.contains("/posts/beta/"), "got: {systems}");
    }

    #[test]
    fn date_change_rebuilds_ordered_consumers_including_sitemap() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Beta moves to newest: its page, every date-ordered listing, both
        // feeds, search, and the sitemap (date is the lastmod fallback).
        fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta\ndate: 2026-05-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        assert_eq!(
            rebuilt,
            vec![
                "index.html",
                "index.json",
                "index.xml",
                "posts/beta/index.html",
                "posts/index.html",
                "sitemap.xml",
                "topics/index.html",
                "topics/rust/index.html",
                "topics/rust/index.xml",
            ]
        );
    }

    /// Site whose entry template exercises inheritance and nested includes.
    fn template_closure_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Closure\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nBody A.\n"),
                (
                    "templates/post.html",
                    "{% extends \"base.html\" %}{% block body %}{{ content | safe }}{% endblock %}",
                ),
                (
                    "templates/base.html",
                    "<html><body>BASE-1{% include \"partials/nav.html\" %}{% block body %}{% endblock %}</body></html>",
                ),
                (
                    "templates/partials/nav.html",
                    "<nav>NAV-1{% include \"partials/badge.html\" %}</nav>",
                ),
                ("templates/partials/badge.html", "<span>BADGE-1</span>"),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    #[test]
    fn extends_parent_change_invalidates_the_artifact() {
        let dir = tempfile::tempdir().expect("tempdir");
        template_closure_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let page = out.join("posts/a/index.html");
        assert!(fs::read_to_string(&page).unwrap().contains("BASE-1"));

        // Nothing changed: full reuse (the set digest is stable).
        let noop = build_site_from_disk(dir.path(), &out).expect("second builds");
        assert_eq!(noop.rebuilt, 0, "unchanged templates must reuse");

        // Change only the `{% extends %}` parent.
        fs::write(
            dir.path().join("templates/base.html"),
            "<html><body>BASE-2{% include \"partials/nav.html\" %}{% block body %}{% endblock %}</body></html>",
        )
        .unwrap();
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert!(
            summary
                .rebuilt_paths
                .contains(&"posts/a/index.html".to_string()),
            "extends parent change must rebuild: {:?}",
            summary.rebuilt_paths
        );
        assert!(
            fs::read_to_string(&page).unwrap().contains("BASE-2"),
            "rebuilt output must reflect the new parent"
        );
    }

    #[test]
    fn included_partial_change_invalidates_the_artifact() {
        let dir = tempfile::tempdir().expect("tempdir");
        template_closure_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let page = out.join("posts/a/index.html");
        assert!(fs::read_to_string(&page).unwrap().contains("NAV-1"));

        // Change only a directly included partial.
        fs::write(
            dir.path().join("templates/partials/nav.html"),
            "<nav>NAV-2{% include \"partials/badge.html\" %}</nav>",
        )
        .unwrap();
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert!(
            summary
                .rebuilt_paths
                .contains(&"posts/a/index.html".to_string()),
            "include change must rebuild: {:?}",
            summary.rebuilt_paths
        );
        assert!(fs::read_to_string(&page).unwrap().contains("NAV-2"));
    }

    #[test]
    fn nested_include_change_invalidates_and_is_recorded() {
        let dir = tempfile::tempdir().expect("tempdir");
        template_closure_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let page = out.join("posts/a/index.html");
        assert!(fs::read_to_string(&page).unwrap().contains("BADGE-1"));

        // Change only a transitively included partial (nav includes badge).
        fs::write(
            dir.path().join("templates/partials/badge.html"),
            "<span>BADGE-2</span>",
        )
        .unwrap();
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert!(
            summary
                .rebuilt_paths
                .contains(&"posts/a/index.html".to_string()),
            "nested include change must rebuild: {:?}",
            summary.rebuilt_paths
        );
        assert!(
            fs::read_to_string(&page).unwrap().contains("BADGE-2"),
            "rebuilt output must reflect the nested change"
        );

        // The manifest records the complete consumed template set, so reuse
        // cannot be fooled by an untracked transitive dependency.
        let manifest = read_manifest(&out);
        let record = manifest
            .artifacts
            .get("posts/a/index.html")
            .expect("manifest record");
        assert!(
            record
                .inputs
                .iter()
                .any(|input| matches!(input, crate::manifest::InputRef::TemplateSet)),
            "record must name the template set: {:?}",
            record.inputs
        );
        for name in [
            "post.html",
            "base.html",
            "partials/nav.html",
            "partials/badge.html",
        ] {
            assert!(
                manifest.templates.contains_key(name),
                "template {name} must be recorded"
            );
        }
    }

    #[test]
    fn unused_template_change_also_rebuilds_template_consumers_by_design() {
        // Documented trade-off of the conservative template-set dependency:
        // any loaded template change invalidates template-rendered artifacts,
        // even when the template is not in an artifact's logical closure.
        let dir = tempfile::tempdir().expect("tempdir");
        template_closure_site(dir.path());
        write_site(dir.path(), &[("templates/unused.html", "<i>unused-1</i>")]);
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::write(dir.path().join("templates/unused.html"), "<i>unused-2</i>").unwrap();
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert!(
            summary
                .rebuilt_paths
                .contains(&"posts/a/index.html".to_string()),
            "conservative template set invalidates rendered artifacts: {:?}",
            summary.rebuilt_paths
        );
        // And non-template artifacts remain reusable.
        assert!(!summary.rebuilt_paths.contains(&"index.json".to_string()));
    }

    #[test]
    fn template_change_rebuilds_all_template_consumers() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::write(
            dir.path().join("templates/post.html"),
            "<html><body>CHANGED {{ content | safe }}</body></html>",
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        // Slice 14C: template-rendered artifacts depend on the complete loaded
        // template set, so a change to any template rebuilds all of them. This
        // is the deliberate precision trade-off that keeps `{% extends %}` /
        // `{% include %}` invalidation sound without a template parser.
        assert_eq!(
            rebuilt,
            vec![
                "index.html",
                "notes/index.html",
                "notes/n/index.html",
                "posts/alpha/index.html",
                "posts/beta/index.html",
                "posts/index.html",
                "topics/index.html",
                "topics/misc/index.html",
                "topics/rust/index.html",
            ]
        );
        // Non-template artifacts stay reusable: feeds, sitemap, search, and
        // statics record no template input.
        for path in [
            "index.json",
            "index.xml",
            "sitemap.xml",
            "asset.txt",
            "topics/misc/index.xml",
            "topics/rust/index.xml",
        ] {
            assert!(
                !summary.rebuilt_paths.contains(&path.to_string()),
                "{path} must stay reusable: {:?}",
                summary.rebuilt_paths
            );
        }
    }

    #[test]
    fn config_title_change_rebuilds_config_consumers_but_not_search() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        let config_path = dir.path().join("signal.toml");
        let config = fs::read_to_string(&config_path).expect("config");
        fs::write(
            config_path,
            config.replace("title = \"Reuse\"", "title = \"Renamed\""),
        )
        .expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        // Whole-config digest: every Config-input artifact rebuilds. Search
        // records only its query and statics only their source, so both stay
        // reusable — the coarse model is still precise where inputs are
        // genuinely independent.
        assert!(!summary.rebuilt_paths.contains(&"index.json".to_string()));
        assert!(!summary.rebuilt_paths.contains(&"asset.txt".to_string()));
        assert!(summary.rebuilt_paths.contains(&"index.xml".to_string()));
        assert!(summary.rebuilt_paths.contains(&"sitemap.xml".to_string()));
        assert!(summary
            .rebuilt_paths
            .contains(&"posts/alpha/index.html".to_string()));
        assert_eq!(
            summary.reused, 2,
            "only search and static reuse: {:?}",
            summary.rebuilt_paths
        );
    }

    #[test]
    fn static_change_rebuilds_only_that_asset() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::write(dir.path().join("static/asset.txt"), "changed-bytes\n").expect("edit");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.rebuilt_paths, vec!["asset.txt".to_string()]);
        assert_eq!(summary.reused, summary.specs.len() - 1);
        assert_eq!(
            fs::read(out.join("asset.txt")).expect("asset"),
            b"changed-bytes\n"
        );
    }

    #[test]
    fn corrupted_output_is_rebuilt() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::write(out.join("posts/alpha/index.html"), "<p>tampered</p>").expect("tamper");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(
            summary.rebuilt_paths,
            vec!["posts/alpha/index.html".to_string()]
        );
        assert!(fs::read_to_string(out.join("posts/alpha/index.html"))
            .expect("page")
            .contains("Alpha body words here."));
    }

    #[test]
    fn deleted_output_is_rebuilt() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::remove_file(out.join("posts/alpha/index.html")).expect("delete");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(
            summary.rebuilt_paths,
            vec!["posts/alpha/index.html".to_string()]
        );
        assert!(out.join("posts/alpha/index.html").exists());
    }

    #[test]
    fn new_content_generates_only_its_dependents() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Gamma is old, untagged, and undescribed: it joins listings, home,
        // search, main feed, and sitemap — nothing taxonomy-related.
        fs::write(
            dir.path().join("content/posts/gamma.md"),
            "---\ntitle: Gamma\ndate: 2026-01-02\n---\n\nGamma body.\n",
        )
        .expect("new file");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut rebuilt = summary.rebuilt_paths.clone();
        rebuilt.sort();
        assert_eq!(
            rebuilt,
            vec![
                "index.html",
                "index.json",
                "index.xml",
                "posts/gamma/index.html",
                "posts/index.html",
                "sitemap.xml",
            ]
        );
    }

    #[test]
    fn read_only_outputs_prove_reuse_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Read-only outputs: any attempted rewrite fails, so success proves
        // reuse performed zero writes. (The manifest directory itself stays
        // writable — only artifact files are locked.)
        for entry in walk_files(&out) {
            if entry.extension().is_some_and(|ext| ext != "json")
                || !entry.to_string_lossy().ends_with("manifest.json")
            {
                let mut permissions = fs::metadata(&entry).expect("meta").permissions();
                permissions.set_readonly(true);
                fs::set_permissions(&entry, permissions).expect("chmod");
            }
        }
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.rebuilt, 0);
        assert_eq!(summary.reused, summary.specs.len());
    }

    #[test]
    fn clean_and_incremental_builds_are_byte_identical() {
        // The mandatory equivalence: same second state built incrementally
        // and from scratch must agree on public output AND manifest bytes.
        let template = tempfile::tempdir().expect("tempdir");
        reuse_site(template.path());
        let state = tempfile::tempdir().expect("tempdir");
        copy_fixture_tree(template.path(), state.path());

        let incremental = tempfile::tempdir().expect("tempdir");
        build_site_from_disk(state.path(), incremental.path()).expect("first builds");
        // Mutate: retitle beta and touch the stylesheet.
        fs::write(
            state.path().join("content/posts/beta.md"),
            "---\ntitle: Beta Two\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        fs::write(state.path().join("static/asset.txt"), "v2\n").expect("edit");
        let incremental_summary =
            build_site_from_disk(state.path(), incremental.path()).expect("incremental builds");
        assert!(incremental_summary.rebuilt > 0);
        assert!(incremental_summary.reused > 0);

        let clean = tempfile::tempdir().expect("tempdir");
        let clean_summary = build_site_from_disk(state.path(), clean.path()).expect("clean builds");
        assert_eq!(clean_summary.rebuilt, clean_summary.specs.len());
        assert_eq!(clean_summary.reused, 0);

        assert_eq!(
            snapshot_public_tree(incremental.path()),
            snapshot_public_tree(clean.path()),
            "public output must not reveal reuse"
        );
        assert_eq!(
            fs::read(incremental.path().join(".signal/manifest.json")).expect("manifest"),
            fs::read(clean.path().join(".signal/manifest.json")).expect("manifest"),
            "manifest must not reveal reuse"
        );
    }

    /// Test-only context: model, config, and renderer for one temp site,
    /// plus its output directory after a full build.
    struct ReuseFixture {
        _dir: tempfile::TempDir,
        _out: tempfile::TempDir,
        root: PathBuf,
        out: PathBuf,
        config: SignalConfig,
        model: SiteModel,
        renderer: MiniJinjaRenderer,
        manifest: crate::manifest::Manifest,
        specs: Vec<ArtifactSpec>,
    }

    impl ReuseFixture {
        fn build() -> Self {
            use crate::manifest::parse_manifest;
            let dir = tempfile::tempdir().expect("tempdir");
            reuse_site(dir.path());
            let out = tempfile::tempdir().expect("tempdir");
            let summary = build_site_from_disk(dir.path(), out.path()).expect("builds");
            let config_path = dir.path().join("signal.toml");
            let config: SignalConfig =
                toml::from_str(&fs::read_to_string(&config_path).expect("config"))
                    .expect("config parses");
            let (model, _) = ingest_site(dir.path(), &config).expect("model");
            let renderer = load_templates(dir.path()).expect("renderer");
            let manifest = parse_manifest(
                &fs::read_to_string(out.path().join(".signal/manifest.json")).expect("manifest"),
            )
            .expect("manifest parses");
            Self {
                root: dir.path().to_path_buf(),
                out: out.path().to_path_buf(),
                config,
                model,
                renderer,
                manifest,
                specs: summary.specs,
                _dir: dir,
                _out: out,
            }
        }

        fn decide(&self, path: &str, manifest: &crate::manifest::Manifest) -> bool {
            use crate::manifest::{artifact_reusable, config_digest, template_digests};
            let spec = self
                .specs
                .iter()
                .find(|s| s.path == path)
                .expect("spec exists");
            artifact_reusable(
                spec,
                &self.config,
                &self.root,
                &self.model,
                &template_digests(&self.renderer).expect("digests"),
                &config_digest(&self.config),
                manifest,
                &self.out,
            )
        }
    }

    #[test]
    fn reuse_decisions_come_from_manifest_records() {
        // Architecture test (§34): decisions consult manifest input
        // references and current digests — no graph, no hard-coded edges.
        // Tampering with ONLY the manifest must flip exactly the dependent
        // decision, with sources untouched.
        let fx = ReuseFixture::build();
        assert!(fx.decide("posts/alpha/index.html", &fx.manifest));

        // Tampered query digest → dependents of that query rebuild.
        // The alpha *page* names Entry+Template+Config only, so it is
        // unaffected by a summaries tamper; the section page names the
        // query and must rebuild. This is exactly manifest-driven
        // propagation with no hard-coded edges.
        let mut tampered = fx.manifest.clone();
        tampered.queries.insert(
            "summaries:posts".to_string(),
            crate::manifest::digest_bytes(b"tampered"),
        );
        assert!(!fx.decide("posts/index.html", &tampered));
        assert!(fx.decide("posts/alpha/index.html", &tampered));
        assert!(fx.decide("notes/n/index.html", &tampered));
        assert!(fx.decide("asset.txt", &tampered));

        // Tampered entry digest → only artifacts naming that entry rebuild.
        let mut tampered_entry = fx.manifest.clone();
        tampered_entry.entries.insert(
            "/posts/alpha/".to_string(),
            crate::manifest::digest_bytes(b"tampered"),
        );
        assert!(!fx.decide("posts/alpha/index.html", &tampered_entry));
        assert!(fx.decide("posts/index.html", &tampered_entry));

        // Deleted manifest record → rebuild even though everything matches.
        let mut pruned = fx.manifest.clone();
        pruned.artifacts.remove("posts/alpha/index.html");
        assert!(!fx.decide("posts/alpha/index.html", &pruned));

        // Kind change at the same path → rebuild, never reuse across kinds.
        let mut rekindled = fx.manifest.clone();
        if let Some(record) = rekindled.artifacts.get_mut("posts/alpha/index.html") {
            record.kind = ArtifactKind::CollectionIndex;
        }
        assert!(!fx.decide("posts/alpha/index.html", &rekindled));
    }

    #[test]
    fn content_deletion_prunes_article_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("posts/alpha/index.html").exists());

        fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete source");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.pruned, 1);
        assert_eq!(summary.pruned_paths, vec!["posts/alpha/index.html"]);
        assert!(!out.join("posts/alpha/index.html").exists());
        // The stale record drops out of the new manifest.
        let manifest = read_manifest(&out);
        assert!(!manifest.artifacts.contains_key("posts/alpha/index.html"));
        // Unrelated content is untouched and reusable.
        assert!(out.join("posts/beta/index.html").exists());
        assert!(out.join("notes/n/index.html").exists());
    }

    #[test]
    fn taxonomy_term_disappearance_prunes_term_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("topics/misc/index.html").exists());
        assert!(out.join("topics/misc/index.xml").exists());

        // n.md is the sole Misc member: the term pages vanish from the plan.
        fs::remove_file(dir.path().join("content/notes/n.md")).expect("delete source");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        let mut pruned = summary.pruned_paths.clone();
        pruned.sort();
        assert_eq!(
            pruned,
            vec![
                "notes/n/index.html",
                "topics/misc/index.html",
                "topics/misc/index.xml",
            ]
        );
        assert!(!out.join("topics/misc/index.html").exists());
        assert!(!out.join("topics/misc/index.xml").exists());
        // The collection section remains (rebuilt with an empty listing).
        assert!(out.join("notes/index.html").exists());
        let manifest = read_manifest(&out);
        assert!(!manifest.artifacts.contains_key("topics/misc/index.html"));
        assert!(!manifest.artifacts.contains_key("topics/misc/index.xml"));
    }

    #[test]
    fn static_deletion_prunes_asset_and_preserves_unknown_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        // An unknown file the manifest never recorded must survive pruning.
        fs::write(out.join("manual.txt"), "handmade").expect("manual file");

        fs::remove_file(dir.path().join("static/asset.txt")).expect("delete source");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.pruned, 1);
        assert_eq!(summary.pruned_paths, vec!["asset.txt"]);
        assert!(!out.join("asset.txt").exists());
        assert_eq!(
            fs::read_to_string(out.join("manual.txt")).expect("manual"),
            "handmade"
        );
        let manifest = read_manifest(&out);
        assert!(!manifest.artifacts.contains_key("asset.txt"));
    }

    #[test]
    fn route_change_generates_new_and_prunes_old() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("posts/beta/index.html").exists());

        // A slug change is a new artifact plus a stale artifact — never a
        // dependency-graph "move".
        fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta\ndate: 2026-01-01\nslug: beta-renamed\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("rename via slug");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert!(out.join("posts/beta-renamed/index.html").exists());
        assert!(!out.join("posts/beta/index.html").exists());
        assert_eq!(summary.pruned_paths, vec!["posts/beta/index.html"]);
    }

    #[test]
    fn multiple_stale_artifacts_pruned_deterministically() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");
        fs::remove_file(dir.path().join("content/notes/n.md")).expect("delete");
        fs::remove_file(dir.path().join("static/asset.txt")).expect("delete");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        // Sorted by construction; the Rust term page survives on beta.
        assert_eq!(
            summary.pruned_paths,
            vec![
                "asset.txt",
                "notes/n/index.html",
                "posts/alpha/index.html",
                "topics/misc/index.html",
                "topics/misc/index.xml",
            ]
        );
        assert_eq!(summary.pruned, 5);
        assert!(out.join("topics/rust/index.html").exists());
    }

    #[test]
    fn nested_static_paths_prune_only_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        write_site(
            dir.path(),
            &[("static/a/b/c.txt", "c\n"), ("static/a/b/d.txt", "d\n")],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        build_site_from_disk(dir.path(), &out).expect("second builds");

        fs::remove_file(dir.path().join("static/a/b/c.txt")).expect("delete");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.pruned_paths, vec!["a/b/c.txt"]);
        assert!(!out.join("a/b/c.txt").exists());
        // The sibling survives and empty parent directories are left alone.
        assert!(out.join("a/b/d.txt").exists());
        assert!(out.join("a/b").is_dir());
        assert!(out.join("a").is_dir());
    }

    #[test]
    fn tampered_manifest_paths_cannot_escape() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Sentinels an escaped deletion would destroy: a sibling of the
        // output dir, and a source file inside the site root.
        let outside = out.parent().expect("parent").join("outside-sentinel.txt");
        fs::write(&outside, "outside").expect("sentinel");
        let manifest_path = out.join(".signal/manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("manifest"))
                .expect("manifest parses");
        let evil = |path: &str| {
            serde_json::json!({
                "kind": "Static",
                "inputs": [],
                "output_digest": "0".repeat(64),
                "_path": path,
            })
        };
        // Rewrite records keyed by hostile paths (BTreeMap order keeps the
        // file deterministic; content here is adversarial by construction).
        let artifacts = value
            .get_mut("artifacts")
            .expect("artifacts")
            .as_object_mut()
            .expect("map");
        artifacts.insert("../outside.txt".to_string(), evil("../outside.txt"));
        artifacts.insert("/tmp/signal-absolute-evil.txt".to_string(), evil("/tmp/x"));
        artifacts.insert("a/../../b.txt".to_string(), evil("a/../../b.txt"));
        artifacts.insert("..\\win.txt".to_string(), evil("..\\win.txt"));
        fs::write(
            &manifest_path,
            serde_json::to_string(&value).expect("serialize"),
        )
        .expect("tamper manifest");

        let summary = build_site_from_disk(dir.path(), &out).expect("builds despite tampering");
        assert!(!summary
            .pruned_paths
            .iter()
            .any(|p| p.contains("..") || p.starts_with('/')));
        // Nothing outside the output root was touched; sources are intact.
        assert_eq!(fs::read_to_string(&outside).expect("sentinel"), "outside");
        assert!(dir.path().join("content/posts/alpha.md").exists());
        // Self-healing: the rewritten manifest contains only real artifacts.
        let fresh: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("manifest"))
                .expect("manifest parses");
        let fresh_artifacts = fresh
            .get("artifacts")
            .expect("artifacts")
            .as_object()
            .expect("map");
        assert!(!fresh_artifacts
            .keys()
            .any(|k| k.contains("..") || k.starts_with('/')));
        fs::remove_file(&outside).expect("cleanup sentinel");
    }

    #[test]
    fn directory_at_stale_path_fails_safely() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let manifest_before = fs::read(out.join(".signal/manifest.json")).expect("manifest before");

        // Replace a stale-bound output with a directory tree: pruning must
        // refuse recursive deletion and fail the build instead.
        fs::remove_file(dir.path().join("static/asset.txt")).expect("delete source");
        fs::remove_file(out.join("asset.txt")).expect("remove output");
        fs::create_dir_all(out.join("asset.txt")).expect("mkdir");
        fs::write(out.join("asset.txt/inner.txt"), "inner").expect("inner file");
        let err = build_site_from_disk(dir.path(), &out).expect_err("must fail safely");
        let message = format!("{err:?}");
        assert!(message.contains("asset.txt"), "got: {message}");
        // Nothing was deleted and the old manifest still describes the last
        // successful build.
        assert!(out.join("asset.txt/inner.txt").exists());
        assert_eq!(
            fs::read(out.join(".signal/manifest.json")).expect("manifest after"),
            manifest_before
        );
    }

    #[test]
    fn invalid_manifest_disables_pruning() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // alpha.md disappears, but the corrupt manifest cannot name anything
        // stale: full build regenerates, stale output is left alone.
        fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete source");
        fs::write(out.join(".signal/manifest.json"), "{not json").expect("corrupt");
        let summary = build_site_from_disk(dir.path(), &out).expect("builds anyway");
        assert_eq!(summary.pruned, 0);
        assert!(summary.pruned_paths.is_empty());
        assert!(
            out.join("posts/alpha/index.html").exists(),
            "stale output survives an untrusted manifest"
        );
        // The new manifest is valid again.
        read_manifest(&out);
    }

    #[test]
    fn missing_stale_output_is_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Source and output both vanish before the rebuild: nothing to
        // delete, nothing to fail.
        fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete source");
        fs::remove_file(out.join("posts/alpha/index.html")).expect("delete output");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.pruned, 0);
        assert!(summary.pruned_paths.is_empty());
    }

    #[test]
    fn clean_incremental_equivalence_with_disappearance() {
        // State A has content that state B removes; incremental and clean
        // builds of B must agree byte-for-byte, with stale outputs absent
        // from both.
        let template = tempfile::tempdir().expect("tempdir");
        reuse_site(template.path());
        write_site(
            template.path(),
            &[
                (
                    "content/posts/gamma.md",
                    "---\ntitle: Gamma\ndate: 2026-04-01\ntopics: [\"Rust\"]\n---\n\nGamma body.\n",
                ),
                ("static/extra.txt", "extra\n"),
            ],
        );
        let state = tempfile::tempdir().expect("tempdir");
        copy_fixture_tree(template.path(), state.path());

        let incremental = tempfile::tempdir().expect("tempdir");
        build_site_from_disk(state.path(), incremental.path()).expect("state A builds");
        assert!(incremental.path().join("posts/gamma/index.html").exists());
        assert!(incremental.path().join("extra.txt").exists());

        fs::remove_file(state.path().join("content/posts/gamma.md")).expect("delete");
        fs::remove_file(state.path().join("static/extra.txt")).expect("delete");
        let inc_summary =
            build_site_from_disk(state.path(), incremental.path()).expect("state B builds");
        assert!(inc_summary
            .pruned_paths
            .contains(&"posts/gamma/index.html".to_string()));
        assert!(inc_summary.pruned_paths.contains(&"extra.txt".to_string()));
        assert!(!incremental.path().join("posts/gamma/index.html").exists());
        assert!(!incremental.path().join("extra.txt").exists());

        let clean = tempfile::tempdir().expect("tempdir");
        build_site_from_disk(state.path(), clean.path()).expect("clean builds");
        assert_eq!(
            snapshot_public_tree(incremental.path()),
            snapshot_public_tree(clean.path()),
            "public output must not reveal reuse or pruning"
        );
        assert_eq!(
            fs::read(incremental.path().join(".signal/manifest.json")).expect("manifest"),
            fs::read(clean.path().join(".signal/manifest.json")).expect("manifest"),
            "manifest must not reveal reuse or pruning"
        );
        assert!(!clean.path().join("posts/gamma/index.html").exists());
        assert!(!clean.path().join("extra.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_stale_path_removes_link_not_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Swap a stale-bound output for a symlink pointing outside the
        // output root: pruning must remove the link itself, never follow it.
        let outside = dir.path().join("outside-secret.txt");
        fs::write(&outside, "secret").expect("sentinel");
        fs::remove_file(dir.path().join("static/asset.txt")).expect("delete source");
        fs::remove_file(out.join("asset.txt")).expect("remove output");
        symlink(&outside, out.join("asset.txt")).expect("symlink");
        let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
        assert_eq!(summary.pruned_paths, vec!["asset.txt"]);
        assert!(!out.join("asset.txt").exists());
        assert_eq!(
            fs::read_to_string(&outside).expect("sentinel intact"),
            "secret"
        );
    }

    // --- Slice 14D: filesystem path identity and symlink hardening ---

    fn filesystem_is_case_insensitive(probe_dir: &Path) -> bool {
        let lower = probe_dir.join("signal-fs-case-probe");
        if fs::write(&lower, b"x").is_err() {
            return false;
        }
        let insensitive = probe_dir.join("SIGNAL-FS-CASE-PROBE").exists();
        let _ = fs::remove_file(&lower);
        insensitive
    }

    fn filesystem_normalizes_unicode(probe_dir: &Path) -> bool {
        let composed = probe_dir.join("signal-caf\u{e9}-probe");
        if fs::write(&composed, b"x").is_err() {
            return false;
        }
        let normalizes = probe_dir.join("signal-cafe\u{301}-probe").exists();
        let _ = fs::remove_file(&composed);
        normalizes
    }

    /// One-entry site used by the alias tests, parameterized by slug.
    fn single_post_site(dir: &Path, slug: &str) {
        let front_matter = format!("---\ntitle: A\nslug: \"{slug}\"\n---\n\nBody A.\n");
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Alias\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", &front_matter),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_symlink_in_output_fails_writes_safely() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");

        // Move the posts subtree outside and redirect `out/posts` to it.
        let moved = dir.path().join("outside-posts");
        fs::create_dir_all(&moved).unwrap();
        fs::rename(out.join("posts"), moved.join("posts")).unwrap();
        symlink(moved.join("posts"), out.join("posts")).unwrap();
        let sentinel = moved.join("posts/alpha/index.html");
        let before = fs::read(&sentinel).unwrap();

        // A title change forces the entry page to be rewritten into `posts/`.
        fs::write(
            dir.path().join("content/posts/alpha.md"),
            "---\ntitle: Alpha Two\ndate: 2026-02-01\ndescription: About A.\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
        )
        .unwrap();
        let err = build_site_from_disk(dir.path(), &out)
            .expect_err("writing through a symlinked ancestor must fail");
        assert!(format!("{err:?}").contains("symlink"), "got: {err:?}");
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            before,
            "no write may escape through the symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_symlink_prune_fails_safely() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        reuse_site(dir.path());
        write_site(dir.path(), &[("static/legacy/old.txt", "legacy-bytes\n")]);
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("legacy/old.txt").exists());

        // Redirect `out/legacy` to an outside tree holding the same file.
        let outside = dir.path().join("outside-legacy");
        fs::create_dir_all(&outside).unwrap();
        fs::rename(out.join("legacy"), outside.join("legacy")).unwrap();
        symlink(outside.join("legacy"), out.join("legacy")).unwrap();
        let sentinel = outside.join("legacy/old.txt");
        let before = fs::read(&sentinel).unwrap();

        // Deleting the source makes the artifact stale so pruning runs.
        fs::remove_file(dir.path().join("static/legacy/old.txt")).unwrap();
        let err = build_site_from_disk(dir.path(), &out)
            .expect_err("pruning through a symlinked ancestor must fail safely");
        assert!(format!("{err:?}").contains("symlink"), "got: {err:?}");
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            before,
            "outside file must survive"
        );
    }

    #[test]
    fn case_alias_of_current_artifact_is_never_pruned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        single_post_site(&site, "Alpha");
        let out = dir.path().join("out");
        build_site_from_disk(&site, &out).expect("first builds");
        assert!(out.join("posts/Alpha/index.html").exists());

        // Same content, slug case changed: a new logical path that may be the
        // same filesystem object as the old one.
        single_post_site(&site, "alpha");
        let summary = build_site_from_disk(&site, &out).expect("rebuilds");

        assert!(
            out.join("posts/alpha/index.html").exists(),
            "the current artifact must survive"
        );
        if filesystem_is_case_insensitive(dir.path()) {
            // The stale `posts/Alpha/...` is the current artifact's object:
            // it must be skipped, never deleted.
            assert!(
                summary.pruned_paths.is_empty(),
                "case alias must not be pruned: {:?}",
                summary.pruned_paths
            );
        } else {
            assert_eq!(
                summary.pruned_paths,
                vec!["posts/Alpha/index.html".to_string()],
                "case-sensitive hosts prune the genuinely separate old path"
            );
        }
    }

    #[test]
    fn unicode_alias_of_current_artifact_is_never_pruned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        single_post_site(&site, "caf\u{e9}"); // composed (NFC)
        let out = dir.path().join("out");
        build_site_from_disk(&site, &out).expect("first builds");
        assert!(out.join("posts/caf\u{e9}/index.html").exists());

        // Decomposed spelling (NFD) of the same name.
        single_post_site(&site, "cafe\u{301}");
        let summary = build_site_from_disk(&site, &out).expect("rebuilds");

        assert!(
            out.join("posts/cafe\u{301}/index.html").exists(),
            "the current artifact must survive"
        );
        if filesystem_normalizes_unicode(dir.path()) {
            assert!(
                summary.pruned_paths.is_empty(),
                "Unicode alias must not be pruned: {:?}",
                summary.pruned_paths
            );
        } else {
            assert_eq!(
                summary.pruned_paths,
                vec!["posts/caf\u{e9}/index.html".to_string()],
                "normalization-sensitive hosts prune the separate old path"
            );
        }
    }

    // --- Slice 15B: current/current filesystem-alias rejection (R14-2) ---

    /// Two sources whose slugs differ only in case: distinct logical routes
    /// that alias on case-insensitive filesystems.
    fn case_alias_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Alias\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/up.md",
                    "---\ntitle: Upper\nslug: Foo\n---\n\nUpper body.\n",
                ),
                (
                    "content/posts/lo.md",
                    "---\ntitle: Lower\nslug: foo\n---\n\nLower body.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    /// Two sources whose slugs differ only in Unicode normalization (NFC vs
    /// NFD): distinct logical routes that alias on normalizing filesystems.
    fn unicode_alias_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Alias\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/up.md",
                    "---\ntitle: Upper\nslug: \"caf\u{e9}\"\n---\n\nUpper body.\n",
                ),
                (
                    "content/posts/lo.md",
                    "---\ntitle: Lower\nslug: \"cafe\u{301}\"\n---\n\nLower body.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
    }

    /// No probe directory may survive a build, successful or not.
    fn assert_no_probe_directories(out: &Path) {
        if !out.exists() {
            return;
        }
        let mut stack = vec![out.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).expect("read out") {
                let path = entry.expect("out entry").path();
                if path.is_dir() {
                    assert!(
                        !path.file_name().is_some_and(|name| name
                            .to_string_lossy()
                            .starts_with(".signal-alias-probe-")),
                        "probe directory must not survive validation: {}",
                        path.display()
                    );
                    stack.push(path);
                }
            }
        }
    }

    /// A rejected plan publishes nothing: no artifact files, no manifest,
    /// and no probe residue. Empty directories are allowed (validation may
    /// ensure the output root exists before refusing the plan).
    fn assert_no_published_output(out: &Path) {
        assert!(
            !out.join(".signal/manifest.json").exists(),
            "a rejected plan must not write a manifest"
        );
        assert_no_probe_directories(out);
        if !out.exists() {
            return;
        }
        let mut files = Vec::new();
        let mut stack = vec![out.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current).expect("read out") {
                let path = entry.expect("out entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        assert!(files.is_empty(), "no artifact may be published: {files:?}");
    }

    #[test]
    fn current_case_alias_is_rejected_before_any_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        case_alias_site(&site);
        let out = dir.path().join("out");
        let result = build_site_from_disk(&site, &out);
        if filesystem_is_case_insensitive(dir.path()) {
            let err = result.expect_err("case-aliased current artifacts must fail");
            match err {
                BuildError::OutputCollision { first, second } => {
                    assert_eq!(first, "posts/Foo/index.html");
                    assert_eq!(second, "posts/foo/index.html");
                }
                other => panic!("expected OutputCollision, got: {other:?}"),
            }
            assert_no_published_output(&out);
        } else {
            // A filesystem that represents both names builds both pages.
            result.expect("case-sensitive filesystems represent both paths");
            assert!(fs::read_to_string(out.join("posts/Foo/index.html"))
                .expect("upper")
                .contains("Upper body."));
            assert!(fs::read_to_string(out.join("posts/foo/index.html"))
                .expect("lower")
                .contains("Lower body."));
            assert_no_probe_directories(&out);
        }
    }

    #[test]
    fn current_unicode_alias_is_rejected_before_any_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        unicode_alias_site(&site);
        let out = dir.path().join("out");
        let result = build_site_from_disk(&site, &out);
        if filesystem_normalizes_unicode(dir.path()) {
            let err = result.expect_err("normalization-aliased artifacts must fail");
            match err {
                BuildError::OutputCollision { first, second } => {
                    // Sorted order: NFD (`e` + combining acute) before NFC.
                    assert_eq!(first, "posts/cafe\u{301}/index.html");
                    assert_eq!(second, "posts/caf\u{e9}/index.html");
                }
                other => panic!("expected OutputCollision, got: {other:?}"),
            }
            assert_no_published_output(&out);
        } else {
            result.expect("non-normalizing filesystems represent both paths");
            assert!(out.join("posts/caf\u{e9}/index.html").exists());
            assert!(out.join("posts/cafe\u{301}/index.html").exists());
            assert_no_probe_directories(&out);
        }
    }

    #[test]
    fn multiple_current_aliases_report_the_first_sorted_pair() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        write_site(
            &site,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Alias\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/a.md",
                    "---\ntitle: A\nslug: FOO\n---\n\nA.\n",
                ),
                (
                    "content/posts/b.md",
                    "---\ntitle: B\nslug: Foo\n---\n\nB.\n",
                ),
                (
                    "content/posts/c.md",
                    "---\ntitle: C\nslug: foo\n---\n\nC.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        let result = build_site_from_disk(&site, &out);
        if filesystem_is_case_insensitive(dir.path()) {
            // Sorted plan order meets `FOO` first, then `Foo`: that pair is
            // reported deterministically, regardless of how many collide.
            let err = result.expect_err("aliased current artifacts must fail");
            match err {
                BuildError::OutputCollision { first, second } => {
                    assert_eq!(first, "posts/FOO/index.html");
                    assert_eq!(second, "posts/Foo/index.html");
                }
                other => panic!("expected OutputCollision, got: {other:?}"),
            }
            assert_no_published_output(&out);
        } else {
            result.expect("case-sensitive filesystems represent all three paths");
            assert!(out.join("posts/FOO/index.html").exists());
            assert!(out.join("posts/Foo/index.html").exists());
            assert!(out.join("posts/foo/index.html").exists());
        }
    }

    #[test]
    fn static_generated_case_alias_is_rejected_before_any_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        // The search index always plans `index.json`; a static file that
        // differs only in case must collide, like any other artifact kind.
        write_site(dir.path(), &[("static/Index.json", "{\"static\":true}\n")]);
        let out = dir.path().join("out");
        let result = build_site_from_disk(dir.path(), &out);
        if filesystem_is_case_insensitive(dir.path()) {
            let err = result.expect_err("static/generated alias must fail");
            match err {
                BuildError::OutputCollision { first, second } => {
                    assert_eq!(first, "Index.json");
                    assert_eq!(second, "index.json");
                }
                other => panic!("expected OutputCollision, got: {other:?}"),
            }
            assert_no_published_output(&out);
        } else {
            result.expect("case-sensitive filesystems represent both paths");
            assert_eq!(
                fs::read(out.join("Index.json")).expect("static"),
                b"{\"static\":true}\n"
            );
            assert!(fs::read_to_string(out.join("index.json"))
                .expect("search")
                .contains("\"version\""));
            assert_no_probe_directories(&out);
        }
    }

    #[test]
    fn incremental_case_alias_fails_without_touching_previous_build() {
        let dir = tempfile::tempdir().expect("tempdir");
        let site = dir.path().join("site");
        single_post_site(&site, "Foo");
        let out = dir.path().join("out");
        build_site_from_disk(&site, &out).expect("first builds");
        let manifest_before = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let page_before = fs::read(out.join("posts/Foo/index.html")).expect("page");

        // A second source aliases the first on case-insensitive filesystems.
        // Validation runs before any reuse decision, so the otherwise
        // reusable `Foo` artifact cannot bypass the collision check.
        write_site(
            &site,
            &[(
                "content/posts/lo.md",
                "---\ntitle: Lower\nslug: foo\n---\n\nLower body.\n",
            )],
        );
        let result = build_site_from_disk(&site, &out);
        if filesystem_is_case_insensitive(dir.path()) {
            let err = result.expect_err("adding a case-aliased artifact must fail");
            assert!(
                matches!(err, BuildError::OutputCollision { .. }),
                "got: {err:?}"
            );
            assert_eq!(
                fs::read(out.join(".signal/manifest.json")).expect("manifest"),
                manifest_before,
                "the previous manifest must survive a rejected plan"
            );
            assert_eq!(
                fs::read(out.join("posts/Foo/index.html")).expect("page"),
                page_before,
                "the current output must not be overwritten"
            );
            assert!(
                !out.join(".signal/manifest.json.tmp").exists(),
                "no partial manifest may be left behind"
            );
        } else {
            let summary = result.expect("case-sensitive filesystems represent both paths");
            assert!(
                summary.pruned_paths.is_empty(),
                "nothing is stale: {:?}",
                summary.pruned_paths
            );
            assert!(out.join("posts/Foo/index.html").exists());
            assert!(out.join("posts/foo/index.html").exists());
        }
    }

    #[test]
    fn file_blocking_planned_subdirectory_is_a_collision() {
        // `a` (file) and `a/b.txt` (file) cannot coexist on any filesystem.
        // Unreachable through source enumeration (the source tree itself
        // could not hold both), but the plan validator still rejects the
        // combination deterministically on every platform, before any write.
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = BuildPlan::new(
            dir.path().join("site"),
            dir.path().join("out"),
            vec![
                ArtifactSpec::new("a", ArtifactKind::Static),
                ArtifactSpec::new("a/b.txt", ArtifactKind::Static),
            ],
        );
        let err = plan
            .validate_output_paths()
            .expect_err("file blocking a planned subdirectory must collide");
        match err {
            BuildError::OutputCollision { first, second } => {
                assert_eq!(first, "a");
                assert_eq!(second, "a/b.txt");
            }
            other => panic!("expected OutputCollision, got: {other:?}"),
        }
        assert_no_published_output(&dir.path().join("out"));
    }

    #[test]
    fn folded_file_blocking_planned_subdirectory_is_a_collision() {
        // `A` (file) and `a/b.txt` (file): on a case-insensitive filesystem
        // the folded name blocks the subdirectory exactly like an exact one.
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = BuildPlan::new(
            dir.path().join("site"),
            dir.path().join("out"),
            vec![
                ArtifactSpec::new("A", ArtifactKind::Static),
                ArtifactSpec::new("a/b.txt", ArtifactKind::Static),
            ],
        );
        let result = plan.validate_output_paths();
        if filesystem_is_case_insensitive(dir.path()) {
            let err = result.expect_err("folded blocker must collide");
            match err {
                BuildError::OutputCollision { first, second } => {
                    assert_eq!(first, "A");
                    assert_eq!(second, "a/b.txt");
                }
                other => panic!("expected OutputCollision, got: {other:?}"),
            }
            assert_no_published_output(&dir.path().join("out"));
        } else {
            result.expect("case-sensitive filesystems represent both paths");
        }
    }

    #[test]
    fn pre_existing_hardlinks_with_distinct_names_are_not_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        write_site(dir.path(), &[("static/a.txt", "linked-bytes\n")]);
        // Same inode, distinct names: representable everywhere. Validation
        // replicates planned *names* in a fresh probe directory, never
        // output-tree inodes, so this must build.
        fs::hard_link(
            dir.path().join("static/a.txt"),
            dir.path().join("static/b.txt"),
        )
        .expect("hard link");
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("distinct names build");
        assert_eq!(fs::read(out.join("a.txt")).expect("a"), b"linked-bytes\n");
        assert_eq!(fs::read(out.join("b.txt")).expect("b"), b"linked-bytes\n");
    }

    #[test]
    fn successful_builds_leave_no_probe_residue() {
        let dir = tempfile::tempdir().expect("tempdir");
        minimal_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        assert_no_probe_directories(&out);
    }

    // --- Slice 15A: fail-closed source/template discovery (R14-1) ---

    /// Whether this process can actually enumerate `dir`, so a
    /// permission-based test skips where the OS does not enforce it (e.g.
    /// root in CI, where `chmod 000` is bypassed).
    #[cfg(unix)]
    fn directory_is_readable(dir: &Path) -> bool {
        match fs::read_dir(dir) {
            Ok(mut entries) => entries.all(|entry| entry.is_ok()),
            Err(_) => false,
        }
    }

    /// Snapshot the manifest bytes, a published page, and the absence of a
    /// partial manifest, then assert all three survived a failed rebuild.
    #[cfg(unix)]
    fn assert_failed_build_left_previous_build_intact(
        out: &Path,
        manifest_bytes: &[u8],
        page_bytes: &[u8],
    ) {
        assert_eq!(
            fs::read(out.join(".signal/manifest.json")).expect("manifest"),
            manifest_bytes,
            "a discovery failure must not replace the previous manifest"
        );
        assert_eq!(
            fs::read(out.join("posts/b/index.html")).expect("page"),
            page_bytes,
            "a discovery failure must not prune or alter published output"
        );
        assert!(
            !out.join(".signal/manifest.json.tmp").exists(),
            "a discovery failure must not leave a partial manifest"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_content_directory_fails_closed_without_pruning() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Disc\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
                ("content/posts/hidden/b.md", "---\ntitle: B\n---\n\nB body.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("posts/b/index.html").exists());
        let manifest_bytes = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let page_bytes = fs::read(out.join("posts/b/index.html")).expect("page");

        // Model a transient source read failure (permissions, NFS, a partial
        // checkout) on a subdirectory holding a published page.
        let hidden = dir.path().join("content/posts/hidden");
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o000)).expect("chmod");
        if directory_is_readable(&hidden) {
            fs::set_permissions(&hidden, fs::Permissions::from_mode(0o755)).expect("restore");
            eprintln!("skipping: directory permissions are not enforced for this user");
            return;
        }

        let result = build_site_from_disk(dir.path(), &out);
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o755)).expect("restore");

        let err = result.expect_err("an unreadable source directory must fail the build");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("hidden"),
            "the error must name the offending path: {err}"
        );
        assert_failed_build_left_previous_build_intact(&out, &manifest_bytes, &page_bytes);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_template_directory_fails_closed_without_pruning() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Disc\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
                ("content/posts/hidden/b.md", "---\ntitle: B\n---\n\nB body.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                ("templates/partials/unused.html", "<em>partial</em>"),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        let manifest_bytes = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let page_bytes = fs::read(out.join("posts/b/index.html")).expect("page");

        // A partial template walk must never look like a legitimately smaller
        // TemplateSet: unreadable means fatal.
        let partials = dir.path().join("templates/partials");
        fs::set_permissions(&partials, fs::Permissions::from_mode(0o000)).expect("chmod");
        if directory_is_readable(&partials) {
            fs::set_permissions(&partials, fs::Permissions::from_mode(0o755)).expect("restore");
            eprintln!("skipping: directory permissions are not enforced for this user");
            return;
        }

        let result = build_site_from_disk(dir.path(), &out);
        fs::set_permissions(&partials, fs::Permissions::from_mode(0o755)).expect("restore");

        let err = result.expect_err("an unreadable template directory must fail the build");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("partials"),
            "the error must name the offending path: {err}"
        );
        assert_failed_build_left_previous_build_intact(&out, &manifest_bytes, &page_bytes);
    }

    #[test]
    fn missing_template_root_is_an_empty_set_not_a_failure() {
        // A site may legitimately ship no templates directory; discovery must
        // keep treating that as empty so rendering later fails with an
        // explicit unknown-template error rather than a discovery error.
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(load_templates(dir.path()).is_ok());
    }

    // --- Slice 15E: metadata fail-closed + URL consistency (15D-1..15D-4) ---

    /// Whether this process can stat `path`, so a metadata-failure test can
    /// skip where the OS does not enforce permissions (e.g. root in CI,
    /// where `chmod 000` on a parent is bypassed and traversal still
    /// succeeds).
    #[cfg(unix)]
    fn path_is_statable(path: &Path) -> bool {
        std::fs::metadata(path).is_ok()
    }

    /// Assert a failed rebuild preserved the previous manifest bytes, the
    /// previously published page bytes at `page`, and left no partial
    /// manifest behind.
    #[cfg(unix)]
    fn assert_failed_rebuild_preserved(
        out: &Path,
        page: &str,
        manifest_bytes: &[u8],
        page_bytes: &[u8],
    ) {
        assert_eq!(
            fs::read(out.join(".signal/manifest.json")).expect("manifest"),
            manifest_bytes,
            "a discovery failure must not replace the previous manifest"
        );
        assert_eq!(
            fs::read(out.join(page)).expect("page"),
            page_bytes,
            "a discovery failure must not prune or alter published output"
        );
        assert!(
            !out.join(".signal/manifest.json.tmp").exists(),
            "a discovery failure must not leave a partial manifest"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unresolvable_content_symlink_fails_closed_without_pruning() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Disc\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        // Symlinked content is followed (explicit policy, R14-4 deferred):
        // the baseline publishes through the link.
        fs::create_dir_all(dir.path().join("ext/secret")).unwrap();
        fs::write(
            dir.path().join("ext/secret/s.md"),
            "---\ntitle: S\n---\n\nS body.\n",
        )
        .unwrap();
        symlink(
            dir.path().join("ext/secret"),
            dir.path().join("content/posts/seclink"),
        )
        .expect("symlink");
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("posts/s/index.html").exists());
        let manifest_bytes = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let page_bytes = fs::read(out.join("posts/s/index.html")).expect("page");

        // Break metadata resolution through the link by revoking search on
        // the target's parent: `stat` now fails, which `is_dir()` would
        // have swallowed as "not a directory". That silent skip is the
        // 15D-1 hole; classification must fail closed instead.
        let ext = dir.path().join("ext");
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o000)).expect("chmod");
        if path_is_statable(&dir.path().join("content/posts/seclink")) {
            fs::set_permissions(&ext, fs::Permissions::from_mode(0o755)).expect("restore");
            eprintln!("skipping: filesystem permissions are not enforced for this user");
            return;
        }

        let result = build_site_from_disk(dir.path(), &out);
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o755)).expect("restore");

        let err = result.expect_err("an unresolvable source entry must fail the build");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("seclink"),
            "the error must name the offending path: {err}"
        );
        assert_failed_rebuild_preserved(&out, "posts/s/index.html", &manifest_bytes, &page_bytes);

        // Recovery: restoring the source and rebuilding must reproduce a
        // clean build exactly (same pages, same manifest bytes).
        build_site_from_disk(dir.path(), &out).expect("recovery builds");
        let clean = dir.path().join("clean");
        build_site_from_disk(dir.path(), &clean).expect("clean builds");
        assert_eq!(
            fs::read(out.join("posts/s/index.html")).expect("page"),
            fs::read(clean.join("posts/s/index.html")).expect("clean page"),
        );
        assert_eq!(
            fs::read(out.join(".signal/manifest.json")).expect("manifest"),
            fs::read(clean.join(".signal/manifest.json")).expect("clean manifest"),
        );
    }

    #[cfg(unix)]
    #[test]
    fn unresolvable_template_symlink_fails_closed_without_pruning() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Disc\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        // Template symlinks are followed like content symlinks; the linked
        // partial joins the TemplateSet in the baseline build.
        fs::create_dir_all(dir.path().join("ext/partials")).unwrap();
        fs::write(dir.path().join("ext/partials/p.html"), "<em>p</em>").unwrap();
        symlink(
            dir.path().join("ext/partials"),
            dir.path().join("templates/partials-link"),
        )
        .expect("symlink");
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("posts/a/index.html").exists());
        let manifest_bytes = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let page_bytes = fs::read(out.join("posts/a/index.html")).expect("page");

        let ext = dir.path().join("ext");
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o000)).expect("chmod");
        if path_is_statable(&dir.path().join("templates/partials-link")) {
            fs::set_permissions(&ext, fs::Permissions::from_mode(0o755)).expect("restore");
            eprintln!("skipping: filesystem permissions are not enforced for this user");
            return;
        }

        let result = build_site_from_disk(dir.path(), &out);
        fs::set_permissions(&ext, fs::Permissions::from_mode(0o755)).expect("restore");

        let err = result.expect_err("an unresolvable template entry must fail the build");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("partials-link"),
            "the error must name the offending path: {err}"
        );
        assert_failed_rebuild_preserved(&out, "posts/a/index.html", &manifest_bytes, &page_bytes);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_static_directory_fails_closed_without_pruning() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Disc\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                ("static/asset.txt", "static-bytes\n"),
                ("static/sub/more.txt", "more-bytes\n"),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("first builds");
        assert!(out.join("static/sub/more.txt").exists() || out.join("sub/more.txt").exists());
        let manifest_bytes = fs::read(out.join(".signal/manifest.json")).expect("manifest");
        let asset_bytes = fs::read(out.join("asset.txt")).expect("asset");

        // Static discovery is fail-closed like content/template discovery:
        // an unreadable subtree aborts instead of reading as deletion.
        // (Single-entry iteration errors share this path via the same
        // helper; they cannot be triggered deterministically without a
        // mid-read race, so the subtree case pins the behavior.)
        let sub = dir.path().join("static/sub");
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o000)).expect("chmod");
        if directory_is_readable(&sub) {
            fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).expect("restore");
            eprintln!("skipping: directory permissions are not enforced for this user");
            return;
        }

        let result = build_site_from_disk(dir.path(), &out);
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).expect("restore");

        let err = result.expect_err("an unreadable static directory must fail the build");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("sub"),
            "the error must name the offending path: {err}"
        );
        assert_failed_rebuild_preserved(&out, "asset.txt", &manifest_bytes, &asset_bytes);
        assert!(
            out.join("sub/more.txt").exists(),
            "the static output must not be pruned"
        );
    }

    #[test]
    fn front_matter_image_urls_encode_consistently() {
        // One logical image, one URL representation: HTML `src`, `og:image`,
        // and JSON-LD `image` must agree, with no fragment/query introduced
        // and no double encoding. Ordinary paths stay byte-identical.
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Img\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/img.md",
                    "---\ntitle: Img\nimage: /images/a#b.png\n---\n\nBody.\n",
                ),
                (
                    "content/posts/qry.md",
                    "---\ntitle: Qry\nimage: /images/a?x=1.png\n---\n\nBody.\n",
                ),
                (
                    "content/posts/plain.md",
                    "---\ntitle: Plain\nimage: /images/plain.png\n---\n\nBody.\n",
                ),
                (
                    "templates/post.html",
                    "<html><head><title>{{ title }}</title><meta property=\"og:image\" content=\"{{ og_image }}\"><script type=\"application/ld+json\">{{ json_ld | safe }}</script></head><body><img src=\"{{ image }}\" alt=\"x\"></body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                ("static/images/a#b.png", "hash-bytes\n"),
                ("static/images/a?x=1.png", "query-bytes\n"),
                ("static/images/plain.png", "plain-bytes\n"),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");

        let hash = fs::read_to_string(out.join("posts/img/index.html")).expect("page");
        assert!(
            hash.contains("src=\"&#x2f;images&#x2f;a%23b.png\""),
            "HTML src must encode `#`: {hash}"
        );
        assert!(
            hash.contains("content=\"https:&#x2f;&#x2f;example.com&#x2f;images&#x2f;a%23b.png\""),
            "og:image must match HTML src: {hash}"
        );
        assert!(
            hash.contains("\"image\":\"https://example.com/images/a%23b.png\""),
            "JSON-LD image must match HTML src: {hash}"
        );
        assert!(!hash.contains("a#b.png"), "raw `#` leaked: {hash}");
        assert!(!hash.contains("%2523"), "double encoding: {hash}");

        let query = fs::read_to_string(out.join("posts/qry/index.html")).expect("page");
        assert!(
            query.contains("src=\"&#x2f;images&#x2f;a%3Fx%3D1.png\""),
            "HTML src must encode `?`: {query}"
        );
        assert!(
            query.contains("\"image\":\"https://example.com/images/a%3Fx%3D1.png\""),
            "JSON-LD image must match HTML src: {query}"
        );
        assert!(!query.contains("a?x=1.png"), "raw `?` leaked: {query}");

        let plain = fs::read_to_string(out.join("posts/plain/index.html")).expect("page");
        assert!(
            plain.contains("src=\"&#x2f;images&#x2f;plain.png\""),
            "ordinary image must stay byte-identical: {plain}"
        );
        assert!(
            plain.contains("\"image\":\"https://example.com/images/plain.png\""),
            "ordinary og:image must stay byte-identical: {plain}"
        );
    }

    // --- Slice 15C: route-to-URL path encoding (R14-3) ---

    /// Site with URL-significant slugs: fragment-, query-, percent-,
    /// quote-, and Unicode-bearing routes. Filenames stay plain where the
    /// slug would complicate fixture creation; the slug is the route source
    /// either way.
    fn url_probe_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"URL\"\nbase_url = \"https://example.com/\"\n[taxonomy]\nroute_prefix = \"/topics/\"\n[feed]\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/issue#12.md",
                    "---\ntitle: Hash\ntags: [\"Rust\"]\n---\n\nBody.\n",
                ),
                (
                    "content/posts/100%-done.md",
                    "---\ntitle: Pct\n---\n\nBody.\n",
                ),
                (
                    "content/posts/q.md",
                    "---\ntitle: Query\nslug: \"a?x=1\"\n---\n\nBody.\n",
                ),
                (
                    "content/posts/uni.md",
                    "---\ntitle: Uni\nslug: \"caf\u{e9}\"\n---\n\nBody.\n",
                ),
                (
                    "content/posts/quote.md",
                    "---\ntitle: 'Q <b>& \"q\"'\nslug: 'a\"b'\n---\n\nBody.\n",
                ),
                (
                    "templates/post.html",
                    "<html><head><title>{{ title }}</title><link rel=\"canonical\" href=\"{{ canonical_url }}\"><meta property=\"og:url\" content=\"{{ og_url }}\"><script type=\"application/ld+json\">{{ json_ld | safe }}</script></head><body data-route=\"{{ route }}\">{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>{% for e in entries %}<a href=\"{{ e.route }}\">{{ e.title }}</a>{% endfor %}</body></html>",
                ),
                (
                    "templates/topics.html",
                    "<html><body><ul>{% for t in topics %}<li><a href=\"{{ t.route }}\">{{ t.label }}</a></li>{% endfor %}</ul></body></html>",
                ),
                (
                    "templates/topic.html",
                    "<html><body><ul>{% for e in entries %}<li><a href=\"{{ e.route }}\">{{ e.title }}</a></li>{% endfor %}</ul></body></html>",
                ),
            ],
        );
    }

    #[test]
    fn encoded_routes_appear_in_html_listings_and_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        url_probe_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");

        // Filesystem output keeps the logical route verbatim: this slice
        // concerns URL serialization, not file naming.
        for file in [
            "posts/issue#12/index.html",
            "posts/100%-done/index.html",
            "posts/a?x=1/index.html",
            "posts/caf\u{e9}/index.html",
            "posts/a\"b/index.html",
        ] {
            assert!(out.join(file).exists(), "output file {file:?} missing");
        }

        // Section listing links carry the encoded route (`/` surfaces as
        // `&#x2f;` through MiniJinja HTML escaping; browsers decode it).
        let section = fs::read_to_string(out.join("posts/index.html")).expect("section");
        for encoded in [
            "&#x2f;posts&#x2f;issue%2312&#x2f;",
            "&#x2f;posts&#x2f;100%25-done&#x2f;",
            "&#x2f;posts&#x2f;a%3Fx%3D1&#x2f;",
            "&#x2f;posts&#x2f;caf%C3%A9&#x2f;",
            "&#x2f;posts&#x2f;a%22b&#x2f;",
        ] {
            assert!(
                section.contains(encoded),
                "listing link {encoded:?} missing: {section}"
            );
        }
        // The raw logical routes never leak into listing markup.
        for raw in ["issue#12", "100%-done", "a?x=1"] {
            assert!(
                !section.contains(raw),
                "raw route {raw:?} leaked into listing: {section}"
            );
        }

        // Term listing and taxonomy index use the same boundary.
        let term = fs::read_to_string(out.join("topics/rust/index.html")).expect("term");
        assert!(
            term.contains("&#x2f;posts&#x2f;issue%2312&#x2f;"),
            "term listing link missing: {term}"
        );
        let topics = fs::read_to_string(out.join("topics/index.html")).expect("topics");
        assert!(
            topics.contains("&#x2f;topics&#x2f;rust&#x2f;"),
            "taxonomy index link missing: {topics}"
        );

        // Entry page: canonical, OpenGraph, JSON-LD, and the `route` context
        // value all carry the encoded path — with no fragment or query.
        let page = fs::read_to_string(out.join("posts/issue#12/index.html")).expect("page");
        let canonical = "https:&#x2f;&#x2f;example.com&#x2f;posts&#x2f;issue%2312&#x2f;";
        assert!(
            page.contains(&format!("<link rel=\"canonical\" href=\"{canonical}\">")),
            "canonical missing: {page}"
        );
        assert!(
            page.contains(&format!(
                "<meta property=\"og:url\" content=\"{canonical}\">"
            )),
            "og:url missing: {page}"
        );
        assert!(
            page.contains("\"url\":\"https://example.com/posts/issue%2312/\""),
            "JSON-LD url missing: {page}"
        );
        assert!(
            page.contains("data-route=\"&#x2f;posts&#x2f;issue%2312&#x2f;\""),
            "route context value missing: {page}"
        );
        assert!(
            !page.contains("issue#12"),
            "raw route leaked into entry page: {page}"
        );

        let query_page = fs::read_to_string(out.join("posts/a?x=1/index.html")).expect("page");
        assert!(
            query_page.contains("a%3Fx%3D1"),
            "query-like route not encoded: {query_page}"
        );
        assert!(
            !query_page.contains("a?x=1"),
            "raw route leaked into entry page: {query_page}"
        );

        // HTML/JSON escaping still applies on top of URL encoding: the
        // quote-bearing slug reaches attributes only as `%22`, never as a
        // break-out `"`, while element text keeps its HTML escaping and
        // JSON-LD keeps its JSON escaping.
        let quote_page = fs::read_to_string(out.join("posts/a\"b/index.html")).expect("page");
        assert!(
            quote_page.contains("&#x2f;posts&#x2f;a%22b&#x2f;"),
            "quote slug not encoded: {quote_page}"
        );
        assert!(
            !quote_page.contains("a\"b"),
            "raw quote leaked into entry page: {quote_page}"
        );
        assert!(
            quote_page.contains("<title>Q &lt;b&gt;&amp; &quot;q&quot;</title>"),
            "title HTML escaping lost: {quote_page}"
        );
        assert!(
            quote_page.contains("\"headline\":\"Q <b>& \\\"q\\\"\""),
            "JSON-LD JSON escaping lost (`<` is inert script text, `\"` must stay escaped): {quote_page}"
        );
    }

    #[test]
    fn encoded_routes_appear_in_feeds_sitemap_and_search() {
        let dir = tempfile::tempdir().expect("tempdir");
        url_probe_site(dir.path());
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");

        // RSS item links and GUIDs (main feed and term feed).
        let feed = fs::read_to_string(out.join("index.xml")).expect("feed");
        for encoded in [
            "https://example.com/posts/issue%2312/",
            "https://example.com/posts/a%3Fx%3D1/",
            "https://example.com/posts/100%25-done/",
            "https://example.com/posts/caf%C3%A9/",
        ] {
            assert!(
                feed.contains(&format!("<link>{encoded}</link>")),
                "feed link {encoded:?} missing"
            );
            assert!(
                feed.contains(&format!("<guid>{encoded}</guid>")),
                "feed guid {encoded:?} missing"
            );
        }
        assert!(!feed.contains("issue#12"), "raw route leaked into feed");
        let term_feed = fs::read_to_string(out.join("topics/rust/index.xml")).expect("term feed");
        assert!(
            term_feed.contains("<link>https://example.com/posts/issue%2312/</link>"),
            "term feed link missing"
        );

        // Sitemap locations.
        let sitemap = fs::read_to_string(out.join("sitemap.xml")).expect("sitemap");
        assert!(
            sitemap.contains("<loc>https://example.com/posts/issue%2312/</loc>"),
            "sitemap loc missing"
        );
        assert!(
            sitemap.contains("<loc>https://example.com/posts/a%3Fx%3D1/</loc>"),
            "sitemap loc missing"
        );
        assert!(
            !sitemap.contains("issue#12"),
            "raw route leaked into sitemap"
        );

        // Search index document id and url.
        let index = fs::read_to_string(out.join("index.json")).expect("search");
        assert!(
            index.contains("\"id\": \"/posts/issue%2312/\""),
            "search id missing: {index}"
        );
        assert!(
            index.contains("\"url\": \"/posts/issue%2312/\""),
            "search url missing: {index}"
        );
        assert!(
            !index.contains("\"url\": \"/posts/issue#12/\""),
            "raw route leaked into search index"
        );
    }

    fn read_manifest(out: &Path) -> crate::manifest::Manifest {
        crate::manifest::parse_manifest(
            &fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
        )
        .expect("manifest parses")
    }

    fn copy_fixture_tree(source: &Path, dest: &Path) {
        let mut stack = vec![source.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current)
                .expect("read dir")
                .filter_map(Result::ok)
            {
                let path = entry.path();
                let rel = path.strip_prefix(source).expect("relative");
                let target = dest.join(rel);
                if path.is_dir() {
                    fs::create_dir_all(&target).expect("mkdir");
                    stack.push(path);
                } else {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).expect("mkdir");
                    }
                    fs::copy(&path, &target).expect("copy");
                }
            }
        }
    }

    #[test]
    fn markdown_unsafe_urls_never_reach_rendered_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
                ),
                (
                    "content/posts/x.md",
                    "---\ntitle: X\n---\n\n[js](javascript:alert(1)) [data](data:text/html,x) [vb](vbscript:x)\n\n![img](javascript:alert(2))\n\n[safe](/ok) and <javascript:alert(3)>\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
            ],
        );
        let out = dir.path().join("out");
        build_site_from_disk(dir.path(), &out).expect("builds");
        let html = fs::read_to_string(out.join("posts/x/index.html")).expect("output");
        let lower = html.to_ascii_lowercase();
        for bad in [
            "href=\"javascript:",
            "href=\"data:",
            "href=\"vbscript:",
            "src=\"javascript:",
            "src=\"data:",
            "src=\"vbscript:",
        ] {
            assert!(
                !lower.contains(bad),
                "unsafe {bad:?} in rendered page: {html}"
            );
        }
        assert!(
            html.contains("href=\"/ok\"") && html.contains("safe"),
            "safe link must survive: {html}"
        );
    }

    fn snapshot_public_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(current) = stack.pop() {
            let mut entries: Vec<PathBuf> = fs::read_dir(&current)
                .expect("read dir")
                .filter_map(Result::ok)
                .map(|e| e.path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name == ".signal") {
                        continue;
                    }
                    stack.push(path);
                } else if let Ok(bytes) = fs::read(&path) {
                    out.push((
                        path.strip_prefix(root).expect("relative").to_path_buf(),
                        bytes,
                    ));
                }
            }
        }
        out.sort();
        out
    }
}
