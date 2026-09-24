//! First-class asset model (A1): references, identity, and media types.
//!
//! An asset is build input that is not a page: an image, stylesheet,
//! script, or any other static file a page references. This module owns the
//! pure vocabulary shared by every crate:
//!
//! ```text
//! source asset     a file under `static/` (e.g. `static/images/hero.jpg`)
//! asset reference  an authored string in content (`body.images` or
//!                  front-matter `image`) that names a source asset
//! output asset     the planned `ArtifactKind::Static` spec plus its bytes,
//!                  copied verbatim ([`output_path_for_source`])
//! ```
//!
//! Generated artifacts are *not* assets in this sense and have their own
//! kinds and identities: image derivatives are [`DerivativeSpec`] triples
//! (A2, ADR 0029) resolved by `signal-cli::images`, and social images are
//! page projections (A5, ADR 0032) owned by [`crate::social`]. Both consume
//! this module's reference resolution and MIME tables rather than
//! duplicating them.
//!
//! All functions are pure, deterministic, and filesystem-free: reference
//! resolution works on strings, MIME detection on extensions. Byte size and
//! content hashes are measured at the filesystem boundary (`signal-cli`)
//! and never here, preserving the `signal-core` I/O ban.
//!
//! Reference semantics mirror `signal-cli::link_check` exactly (same
//! external classification, same document-relative resolution against the
//! containing entry's route, same traversal failure, same raw-then-decoded
//! matching): the planner and the validator resolve the same authored
//! string to the same static path. Front-matter images are literal paths
//! (`#`/`?` are path characters); Markdown image destinations split off
//! query/fragment before resolution.

use serde::{Deserialize, Serialize};

use crate::model::ContentEntry;

/// Output path for a source asset.
///
/// A1 is the identity mapping: `images/hero.jpg` in, `images/hero.jpg` out.
/// Content-addressed (fingerprinted) paths (`hero-<hash>.jpg`) fit here
/// without touching callers: change this function plus the planner's output
/// enumeration, and every consumer (manifest records, reference validation,
/// `explain`) follows. Generated families keep their own naming
/// ([`derivative_output_path`] for `(source, width, format)`,
/// `signal_core::social_image_path` for a page route) because their
/// identity is not a source path.
pub fn output_path_for_source(source_path: &str) -> String {
    source_path.to_string()
}

/// Guess the MIME type from the file extension.
///
/// Lowercases the extension and matches a fixed table of web-relevant types;
/// unknown or missing extensions map to `application/octet-stream`.
/// Deterministic and locale-free: pure byte/char operations.
pub fn mime_for_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default();
    let ext = ext.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut buf = [0u8; 16];
    let bytes = ext.as_bytes();
    if bytes.len() > buf.len() {
        return "application/octet-stream";
    }
    let mut len = 0;
    for &b in bytes {
        buf[len] = b.to_ascii_lowercase();
        len += 1;
    }
    match &buf[..len] {
        b"jpg" | b"jpeg" => "image/jpeg",
        b"png" => "image/png",
        b"gif" => "image/gif",
        b"webp" => "image/webp",
        b"avif" => "image/avif",
        b"svg" => "image/svg+xml",
        b"ico" => "image/x-icon",
        b"bmp" => "image/bmp",
        b"tif" | b"tiff" => "image/tiff",
        b"css" => "text/css",
        b"js" | b"mjs" => "text/javascript",
        b"json" => "application/json",
        b"xml" => "application/xml",
        b"html" | b"htm" => "text/html",
        b"txt" => "text/plain",
        b"md" | b"markdown" => "text/markdown",
        b"pdf" => "application/pdf",
        b"woff" => "font/woff",
        b"woff2" => "font/woff2",
        b"ttf" => "font/ttf",
        b"otf" => "font/otf",
        b"mp4" => "video/mp4",
        b"webm" => "video/webm",
        b"mp3" => "audio/mpeg",
        b"wav" => "audio/wav",
        b"ogg" => "audio/ogg",
        b"ogv" => "video/ogg",
        _ => "application/octet-stream",
    }
}

/// Default `sizes` value for responsive images (A3, ADR 0030): the image
/// is assumed to occupy the full viewport width. There is no layout DSL
/// and no per-image override in A3 — a documented default beats an
/// inferred one.
pub const DEFAULT_SIZES: &str = "100vw";

