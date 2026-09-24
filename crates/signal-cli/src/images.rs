//! Image derivative producer (A2, ADR 0029; AVIF added in A4, ADR 0031):
//! decode, resize, WebP/AVIF-encode.
//!
//! This module is the single implementation boundary around image bytes.
//! The planner reasons about derivatives as specs and inputs (see
//! `signal_core::asset`); everything here deals with pixels and files:
//!
//! ```text
//! DerivativeSpec (core: source, width, format)
//!       ↓
//! read source bytes (raw form, then percent-decoded form)
//!       ↓
//! decode (magic-sniffed PNG/JPEG/WebP) → dimensions
//!       ↓
//! resize to clamped aspect-preserving dimensions (skipped when equal)
//!       ↓
//! WebP (lossless) or AVIF (fixed quality/speed) encode → artifact bytes
//! ```
//!
//! Codec details never leave this module: planning, manifests, `check`,
//! and `explain` consume [`DerivedOutput`] values and [`image::ImageError`]
//! diagnostics, never `image`- or `ravif`-crate types.
//!
//! Determinism contract: fixed Lanczos3 resampling over pure integer
//! target dimensions plus fixed lossless WebP encoding plus fixed-setting
//! single-threaded AVIF encoding (ravif without `asm`/`threading`: no
//! system libraries, no thread-count-dependent bytes). No timestamps, no
//! quality heuristics, no tuning knobs. Any future change that can alter
//! output bytes (encoder version, filter, settings) MUST bump
//! `GENERATION_BEHAVIOR_VERSION` — the behavior gate is the encoder's
//! reuse identity, since A2 records no per-derivative encoder version.

use std::path::Path;

use signal_core::{DerivativeFormat, DerivativeSpec, SignalConfig, SiteModel};

use crate::errors::BuildError;

/// One produced derivative: encoded bytes plus actual output dimensions.
///
/// Dimensions equal the spec's clamped target for the decoded source.
/// They are measured facts (for `explain` and future `srcset`
/// rendering), never plan inputs — planning stays byte-free.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedOutput {
    /// Encoded bytes (WebP or AVIF per the spec's format).
    pub bytes: Vec<u8>,
    /// Actual output width in pixels.
    pub width: u32,
    /// Actual output height in pixels.
    pub height: u32,
    /// Decoded source width in pixels.
    pub source_width: u32,
    /// Decoded source height in pixels.
    pub source_height: u32,
    /// Source format as decoded (PNG, JPEG, or WebP).
    pub source_format: String,
}

/// Read a `static/`-relative source file, trying the raw form first and
/// the percent-decoded form second — the same precedence `link_check`
/// validates with and `current_input_digest` digests with. Shared with the
/// social-image generator (A5), which reads hero sources with identical
/// precedence.
pub(crate) fn read_source_bytes(
    root: &Path,
    source: &str,
) -> Result<(Vec<u8>, String), BuildError> {
    let direct = root.join("static").join(source);
    match std::fs::read(&direct) {
        Ok(bytes) => Ok((bytes, source.to_string())),
        Err(first) => {
            if let Some(decoded) = signal_core::percent_decode(source) {
                if decoded != source {
                    let fallback = root.join("static").join(&decoded);
                    if let Ok(bytes) = std::fs::read(&fallback) {
                        return Ok((bytes, decoded));
                    }
                }
            }
            Err(BuildError::Read {
                path: direct.display().to_string(),
                message: first.to_string(),
            })
        }
    }
}

/// Decode image bytes to dimensions without producing pixels.
///
/// Magic-sniffed: content determines the format, so a GIF wearing a
/// `.png` extension fails here clearly instead of producing output.
pub fn probe_dimensions(bytes: &[u8]) -> Result<(u32, u32, String), String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("could not identify image format: {e}"))?;
    let format = reader
        .format()
        .ok_or_else(|| "could not identify image format: unrecognized magic bytes".to_string())?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| format!("could not decode image dimensions: {e}"))?;
    Ok((width, height, format_name(format)))
}