/// One generated derivative as rendering sees it (A3, ADR 0030;
/// format-aware since A4, ADR 0031): output identity plus requested and
/// actual dimensions.
///
/// Requested and actual widths differ exactly when the A2 no-upscale
/// clamp applied (e.g. a 1920w request against a 1600px source yields
/// `actual_width == 1600`). Rendering must always advertise actual
/// dimensions, never requested ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivativeView {
    /// Derivative output path relative to the output root.
    pub output: String,
    /// Requested width in pixels (the output filename label).
    pub width: u32,
    /// Output format (WebP, AVIF since A4).
    pub format: DerivativeFormat,
    /// Actual output width in pixels (clamped, never upscaled).
    pub actual_width: u32,
    /// Actual output height in pixels.
    pub actual_height: u32,
}

/// One `srcset` candidate: an encoded URL plus its intrinsic width.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsiveCandidate {
    /// Site-root URL in URL-path form (encoded, root-relative).
    pub url: String,
    /// Intrinsic width in pixels (actual, never requested).
    pub width: u32,
    /// Intrinsic height in pixels.
    pub height: u32,
    /// Output format this candidate is encoded in.
    pub format: DerivativeFormat,
}

/// One output format's responsive candidates (A4, ADR 0031): the unit a
/// `<source type=…>` element renders from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsiveSource {
    /// Output format of this group.
    pub format: DerivativeFormat,
    /// MIME type of this group (`image/avif`, `image/webp`).
    pub mime: String,
    /// Prebuilt `srcset` attribute for this format (`"<url> <w>w, …"`).
    pub srcset: String,
    /// Candidates of this format, actual-width ascending, widths unique.
    pub candidates: Vec<ResponsiveCandidate>,
}

/// A source image's responsive representation (A3, ADR 0030; multi-format
/// since A4, ADR 0031): everything rendering needs, decided once from
/// planned views. URLs are presentation-ready (encoded, root-relative);
/// dimensions are actual.
///
/// The flat `candidates`/`srcset`/`default_src` fields always describe
/// the **fallback** representation — the WebP group when one is planned,
/// else the sole planned group — so single-format sites render exactly
/// the A3 `<img>`. `sources` carries every planned format group in
/// `<source>` order (AVIF before WebP); rendering emits `<picture>` only
/// when more than one group exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsiveImage {
    /// Source path relative to `static/`.
    pub source: String,
    /// Fallback `srcset` candidates, actual-width ascending, widths unique.
    pub candidates: Vec<ResponsiveCandidate>,
    /// Fallback `src`: the largest available fallback candidate.
    pub default_src: String,
    /// Intrinsic width of the fallback.
    pub width: u32,
    /// Intrinsic height of the fallback.
    pub height: u32,
    /// `sizes` value (`DEFAULT_SIZES` in A3; unchanged in A4).
    pub sizes: String,
    /// Prebuilt fallback `srcset` attribute (`"<url> <w>w, …"`).
    pub srcset: String,
    /// One group per planned format, in `<source>` order (AVIF first).
    pub sources: Vec<ResponsiveSource>,
}

impl ResponsiveImage {
    /// Whether rendering must emit `<picture>` (more than one format
    /// group) rather than the plain responsive `<img>`.
    pub fn has_picture(&self) -> bool {
        self.sources.len() > 1
    }
}

/// Select a source's responsive representation from planned derivative
/// views (A3, ADR 0030; multi-format since A4, ADR 0031).
///
/// Pure and total: views group by format (each group sorted by actual
/// width, equal actual widths deduped keeping the smallest requested
/// width — tightest label, deterministic), groups order AVIF-before-WebP
/// (browsers take the first supported `<source>`, so AVIF wins where
/// supported), and the fallback is the WebP group when planned, else the
/// sole group. Views wider than the source are discarded (defense in
/// depth — the A2 clamp makes them unrepresentable); no views, or none
/// within the source, yields `None`, and callers leave the original
/// markup untouched.
pub fn responsive_image(
    source: &str,
    source_dimensions: (u32, u32),
    views: &[DerivativeView],
) -> Option<ResponsiveImage> {
    let (source_width, _) = source_dimensions;
    let floor = source_width.max(1);
    let mut ordered: Vec<&DerivativeView> = views
        .iter()
        .filter(|view| view.actual_width >= 1 && view.actual_width <= floor)
        .collect();
    ordered.sort_by(|a, b| {
        (a.format.source_rank(), a.actual_width, a.width, &a.output).cmp(&(
            b.format.source_rank(),
            b.actual_width,
            b.width,
            &b.output,
        ))
    });
    let mut sources = Vec::new();
    for view in ordered {
        let group: &mut ResponsiveSource = match sources
            .iter_mut()
            .find(|group: &&mut ResponsiveSource| group.format == view.format)
        {
            Some(group) => group,
            None => {
                sources.push(ResponsiveSource {
                    format: view.format,
                    mime: view.format.mime().to_string(),
                    srcset: String::new(),
                    candidates: Vec::new(),
                });
                sources.last_mut().expect("just pushed")
            }
        };
        if group
            .candidates
            .iter()
            .any(|candidate: &ResponsiveCandidate| candidate.width == view.actual_width)
        {
            continue;
        }
        group.candidates.push(ResponsiveCandidate {
            url: crate::meta::image_src_url(&format!("/{}", view.output)),
            width: view.actual_width,
            height: view.actual_height,
            format: view.format,
        });
    }
    sources.retain(|group| !group.candidates.is_empty());
    for group in &mut sources {
        group.srcset = group
            .candidates
            .iter()
            .map(|candidate| format!("{} {}w", candidate.url, candidate.width))
            .collect::<Vec<_>>()
            .join(", ");
    }
    // Fallback prefers WebP (universal support); an AVIF-only plan falls
    // back to its sole group. Either way the fallback is a real generated
    // representation with intrinsic dimensions — never the original.
    let fallback = sources
        .iter()
        .find(|group| group.format == DerivativeFormat::WebP)
        .or(sources.first())?;
    let default = fallback.candidates.last()?.clone();
    Some(ResponsiveImage {
        source: source.to_string(),
        candidates: fallback.candidates.clone(),
        default_src: default.url.clone(),
        width: default.width,
        height: default.height,
        sizes: DEFAULT_SIZES.to_string(),
        srcset: fallback.srcset.clone(),
        sources,
    })
}

/// Derivative output format (A2, ADR 0029; AVIF added in A4, ADR 0031).
///
/// A closed enum, not a string: only implemented codecs are representable,
/// so an unimplemented format is a parse/validation error, never a
/// runtime surprise. New codecs arrive as new variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DerivativeFormat {
    /// AVIF (rav1e/ravif-encoded, fixed settings — see ADR 0031).
    Avif,
    /// Lossless WebP: the only A2 output format.
    WebP,
}

impl DerivativeFormat {
    /// Parse an `[images]` format value. Anything but `"avif"`/`"webp"`
    /// is rejected with a diagnostic naming the supported set.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "avif" => Ok(DerivativeFormat::Avif),
            "webp" => Ok(DerivativeFormat::WebP),
            other => Err(format!(
                "unsupported image format {other:?}: supported formats are \"avif\", \"webp\""
            )),
        }
    }

    /// `<source>`/group ordering rank: AVIF before WebP, so browsers that
    /// support AVIF take it (first supported `<source>` wins) while every
    /// other consumer sees the more widely supported WebP fallback first
    /// in spirit. Also the canonical plan order, so author-chosen config
    /// order never affects output.
    pub fn source_rank(self) -> u8 {
        match self {
            DerivativeFormat::Avif => 0,
            DerivativeFormat::WebP => 1,
        }
    }

    /// File extension for derivative outputs.
    pub fn extension(self) -> &'static str {
        match self {
            DerivativeFormat::Avif => "avif",
            DerivativeFormat::WebP => "webp",
        }
    }

    /// MIME type of derivative outputs (also the `<source type=…>` value).
    pub fn mime(self) -> &'static str {
        match self {
            DerivativeFormat::Avif => "image/avif",
            DerivativeFormat::WebP => "image/webp",
        }
    }
}

impl std::fmt::Display for DerivativeFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DerivativeFormat::Avif => write!(f, "avif"),
            DerivativeFormat::WebP => write!(f, "webp"),
        }
    }
}

/// Whether a static path is a supported derivative source (A2): a
/// content-referenced raster image with a `png`, `jpg`, `jpeg`, or `webp`
/// extension (case-insensitive).
///
/// Extension-based and pure, so planning needs no file bytes. Content is
/// verified at resolve/check time: a non-raster file wearing a raster
/// extension fails clearly there instead of silently producing output.
/// SVG, GIF, AVIF, and every other type are never rasterized — they stay
/// verbatim static outputs.
pub fn is_derivable_source(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let ext = file.rsplit('.').next().unwrap_or_default();
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
}