fn format_name(format: image::ImageFormat) -> String {
    match format {
        image::ImageFormat::Png => "PNG".to_string(),
        image::ImageFormat::Jpeg => "JPEG".to_string(),
        image::ImageFormat::WebP => "WebP".to_string(),
        other => format!("{other:?}"),
    }
}

/// Produce one derivative from decoded-ready source bytes.
///
/// Decode → clamp → resize (Lanczos3, skipped when the target equals the
/// source) → encode in the spec's format (lossless WebP, fixed-setting
/// AVIF). Only PNG/JPEG/WebP sources are accepted; anything else (GIF,
/// SVG-as-bytes, unknown magic) is a deterministic error, never a silent
/// conversion.
pub fn render_derivative(bytes: &[u8], spec: &DerivativeSpec) -> Result<DerivedOutput, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|e| format!("could not decode {}: {e}", spec.source))?;
    let (source_width, source_height) = (image.width(), image.height());
    let source_format = sniff_format(bytes);
    let (width, height) = spec.target_dimensions(source_width, source_height);
    let resized = if (width, height) == (source_width, source_height) {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    };
    let bytes = match spec.format {
        DerivativeFormat::WebP => encode_webp(&resized)?,
        DerivativeFormat::Avif => encode_avif(&resized)?,
    };
    Ok(DerivedOutput {
        bytes,
        width,
        height,
        source_width,
        source_height,
        source_format,
    })
}