/// One requested image derivative (A2, ADR 0029): a source asset plus
/// deterministic transformation parameters.
///
/// Identity is the triple `(source, width, format)`: identical triples
/// always resolve to the same artifact, differing triples never share an
/// output path (cross-source collisions like `a.png` + `a.jpg` fail in
/// plan validation rather than silently overwriting).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DerivativeSpec {
    /// Source path relative to `static/`, e.g. `images/hero.jpg`.
    pub source: String,
    /// Requested output width in pixels (`>= 1`, enforced by `new`).
    pub width: u32,
    /// Output format (WebP in A2).
    pub format: DerivativeFormat,
}

impl DerivativeSpec {
    /// Create a derivative request. Width `0` is rejected: widths come
    /// from configuration, and a zero width is a config diagnostic, never
    /// a skipped derivative.
    pub fn new(
        source: impl Into<String>,
        width: u32,
        format: DerivativeFormat,
    ) -> Result<Self, String> {
        if width == 0 {
            return Err("derivative width must be at least 1".to_string());
        }
        Ok(Self {
            source: source.into(),
            width,
            format,
        })
    }

    /// Deterministic output path: `{stem}-{width}.{ext}` beside the
    /// source (`images/hero.jpg` + 640 + webp →
    /// `images/hero-640.webp`). The single naming function every caller
    /// (planning, resolution, `explain`) shares, so fingerprinting later
    /// extends one place. Deliberately not content-hashed in A2.
    pub fn output_path(&self) -> String {
        derivative_output_path(&self.source, self.width, self.format)
    }

    /// Output dimensions for a source of `source_width × source_height`:
    /// aspect-preserving scale to the requested width, clamped to never
    /// exceed the source (no upscaling — see ADR 0029). Pure integer
    /// math, so every platform computes identical dimensions.
    pub fn target_dimensions(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        clamped_dimensions(source_width, source_height, self.width)
    }
}

/// Deterministic derivative output path for a source, width, and format.
///
/// Split from [`DerivativeSpec::output_path`] so planners can name outputs
/// while enumerating (source × widths) without constructing specs first.
pub fn derivative_output_path(source: &str, width: u32, format: DerivativeFormat) -> String {
    let (dir, file) = match source.rfind('/') {
        Some(index) => (&source[..index], &source[index + 1..]),
        None => ("", source),
    };
    let stem = match file.rfind('.') {
        Some(index) => &file[..index],
        None => file,
    };
    let name = format!("{stem}-{width}.{}", format.extension());
    if dir.is_empty() {
        name
    } else {
        format!("{dir}/{name}")
    }
}

/// Aspect-preserving output dimensions for a requested width, clamped to
/// the source (never upscale). Rounding is half-up integer math; outputs
/// are at least 1px in each axis. `(0, _)` / `(_, 0)` sources are
/// degenerate input no decoder produces — they resolve to a 1px floor
/// rather than dividing by zero.
fn clamped_dimensions(source_width: u32, source_height: u32, width: u32) -> (u32, u32) {
    if source_width == 0 || source_height == 0 || width == 0 {
        return (1, 1);
    }
    if width >= source_width {
        return (source_width, source_height);
    }
    let height =
        ((source_height as u64 * width as u64) + (source_width as u64 / 2)) / source_width as u64;
    (width, height.max(1) as u32)
}

/// Whether an authored reference names an external (non-source) target:
/// protocol-relative (`//host/…`) or any `scheme:` destination. Mirrors
/// `link_check::classify`: external references are never source assets,
/// whatever their scheme.
pub fn is_external_reference(value: &str) -> bool {
    let value = value.trim();
    if value.starts_with("//") {
        return true;
    }
    scheme_of(value).is_some()
}

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
/// detection). Mirrors `link_check::percent_decode`.
pub fn percent_decode(value: &str) -> Option<String> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SplitTarget {
    path: String,
}

fn split_target(value: &str) -> SplitTarget {
    let before_fragment = match value.find('#') {
        Some(index) => &value[..index],
        None => value,
    };
    let path = match before_fragment.find('?') {
        Some(index) => &before_fragment[..index],
        None => before_fragment,
    };
    SplitTarget {
        path: path.to_string(),
    }
}