/// Best-effort source format label from magic bytes (for reporting only;
/// decoding authority stays with `load_from_memory`).
fn sniff_format(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']) {
        "PNG".to_string()
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "JPEG".to_string()
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "WebP".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Fixed AVIF encoder identity (A4, ADR 0031).
///
/// ravif quality on its 1–100 scale: 70 is the documented still-image
/// default neighborhood — substantially smaller than the lossless WebP
/// fallback while visually near-transparent on photographic content. It
/// is a fixed build constant, not a user knob: no quality sliders, no
/// heuristics, and any change rides `GENERATION_BEHAVIOR_VERSION` like
/// every other byte-affecting encoder change.
pub const AVIF_QUALITY: f32 = 70.0;
/// Fixed ravif speed (1 slowest/best compression – 10 fastest). 10: rav1e
/// without threading or asm is slow (seconds per photographic
/// derivative at this scale), so the fastest fixed speed keeps site
/// builds practical; the compression cost is fixed and documented, and
/// unchanged sources never re-encode (manifest reuse). Fixed like
/// quality: changing it changes bytes.
pub const AVIF_SPEED: u8 = 10;

/// AVIF-encode an 8-bit image via ravif (rav1e, pure Rust: the crate is
/// built without its `asm`/`threading` features, so encoding needs no
/// system libraries and runs single-threaded — the determinism basis
/// alongside the fixed quality/speed above).
///
/// RGB inputs encode without an alpha plane; RGBA inputs let ravif drop
/// the alpha plane automatically when every pixel is opaque. Higher-
/// than-8-bit sources are deterministically reduced to 8-bit first,
/// mirroring the WebP path.
fn encode_avif(image: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::GenericImageView as _;
    use rgb::FromSlice as _;
    let encoder = ravif::Encoder::new()
        .with_quality(AVIF_QUALITY)
        .with_speed(AVIF_SPEED);
    let (width, height) = image.dimensions();
    let encoded = match image {
        image::DynamicImage::ImageRgb8(buffer) => {
            let pixels = buffer.as_raw().as_rgb();
            encoder
                .encode_rgb(imgref::Img::new(pixels, width as usize, height as usize))
                .map_err(|e| format!("could not encode AVIF: {e}"))?
        }
        image::DynamicImage::ImageRgba8(buffer) => {
            let pixels = buffer.as_raw().as_rgba();
            encoder
                .encode_rgba(imgref::Img::new(pixels, width as usize, height as usize))
                .map_err(|e| format!("could not encode AVIF: {e}"))?
        }
        other if other.color().has_alpha() => {
            let buffer = other.to_rgba8();
            let pixels = buffer.as_raw().as_rgba();
            encoder
                .encode_rgba(imgref::Img::new(pixels, width as usize, height as usize))
                .map_err(|e| format!("could not encode AVIF: {e}"))?
        }
        other => {
            let buffer = other.to_rgb8();
            let pixels = buffer.as_raw().as_rgb();
            encoder
                .encode_rgb(imgref::Img::new(pixels, width as usize, height as usize))
                .map_err(|e| format!("could not encode AVIF: {e}"))?
        }
    };
    Ok(encoded.avif_file)
}

/// Lossless-WebP encode an 8-bit image. Higher-than-8-bit sources are
/// deterministically reduced to 8-bit first (WebP lossless is an 8-bit
/// format); 8-bit sources pass through untouched.
fn encode_webp(image: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::GenericImageView as _;
    let (pixels, color) = match image {
        image::DynamicImage::ImageRgb8(buffer) => {
            (buffer.as_raw().clone(), image::ExtendedColorType::Rgb8)
        }
        image::DynamicImage::ImageRgba8(buffer) => {
            (buffer.as_raw().clone(), image::ExtendedColorType::Rgba8)
        }
        image::DynamicImage::ImageLuma8(buffer) => {
            (buffer.as_raw().clone(), image::ExtendedColorType::L8)
        }
        image::DynamicImage::ImageLumaA8(buffer) => {
            (buffer.as_raw().clone(), image::ExtendedColorType::La8)
        }
        other => {
            let buffer = other.to_rgba8();
            (buffer.as_raw().clone(), image::ExtendedColorType::Rgba8)
        }
    };
    let (actual_width, actual_height) = image.dimensions();
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .encode(&pixels, actual_width, actual_height, color)
        .map_err(|e| format!("could not encode WebP: {e}"))?;
    Ok(out)
}

/// Read and produce one derivative from disk: the [`resolve_artifact`]
/// producer for `ArtifactKind::DerivedImage`. Self-contained like every
/// other resolver: one spec in, bytes out, no loop-local state.
pub fn resolve_derived_image(
    root: &Path,
    spec: &signal_core::ArtifactSpec,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<Vec<u8>, BuildError> {
    let deriv =
        derivative_for_output(config, model, &spec.path).ok_or_else(|| BuildError::Model {
            message: format!("unrecognized derivative path {:?}", spec.path),
        })?;
    let (bytes, _) = read_source_bytes(root, &deriv.source)?;
    render_derivative(&bytes, &deriv)
        .map(|output| output.bytes)
        .map_err(|message| BuildError::Read {
            path: root
                .join("static")
                .join(&deriv.source)
                .display()
                .to_string(),
            message,
        })
}

/// Every derivative the current site state requests, in deterministic
/// (output-path) order.
///
/// Pure over config plus model: content-referenced raster sources crossed
/// with configured widths and formats. The single enumeration planning
/// (`generate_specs`), input derivation (`artifact_inputs`), resolution
/// inversion, and `explain` all share — the `feed_identity` precedent —
/// so the four can never disagree about which derivative an output path
/// is. No bytes are read: unsupported-but-well-formed inputs fail later
/// at resolve/probe time with actionable diagnostics.
pub fn planned_derivatives(
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<Vec<DerivativeSpec>, BuildError> {
    let Some(request) = derivative_request(config)? else {
        return Ok(Vec::new());
    };
    let mut sources = std::collections::BTreeSet::new();
    for entry in model.entries() {
        for path in signal_core::entry_asset_paths(entry) {
            if signal_core::is_derivable_source(&path) {
                sources.insert(path);
            }
        }
    }
    let mut out = Vec::new();
    for source in &sources {
        for width in &request.widths {
            for format in &request.formats {
                out.push(
                    DerivativeSpec::new(source.clone(), *width, *format).map_err(|message| {
                        BuildError::Model {
                            message: format!("invalid [images] configuration: {message}"),
                        }
                    })?,
                );
            }
        }
    }
    out.sort_by_key(|deriv| deriv.output_path());
    Ok(out)
}

/// The configured derivative request, if any: sorted/deduped widths plus
/// the output formats in canonical (`avif`-before-`webp`) order.
/// Shared by global and per-entry enumeration so both agree on what
/// "requested" means. The config gates validate the same shapes even
/// earlier; errors here are defense in depth.
struct DerivativeRequest {
    /// Requested widths, sorted and deduplicated.
    widths: Vec<u32>,
    /// Requested formats in canonical order.
    formats: Vec<DerivativeFormat>,
}

/// The configured derivative formats, in canonical (`avif`-before-`webp`)
/// order: parses every name, rejects an ambiguous `format` + `formats`
/// pair, and deduplicates. Pure.
///
/// The single source of truth for "which formats are requested" — the
/// shared config gates and [`derivative_request`] both call it, so the
/// failure surface (`signal build`, `check`, `--explain`) can never
/// disagree with what planning enumerates.
pub(crate) fn configured_formats(
    config: &SignalConfig,
) -> Result<Vec<DerivativeFormat>, BuildError> {
    let Some(images) = config.images.as_ref() else {
        return Ok(Vec::new());
    };
    let plural = images
        .formats
        .iter()
        .any(|format| !format.trim().is_empty());
    if plural && config.image_format().is_some() {
        return Err(BuildError::Model {
            message:
                "invalid [images] configuration: specify either `format` or `formats`, not both"
                    .to_string(),
        });
    }
    let mut formats = Vec::new();
    for name in config.image_formats() {
        formats.push(
            DerivativeFormat::parse(&name).map_err(|message| BuildError::Model {
                message: format!("invalid [images] configuration: {message}"),
            })?,
        );
    }
    formats.sort_by_key(|format| format.source_rank());
    formats.dedup();
    Ok(formats)
}

fn derivative_request(config: &SignalConfig) -> Result<Option<DerivativeRequest>, BuildError> {
    let widths = config.image_widths();
    if widths.is_empty() {
        return Ok(None);
    }
    let formats = configured_formats(config)?;
    Ok(Some(DerivativeRequest { widths, formats }))
}

/// Every derivative one entry's references request, in deterministic
/// (output-path) order: the entry's raster sources crossed with the
/// configured widths and formats. Shared by page and section input derivation.
pub fn entry_derivatives(
    config: &SignalConfig,
    entry: &signal_core::ContentEntry,
) -> Result<Vec<DerivativeSpec>, BuildError> {
    let Some(request) = derivative_request(config)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for path in signal_core::entry_asset_paths(entry) {
        if !signal_core::is_derivable_source(&path) {
            continue;
        }
        for width in &request.widths {
            for format in &request.formats {
                out.push(DerivativeSpec::new(path.clone(), *width, *format).map_err(
                    |message| BuildError::Model {
                        message: format!("invalid [images] configuration: {message}"),
                    },
                )?);
            }
        }
    }
    out.sort_by_key(|deriv: &DerivativeSpec| deriv.output_path());
    Ok(out)
}

/// Every derivative one entry's front-matter hero requests, in deterministic
/// output-path order.
///
/// External, absent, and non-raster heroes request nothing. This is the
/// homepage dependency counterpart to [`crate::responsive::responsive_hero`]:
/// its selected featured entry names only the source and derivatives whose
/// generated paths can appear in `featured.responsive_image`.
pub fn hero_derivatives(
    config: &SignalConfig,
    entry: &signal_core::ContentEntry,
) -> Result<Vec<DerivativeSpec>, BuildError> {
    let Some(image) = entry.image.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(source) = signal_core::resolve_front_matter_image(image) else {
        return Ok(Vec::new());
    };
    if !signal_core::is_derivable_source(&source) {
        return Ok(Vec::new());
    }
    // Reuse the page/section derivative enumeration rather than defining a
    // second homepage planning loop. A summary carries only the hero, so
    // retain exactly that source from the full entry's planned derivatives.
    Ok(entry_derivatives(config, entry)?
        .into_iter()
        .filter(|deriv| deriv.source == source)
        .collect())
}

/// Select one source's responsive representation (A3, ADR 0030):
/// decode once, build actual-dimension views, run the shared pure
/// selector.
///
/// Returns `None` without touching the filesystem when no derivatives
/// are configured or the source is not derivable — the cases where
/// rendering must leave the original markup untouched. Otherwise reads
/// and decodes the source (raw form, then percent-decoded form); the
/// caller runs post-validation, so the source exists and decodes, and
/// any failure here is a hard read error, never a silent fallback.
pub fn responsive_for_source(
    root: &Path,
    config: &SignalConfig,
    source: &str,
) -> Result<Option<signal_core::ResponsiveImage>, BuildError> {
    let Some(request) = derivative_request(config)? else {
        return Ok(None);
    };
    if !signal_core::is_derivable_source(source) {
        return Ok(None);
    }
    let (bytes, _) = read_source_bytes(root, source)?;
    let path = root.join("static").join(source);
    let (source_width, source_height, _) =
        probe_dimensions(&bytes).map_err(|message| BuildError::Read {
            path: path.display().to_string(),
            message,
        })?;
    let mut views = Vec::new();
    for width in &request.widths {
        for format in &request.formats {
            let spec = signal_core::DerivativeSpec::new(source.to_string(), *width, *format)
                .map_err(|message| BuildError::Model {
                    message: format!("invalid [images] configuration: {message}"),
                })?;
            let (actual_width, actual_height) = spec.target_dimensions(source_width, source_height);
            views.push(signal_core::DerivativeView {
                output: spec.output_path(),
                width: spec.width,
                format: spec.format,
                actual_width,
                actual_height,
            });
        }
    }
    Ok(signal_core::responsive_image(
        source,
        (source_width, source_height),
        &views,
    ))
}

/// Invert an output path to its derivative request: the spec whose
/// [`DerivativeSpec::output_path`] equals `path`, if the current site
/// state plans one. Shared verbatim by resolution and explanation.
pub fn derivative_for_output(
    config: &SignalConfig,
    model: &SiteModel,
    path: &str,
) -> Option<DerivativeSpec> {
    planned_derivatives(config, model)
        .ok()?
        .into_iter()
        .find(|deriv| deriv.output_path() == path)
}

/// Validate every planned derivative source: it exists, and its bytes
/// decode as a raster image.
///
/// Called from both `validated_plan` and `validated_plan_for_check`
/// (after spec generation), so `build`, `check`, and `explain` fail
/// identically on missing, unsupported, or malformed sources — before
/// any write, prune, or manifest step. Read-only: sources are read,
/// nothing is written.
pub fn validate_derivative_sources(
    root: &Path,
    specs: &[signal_core::ArtifactSpec],
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<(), BuildError> {
    let mut sources = std::collections::BTreeSet::new();
    for spec in specs {
        if spec.kind != signal_core::ArtifactKind::DerivedImage {
            continue;
        }
        let deriv =
            derivative_for_output(config, model, &spec.path).ok_or_else(|| BuildError::Model {
                message: format!("unrecognized derivative path {:?}", spec.path),
            })?;
        sources.insert(deriv.source);
    }
    for source in sources {
        let (bytes, actual) = read_source_bytes(root, &source)?;
        // Extension support was decided at plan time; content support is
        // decided here: only PNG/JPEG/WebP magic decodes.
        match probe_dimensions(&bytes) {
            Ok((_, _, format)) if matches!(format.as_str(), "PNG" | "JPEG" | "WebP") => {}
            Ok((_, _, format)) => {
                return Err(BuildError::Read {
                    path: root.join("static").join(&actual).display().to_string(),
                    message: format!(
                        "unsupported image content for {source:?}: decoded as {format}, only PNG, JPEG, and WebP sources can be derived"
                    ),
                });
            }
            Err(message) => {
                return Err(BuildError::Read {
                    path: root.join("static").join(&actual).display().to_string(),
                    message: format!("could not decode image {source:?}: {message}"),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder as _;

    /// Deterministic 160 × 90 RGB gradient fixture, encoded in-test: no
    /// binary blobs in the repository, identical bytes on every run.
    fn gradient_png() -> Vec<u8> {
        let mut image = image::RgbImage::new(160, 90);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, 128];
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 160, 90, image::ExtendedColorType::Rgb8)
            .expect("fixture encodes");
        bytes
    }

    fn gradient_jpeg() -> Vec<u8> {
        let mut image = image::RgbImage::new(160, 90);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, 128];
        }
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .write_image(image.as_raw(), 160, 90, image::ExtendedColorType::Rgb8)
            .expect("fixture encodes");
        bytes
    }

    fn gradient_webp() -> Vec<u8> {
        let mut image = image::RgbImage::new(160, 90);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, 128];
        }
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .write_image(image.as_raw(), 160, 90, image::ExtendedColorType::Rgb8)
            .expect("fixture encodes");
        bytes
    }

    fn spec(source: &str, width: u32) -> DerivativeSpec {
        DerivativeSpec::new(source, width, DerivativeFormat::WebP).expect("valid")
    }

    fn avif_spec(source: &str, width: u32) -> DerivativeSpec {
        DerivativeSpec::new(source, width, DerivativeFormat::Avif).expect("valid")
    }

    #[test]
    fn render_avif_produces_valid_output_at_target_dimensions() {
        let bytes = gradient_png();
        let start = std::time::Instant::now();
        let output = render_derivative(&bytes, &avif_spec("images/hero.png", 80)).expect("renders");
        eprintln!("avif 80w encode: {:?}", start.elapsed());
        assert_eq!((output.width, output.height), (80, 45));
        assert_eq!((output.source_width, output.source_height), (160, 90));
        // AVIF container magic: `....ftypavif` (ISO-BMFF with AVIF brand).
        assert!(output.bytes.len() > 12, "non-trivial payload");
        assert_eq!(&output.bytes[4..12], b"ftypavif");
    }

    #[test]
    fn render_avif_clamps_instead_of_upscaling() {
        let bytes = gradient_png();
        let output =
            render_derivative(&bytes, &avif_spec("images/hero.png", 320)).expect("renders");
        assert_eq!((output.width, output.height), (160, 90));
    }

    #[test]
    fn render_avif_is_deterministic() {
        let bytes = gradient_png();
        let first =
            render_derivative(&bytes, &avif_spec("images/hero.png", 80)).expect("first renders");
        let second =
            render_derivative(&bytes, &avif_spec("images/hero.png", 80)).expect("second renders");
        assert_eq!(first.bytes, second.bytes);
    }

    #[test]
    fn render_avif_large_source_stays_practical() {
        // 1600 × 900 (the A4 fixture scale): guards the fixed speed
        // setting against rav1e slowness regressions.
        let mut image = image::RgbImage::new(1600, 900);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8];
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 1600, 900, image::ExtendedColorType::Rgb8)
            .expect("fixture encodes");
        let start = std::time::Instant::now();
        let output =
            render_derivative(&bytes, &avif_spec("images/hero.png", 640)).expect("renders");
        let elapsed = start.elapsed();
        eprintln!(
            "avif 1600x900→640w encode: {elapsed:?} ({} bytes)",
            output.bytes.len()
        );
        assert_eq!((output.width, output.height), (640, 360));
        assert_eq!(&output.bytes[4..12], b"ftypavif");
    }

    #[test]
    fn probe_reads_png_and_jpeg_dimensions() {
        assert_eq!(
            probe_dimensions(&gradient_png()).expect("png probes").0,
            160
        );
        let (width, height, format) = probe_dimensions(&gradient_jpeg()).expect("jpeg probes");
        assert_eq!((width, height), (160, 90));
        assert_eq!(format, "JPEG");
    }

    #[test]
    fn probe_reads_webp_dimensions() {
        let (width, height, format) = probe_dimensions(&gradient_webp()).expect("webp probes");
        assert_eq!((width, height), (160, 90));
        assert_eq!(format, "WebP");
    }

    #[test]
    fn render_webp_source_to_webp_and_avif() {
        let bytes = gradient_webp();
        let webp = render_derivative(&bytes, &spec("images/hero.webp", 80)).expect("webp renders");
        assert_eq!(webp.source_format, "WebP");
        assert_eq!((webp.width, webp.height), (80, 45));
        assert!(webp.bytes.starts_with(b"RIFF"));
        assert_eq!(&webp.bytes[8..12], b"WEBP");
        let back = image::load_from_memory(&webp.bytes).expect("webp output decodes");
        assert_eq!((back.width(), back.height()), (80, 45));

        let avif =
            render_derivative(&bytes, &avif_spec("images/hero.webp", 80)).expect("avif renders");
        assert_eq!(avif.source_format, "WebP");
        assert_eq!((avif.width, avif.height), (80, 45));
        assert_eq!(&avif.bytes[4..12], b"ftypavif");
    }

    #[test]
    fn probe_rejects_non_raster_and_malformed_bytes() {
        // Unrecognized magic and truncated inputs fail with diagnostics.
        assert!(probe_dimensions(b"not an image").is_err());
        assert!(probe_dimensions(&[]).is_err());
        assert!(probe_dimensions(&[0x89, b'P', b'N', b'G']).is_err());
        // A raster extension with non-image content fails at decode.
        assert!(image::load_from_memory(b"nope").is_err());
    }

    #[test]
    fn render_produces_valid_webp_at_target_dimensions() {
        let bytes = gradient_png();
        let output = render_derivative(&bytes, &spec("images/hero.png", 80)).expect("renders");
        assert_eq!((output.width, output.height), (80, 45));
        assert_eq!((output.source_width, output.source_height), (160, 90));
        // WebP container magic: RIFF....WEBP (lossless VP8L payload).
        assert!(output.bytes.starts_with(b"RIFF"));
        assert_eq!(&output.bytes[8..12], b"WEBP");
        // The output decodes back to the advertised dimensions.
        let back = image::load_from_memory(&output.bytes).expect("webp decodes");
        assert_eq!((back.width(), back.height()), (80, 45));
    }

    #[test]
    fn render_clamps_instead_of_upscaling() {
        let bytes = gradient_png();
        let output = render_derivative(&bytes, &spec("images/hero.png", 320)).expect("renders");
        assert_eq!((output.width, output.height), (160, 90));
        let back = image::load_from_memory(&output.bytes).expect("webp decodes");
        assert_eq!((back.width(), back.height()), (160, 90));
    }

    #[test]
    fn render_is_deterministic() {
        let bytes = gradient_png();
        let first = render_derivative(&bytes, &spec("images/hero.png", 80)).expect("first renders");
        let second =
            render_derivative(&bytes, &spec("images/hero.png", 80)).expect("second renders");
        assert_eq!(first.bytes, second.bytes);
    }

    #[test]
    fn render_rejects_undecodable_sources_clearly() {
        let err = render_derivative(b"definitely not pixels", &spec("images/hero.png", 80))
            .expect_err("must fail");
        assert!(err.contains("could not decode"), "got: {err}");
    }

    #[test]
    fn webp_output_carries_image_mime() {
        assert_eq!(DerivativeFormat::WebP.mime(), "image/webp");
    }
}