/// Resolve an internal path against a source route to a canonical route.
///
/// Direct mirror of `link_check::resolve_route`: document-relative targets
/// resolve against the source route's directory, `.` is ignored, `..` pops,
/// and popping above the site root (or a decoded-only `.`/`..` segment)
/// fails. Returns `None` for unresolvable (unsafe) references.
fn resolve_route(source_route: &str, target: &str) -> Option<String> {
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
            segments.pop()?;
            continue;
        }
        if let Some(decoded) = percent_decode(segment) {
            if decoded != segment && (decoded == "." || decoded == "..") {
                return None;
            }
        }
        segments.push(segment.to_string());
    }
    let mut route = String::from("/");
    route.push_str(&segments.join("/"));
    if target.ends_with('/') && route != "/" {
        route.push('/');
    }
    Some(route)
}

/// Resolve one Markdown image destination to its `static/`-relative path.
///
/// Returns `None` for external destinations, empty/self references, and
/// unresolvable (unsafe) references. Query strings and fragments are
/// stripped before resolution (they never address asset bytes); the
/// returned path keeps its authored encoding — filesystem matching tries
/// the raw form first, then the percent-decoded form (see `link_check`).
pub fn resolve_body_image(source_route: &str, raw: &str) -> Option<String> {
    let target = raw.trim();
    if target.is_empty() || is_external_reference(target) {
        return None;
    }
    let split = split_target(target);
    if split.path.is_empty() {
        return None;
    }
    let route = resolve_route(source_route, &split.path)?;
    Some(route.trim_start_matches('/').to_string())
}

/// Resolve one normalized front-matter image to its `static/`-relative path.
///
/// Front-matter images are literal paths (`#`/`?` are path characters, so
/// no fragment/query split applies). Returns `None` for external
/// destinations.
pub fn resolve_front_matter_image(image: &str) -> Option<String> {
    let target = image.trim();
    if target.is_empty() || is_external_reference(target) {
        return None;
    }
    Some(target.trim_start_matches('/').to_string())
}

/// Every source asset one entry references, sorted and deduplicated.
///
/// Union of the front-matter hero image and all Markdown body images that
/// resolve to internal paths. External destinations (`http(s)`, `//…`,
/// other schemes) and empty/self references contribute nothing: they are
/// not source assets. Unresolvable (escaping) references also contribute
/// nothing here — reference validation reports them as errors.
pub fn entry_asset_paths(entry: &ContentEntry) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    if let Some(image) = entry.image.as_deref() {
        if let Some(path) = resolve_front_matter_image(image) {
            if !path.is_empty() {
                out.insert(path);
            }
        }
    }
    for raw in &entry.body.images {
        if let Some(path) = resolve_body_image(&entry.route.0, raw) {
            if !path.is_empty() {
                out.insert(path);
            }
        }
    }
    out.into_iter().collect()
}

/// Reverse index: every referenced asset path to the routes that name it.
///
/// Deterministic (`BTreeMap`/`BTreeSet`); routes sorted. Entries without
/// asset references contribute nothing. This is the `post.md └── hero.jpg`
/// edge in queryable form; the build planner consumes the forward
/// direction (entry inputs), `explain` consumes this reverse direction.
pub fn asset_referrers(
    model: &crate::model::SiteModel,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut index: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for entry in model.entries() {
        for path in entry_asset_paths(entry) {
            index.entry(path).or_default().insert(entry.route.0.clone());
        }
    }
    index
        .into_iter()
        .map(|(path, routes)| (path, routes.into_iter().collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{CollectionId, ContentId, Route, Slug, SourceRef};

    fn entry(route: &str, image: Option<&str>, images: &[&str]) -> ContentEntry {
        let mut e = ContentEntry::new(
            ContentId(1),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "a.md"),
            Slug::new("a"),
            Route::new(route.to_string()),
            "Title",
        );
        e.image = image.map(str::to_string);
        e.body.images = images.iter().map(|s| s.to_string()).collect();
        e
    }

    #[test]
    fn mime_table_covers_common_types() {
        assert_eq!(mime_for_path("hero.jpg"), "image/jpeg");
        assert_eq!(mime_for_path("HERO.JPG"), "image/jpeg");
        assert_eq!(mime_for_path("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_for_path("a.png"), "image/png");
        assert_eq!(mime_for_path("a.svg"), "image/svg+xml");
        assert_eq!(mime_for_path("a.webp"), "image/webp");
        assert_eq!(mime_for_path("a.avif"), "image/avif");
        assert_eq!(mime_for_path("a.css"), "text/css");
        assert_eq!(mime_for_path("a.js"), "text/javascript");
        assert_eq!(mime_for_path("a.json"), "application/json");
        assert_eq!(mime_for_path("a.pdf"), "application/pdf");
        assert_eq!(mime_for_path("a.woff2"), "font/woff2");
        assert_eq!(mime_for_path("a.mp4"), "video/mp4");
        assert_eq!(mime_for_path("no-extension"), "application/octet-stream");
        assert_eq!(mime_for_path("unknown.zzz"), "application/octet-stream");
    }

    #[test]
    fn body_images_resolve_relative_to_source_route() {
        assert_eq!(
            resolve_body_image("/posts/alpha/", "/images/a.svg"),
            Some("images/a.svg".to_string())
        );
        // Document-relative against the route directory.
        assert_eq!(
            resolve_body_image("/posts/alpha/", "../images/a.svg"),
            Some("posts/images/a.svg".to_string())
        );
        // Query and fragment never address bytes.
        assert_eq!(
            resolve_body_image("/posts/alpha/", "/images/a.svg?w=1#frag"),
            Some("images/a.svg".to_string())
        );
        // External destinations are not source assets.
        for external in [
            "https://example.com/a.png",
            "//cdn.example.com/a.png",
            "mailto:a@b.c",
            "data:image/png;base64,AAA",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                resolve_body_image("/posts/alpha/", external),
                None,
                "{external}"
            );
        }
        // Escaping the root is unresolvable.
        assert_eq!(resolve_body_image("/posts/alpha/", "../../../x.png"), None);
        assert_eq!(resolve_body_image("/posts/alpha/", "%2e%2e/x.png"), None);
    }

    #[test]
    fn front_matter_images_are_literal_paths() {
        assert_eq!(
            resolve_front_matter_image("/images/a.svg"),
            Some("images/a.svg".to_string())
        );
        assert_eq!(
            resolve_front_matter_image("images/a.svg"),
            Some("images/a.svg".to_string())
        );
        // `#`/`?` stay literal for front-matter images.
        assert_eq!(
            resolve_front_matter_image("/images/a.svg#frag"),
            Some("images/a.svg#frag".to_string())
        );
        for external in ["https://example.com/a.png", "//cdn.example.com/a.png"] {
            assert_eq!(resolve_front_matter_image(external), None);
        }
    }

    #[test]
    fn entry_paths_union_and_dedupe() {
        let e = entry(
            "/posts/alpha/",
            Some("/images/a.svg"),
            &["/images/a.svg", "/images/b.png"],
        );
        assert_eq!(
            entry_asset_paths(&e),
            vec!["images/a.svg".to_string(), "images/b.png".to_string()]
        );
    }

    #[test]
    fn output_path_is_identity_in_a1() {
        assert_eq!(output_path_for_source("images/hero.jpg"), "images/hero.jpg");
    }

    #[test]
    fn derivative_format_parses_avif_and_webp() {
        assert_eq!(
            DerivativeFormat::parse("webp").expect("webp parses"),
            DerivativeFormat::WebP
        );
        assert_eq!(
            DerivativeFormat::parse("avif").expect("avif parses"),
            DerivativeFormat::Avif
        );
        assert_eq!(
            DerivativeFormat::parse(" Avif ").expect("trims and folds"),
            DerivativeFormat::Avif
        );
        for bad in ["jpeg", "png", "", "web p", "jxl"] {
            assert!(
                DerivativeFormat::parse(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
        assert_eq!(DerivativeFormat::WebP.extension(), "webp");
        assert_eq!(DerivativeFormat::WebP.mime(), "image/webp");
        assert_eq!(DerivativeFormat::Avif.extension(), "avif");
        assert_eq!(DerivativeFormat::Avif.mime(), "image/avif");
        // AVIF sorts before WebP in `<source>` order.
        assert!(DerivativeFormat::Avif.source_rank() < DerivativeFormat::WebP.source_rank());
        // Same source/width in different formats: different identities,
        // different output paths.
        let webp =
            DerivativeSpec::new("images/hero.jpg", 640, DerivativeFormat::WebP).expect("valid");
        let avif =
            DerivativeSpec::new("images/hero.jpg", 640, DerivativeFormat::Avif).expect("valid");
        assert_ne!(webp, avif);
        assert_eq!(webp.output_path(), "images/hero-640.webp");
        assert_eq!(avif.output_path(), "images/hero-640.avif");
    }

    #[test]
    fn derivable_sources_are_raster_only() {
        for ok in [
            "hero.jpg",
            "HERO.JPG",
            "a.jpeg",
            "a.png",
            "a.webp",
            "dir/a.PNG",
        ] {
            assert!(is_derivable_source(ok), "{ok:?}");
        }
        for skipped in [
            "a.svg", "a.gif", "a.avif", "a.css", "a.js", "no-ext", "a.jpgx",
        ] {
            assert!(!is_derivable_source(skipped), "{skipped:?}");
        }
    }

    #[test]
    fn derivative_identity_and_naming() {
        let base =
            DerivativeSpec::new("images/hero.jpg", 640, DerivativeFormat::WebP).expect("valid");
        // Same triple → same spec, same output.
        assert_eq!(
            base,
            DerivativeSpec::new("images/hero.jpg", 640, DerivativeFormat::WebP).expect("valid")
        );
        assert_eq!(base.output_path(), "images/hero-640.webp");
        // Different widths → different identities and paths.
        let wide =
            DerivativeSpec::new("images/hero.jpg", 1280, DerivativeFormat::WebP).expect("valid");
        assert_ne!(base, wide);
        assert_eq!(wide.output_path(), "images/hero-1280.webp");
        // Root-level sources keep no directory prefix.
        assert_eq!(
            DerivativeSpec::new("hero.jpg", 640, DerivativeFormat::WebP)
                .expect("valid")
                .output_path(),
            "hero-640.webp"
        );
        // Zero widths are rejected, never planned.
        assert!(DerivativeSpec::new("images/hero.jpg", 0, DerivativeFormat::WebP).is_err());
    }

    #[test]
    fn derivative_dimensions_preserve_aspect_and_never_upscale() {
        let down =
            DerivativeSpec::new("images/hero.jpg", 800, DerivativeFormat::WebP).expect("valid");
        // 1600 × 900 at 800w → 800 × 450.
        assert_eq!(down.target_dimensions(1600, 900), (800, 450));
        // Requested widths at or above the source clamp to the source.
        assert_eq!(down.target_dimensions(800, 450), (800, 450));
        assert_eq!(down.target_dimensions(640, 360), (640, 360));
        // Half-up rounding is exact integer math: 1000 × 333 at 333w.
        let odd =
            DerivativeSpec::new("images/hero.jpg", 333, DerivativeFormat::WebP).expect("valid");
        assert_eq!(odd.target_dimensions(1000, 333), (333, 111));
        // Degenerate inputs floor instead of dividing by zero.
        assert_eq!(down.target_dimensions(0, 900), (1, 1));
    }

    fn view(output: &str, requested: u32, actual: (u32, u32)) -> DerivativeView {
        DerivativeView {
            output: output.to_string(),
            width: requested,
            format: DerivativeFormat::WebP,
            actual_width: actual.0,
            actual_height: actual.1,
        }
    }

    fn avif_view(output: &str, requested: u32, actual: (u32, u32)) -> DerivativeView {
        DerivativeView {
            output: output.to_string(),
            width: requested,
            format: DerivativeFormat::Avif,
            actual_width: actual.0,
            actual_height: actual.1,
        }
    }

    #[test]
    fn responsive_selects_sorted_deduped_actual_widths() {
        let views = vec![
            view("images/hero-1920.webp", 1920, (1600, 900)),
            view("images/hero-640.webp", 640, (640, 360)),
            view("images/hero-1280.webp", 1280, (1280, 720)),
        ];
        let responsive = responsive_image("images/hero.jpg", (1600, 900), &views).expect("selects");
        let widths: Vec<u32> = responsive
            .candidates
            .iter()
            .map(|candidate| candidate.width)
            .collect();
        // Actual widths ascending — the clamped 1920 request advertises
        // 1600w, never 1920w.
        assert_eq!(widths, vec![640, 1280, 1600]);
        assert_eq!(
            responsive.srcset,
            "/images/hero-640.webp 640w, /images/hero-1280.webp 1280w, /images/hero-1920.webp 1600w"
        );
        // Default is the largest available representation.
        assert_eq!(responsive.default_src, "/images/hero-1920.webp");
        assert_eq!((responsive.width, responsive.height), (1600, 900));
        assert_eq!(responsive.sizes, "100vw");
    }

    #[test]
    fn responsive_dedupes_equal_actual_widths_by_smallest_request() {
        // Two clamped requests collapsing onto one actual width: one
        // candidate survives, keeping the tighter label.
        let views = vec![
            view("images/logo-1920.webp", 1920, (800, 600)),
            view("images/logo-1280.webp", 1280, (800, 600)),
            view("images/logo-640.webp", 640, (640, 480)),
        ];
        let responsive = responsive_image("images/logo.png", (800, 600), &views).expect("selects");
        let urls: Vec<&str> = responsive
            .candidates
            .iter()
            .map(|candidate| candidate.url.as_str())
            .collect();
        assert_eq!(
            urls,
            vec!["/images/logo-640.webp", "/images/logo-1280.webp"]
        );
        assert_eq!(responsive.default_src, "/images/logo-1280.webp");
    }

    #[test]
    fn responsive_encodes_urls_and_rejects_empties() {
        let views = vec![view("images/a b-640.webp", 640, (640, 360))];
        let responsive = responsive_image("images/a b.png", (1600, 900), &views).expect("selects");
        assert_eq!(responsive.srcset, "/images/a%20b-640.webp 640w");
        // No views, or none within the source, selects nothing: callers
        // leave the original markup untouched.
        assert!(responsive_image("images/a.png", (1600, 900), &[]).is_none());
        assert!(responsive_image(
            "images/a.png",
            (1600, 900),
            &[view("images/a-3200.webp", 3200, (3200, 1800))]
        )
        .is_none());
    }

    #[test]
    fn responsive_groups_formats_avif_first_with_webp_fallback() {
        // Author order (WebP views first) never affects output: groups
        // order AVIF-before-WebP, each deduplicated independently.
        let views = vec![
            view("images/hero-640.webp", 640, (640, 360)),
            avif_view("images/hero-1280.avif", 1280, (1280, 720)),
            view("images/hero-1920.webp", 1920, (1600, 900)),
            avif_view("images/hero-640.avif", 640, (640, 360)),
            avif_view("images/hero-1920.avif", 1920, (1600, 900)),
            view("images/hero-1280.webp", 1280, (1280, 720)),
        ];
        let responsive = responsive_image("images/hero.jpg", (1600, 900), &views).expect("selects");
        assert!(responsive.has_picture());
        assert_eq!(
            responsive
                .sources
                .iter()
                .map(|group| group.format)
                .collect::<Vec<_>>(),
            vec![DerivativeFormat::Avif, DerivativeFormat::WebP]
        );
        assert_eq!(
            responsive.sources[0].srcset,
            "/images/hero-640.avif 640w, /images/hero-1280.avif 1280w, /images/hero-1920.avif 1600w"
        );
        assert_eq!(responsive.sources[0].mime, "image/avif");
        assert_eq!(
            responsive.sources[1].srcset,
            "/images/hero-640.webp 640w, /images/hero-1280.webp 1280w, /images/hero-1920.webp 1600w"
        );
        // Fallback prefers WebP: flat fields mirror the WebP group.
        assert_eq!(responsive.default_src, "/images/hero-1920.webp");
        assert_eq!(responsive.srcset, responsive.sources[1].srcset);
        assert!(responsive
            .candidates
            .iter()
            .all(|candidate| candidate.format == DerivativeFormat::WebP));
    }

    #[test]
    fn responsive_avif_only_falls_back_to_its_sole_group() {
        let views = vec![
            avif_view("images/hero-1280.avif", 1280, (1280, 720)),
            avif_view("images/hero-640.avif", 640, (640, 360)),
        ];
        let responsive = responsive_image("images/hero.jpg", (1600, 900), &views).expect("selects");
        assert!(!responsive.has_picture());
        assert_eq!(responsive.sources.len(), 1);
        assert_eq!(responsive.default_src, "/images/hero-1280.avif");
        assert_eq!(
            responsive.srcset,
            "/images/hero-640.avif 640w, /images/hero-1280.avif 1280w"
        );
    }
}
