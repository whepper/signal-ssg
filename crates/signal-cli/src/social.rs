//! Social-image generator (A5, ADR 0032): page metadata plus an optional
//! hero image → one deterministic 1200 × 630 PNG card.
//!
//! Like [`crate::images`] for image derivatives, this module is the single
//! implementation boundary around social-card pixels. Identity, naming,
//! participation, and URL forms live in `signal_core::social`; planning
//! reasons about `ArtifactKind::SocialImage` specs and inputs, never about
//! layout:
//!
//! ```text
//! route → SocialImage artifact  (signal_core::social)
//!       ↓
//! entry metadata + optional hero source bytes
//!       ↓
//! fixed layout: background, accent bar, hero cover crop, text
//!       ↓
//! RGBA compose (opaque) → RGB8 → deterministic PNG
//! ```
//!
//! Determinism contract: the font is compiled in (`include_bytes!`, a
//! Latin subset of Roboto — see `assets/fonts/README.md`), so rendering
//! never consults system fonts; every coordinate is pure integer/float
//! math over the configured dimensions (layout metrics scale linearly with
//! the configured height); text advances are per-glyph `ab_glyph` metrics
//! (no kerning, no shaping); the hero is cropped with centered integer math
//! and resized with the same Lanczos3 filter the derivative pipeline uses;
//! PNG encoding pins compression and filter. No timestamps, no randomness,
//! no environment data.
//!
//! Any change that can alter card bytes — font, palette, layout
//! constants, encoder, or `GENERATION_BEHAVIOR_VERSION`-relevant code —
//! MUST bump the generation behavior gate, which is the social artifact's
//! reuse identity (ADR 0032).

use std::path::Path;

use ab_glyph::{point, Font as _, FontRef, Glyph, PxScale, PxScaleFont, ScaleFont as _};
use image::{Rgba, RgbaImage};
use signal_core::{ContentEntry, SignalConfig, SiteModel};

use crate::errors::BuildError;

/// Bundled text fonts (see `assets/fonts/README.md` for provenance and the
/// licence). Compiled in so rendering never reads the filesystem.
const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular-subset.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/Roboto-Bold-subset.ttf");

/// Fixed palette (a stable design system, not a graphics DSL).
const BACKGROUND: [u8; 3] = [0x0B, 0x12, 0x20];
const FOREGROUND: [u8; 3] = [0xF8, 0xFA, 0xFC];
const MUTED: [u8; 3] = [0x94, 0xA3, 0xB8];
const ACCENT: [u8; 3] = [0x38, 0xBD, 0xF8];

/// Fixed layout geometry at the reference size (1200 × 630), in pixels.
/// [`Metrics::scaled`] scales these to the configured height, so any
/// dimension renders the same composition rather than clipping text.
const ACCENT_BAR_WIDTH: f32 = 12.0;
const PADDING: f32 = 72.0;
/// Share of the card width the hero occupies when one is composited.
const HERO_FRACTION: f32 = 0.42;
/// Gap between the hero region and the text column.
const HERO_GAP: f32 = 48.0;
const IDENTITY_SIZE: f32 = 26.0;
const IDENTITY_LINE_HEIGHT: f32 = 32.0;
const GAP_AFTER_IDENTITY: f32 = 26.0;
const TITLE_SIZE: f32 = 58.0;
const TITLE_LINE_HEIGHT: f32 = 68.0;
const TITLE_MAX_LINES: usize = 4;
const GAP_BEFORE_DESCRIPTION: f32 = 30.0;
const DESCRIPTION_SIZE: f32 = 28.0;
const DESCRIPTION_LINE_HEIGHT: f32 = 38.0;
const DESCRIPTION_MAX_LINES: usize = 3;
const BYLINE_SIZE: f32 = 24.0;
/// Below this text-column width nothing is drawn: a card narrow enough to
/// leave no room is a configuration mistake, not a rendering target.
const MIN_TEXT_WIDTH: f32 = 40.0;
/// U+2026 HORIZONTAL ELLIPSIS, present in the bundled subset.
const ELLIPSIS: &str = "\u{2026}";

/// One card's layout metrics: the reference design scaled by the
/// configured height.
///
/// Scaling keeps the composition intact at any allowed dimension — a
/// 600 × 315 card is a faithful half-size card, not a clipped 1200 × 630
/// one — and is a pure function of configuration, so it cannot affect
/// determinism.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Metrics {
    padding: f32,
    accent_bar: u32,
    hero_gap: f32,
    identity_size: f32,
    identity_line_height: f32,
    gap_after_identity: f32,
    title_size: f32,
    title_line_height: f32,
    gap_before_description: f32,
    description_size: f32,
    description_line_height: f32,
    byline_size: f32,
    min_text_width: f32,
}

impl Metrics {
    /// Metrics for a card of `height` pixels, scaled from the reference
    /// design ([`DEFAULT_SOCIAL_HEIGHT`](signal_core::DEFAULT_SOCIAL_HEIGHT)).
    fn scaled(height: u32) -> Self {
        let scale = height.max(1) as f32 / signal_core::DEFAULT_SOCIAL_HEIGHT as f32;
        Self {
            padding: PADDING * scale,
            accent_bar: (ACCENT_BAR_WIDTH * scale).round().max(1.0) as u32,
            hero_gap: HERO_GAP * scale,
            identity_size: IDENTITY_SIZE * scale,
            identity_line_height: IDENTITY_LINE_HEIGHT * scale,
            gap_after_identity: GAP_AFTER_IDENTITY * scale,
            title_size: TITLE_SIZE * scale,
            title_line_height: TITLE_LINE_HEIGHT * scale,
            gap_before_description: GAP_BEFORE_DESCRIPTION * scale,
            description_size: DESCRIPTION_SIZE * scale,
            description_line_height: DESCRIPTION_LINE_HEIGHT * scale,
            byline_size: BYLINE_SIZE * scale,
            min_text_width: MIN_TEXT_WIDTH * scale,
        }
    }
}

/// A font scaled to a pixel size, borrowing the compile-time font data.
type Scaled<'a> = PxScaleFont<&'a FontRef<'static>>;

/// Presentation metadata for one page's social image, as templates
/// consume it (A5, ADR 0032).
///
/// Inserted under `social_image` only for participating pages on enabled
/// sites with a usable `site.base_url`; templates gate with
/// `{% if social_image %}`. `url` is the URL-path form (encoded,
/// root-relative); `absolute_url` is what Open Graph requires.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SocialPageMetadata {
    /// URL-path form of the generated card (encoded, root-relative).
    pub url: String,
    /// Absolute URL of the generated card (Open Graph requirement).
    pub absolute_url: String,
    /// Card width in pixels (as configured).
    pub width: u32,
    /// Card height in pixels (as configured).
    pub height: u32,
}

/// Whether this page's social image is planned, and its URLs.
///
/// Pure: config + entry only. `None` for disabled sites, section roots,
/// opted-out pages, and pages whose site has no `base_url` — the cases
/// where rendering must leave metadata exactly as A4 produced it. (A
/// `[social]`-enabled site without `base_url` never reaches rendering:
/// the shared config gates reject it, mirroring feeds.)
pub fn page_social_metadata(
    config: &SignalConfig,
    entry: &ContentEntry,
) -> Option<SocialPageMetadata> {
    let (width, height) = config.social_size()?;
    if !signal_core::social_image_eligible(config, entry) {
        return None;
    }
    let base_url = config.site.base_url.as_deref()?;
    Some(SocialPageMetadata {
        url: signal_core::social_image_url(&entry.route.0),
        absolute_url: signal_core::social_image_absolute_url(base_url, &entry.route.0),
        width,
        height,
    })
}

/// Resolve one social-image output path against the model, with the
/// planner-disagreement diagnostic.
///
/// Thin wrapper over [`signal_core::social_image_for_output`] — the single
/// path → page inversion shared by input derivation, source validation,
/// resolution, and `explain`. The message covers both failure modes
/// (a non-social path, or a route with no entry); through the CLI the
/// target always comes from the plan, so either is a planner
/// disagreement, not a user error.
pub(crate) fn planned_social<'a>(
    model: &'a SiteModel,
    path: &str,
) -> Result<signal_core::PlannedSocial<'a>, BuildError> {
    signal_core::social_image_for_output(model, path).ok_or_else(|| BuildError::Model {
        message: format!("social image {path:?} does not resolve to a page"),
    })
}

/// Read and produce one social card from disk: the [`resolve_artifact`]
/// producer for `ArtifactKind::SocialImage`. Self-contained like every
/// other resolver: one spec in, bytes out, no loop-local state.
pub fn resolve_social_image(
    root: &Path,
    spec: &signal_core::ArtifactSpec,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<Vec<u8>, BuildError> {
    let planned = planned_social(model, &spec.path)?;
    let (width, height) = config.social_size().ok_or_else(|| BuildError::Model {
        message: format!(
            "social image {:?} is planned but [social] is not enabled",
            spec.path
        ),
    })?;
    // Only raster heroes are composited (the same predicate planning uses
    // for the dependency edge), so a SVG/GIF/external hero yields a
    // metadata-only card instead of a decode failure.
    let hero = match &planned.hero {
        Some(source) => {
            let (bytes, _) = crate::images::read_source_bytes(root, source)?;
            let image = image::load_from_memory(&bytes).map_err(|e| BuildError::Read {
                path: root.join("static").join(source).display().to_string(),
                message: format!("could not decode social hero {source:?}: {e}"),
            })?;
            Some(image)
        }
        None => None,
    };
    let entry = planned.entry;
    let author = entry
        .author
        .clone()
        .or_else(|| config.site.author.clone())
        .filter(|author| !author.trim().is_empty());
    let card_text = Card {
        site: &config.site.title,
        title: &entry.title,
        description: entry.description.as_deref(),
        author: author.as_deref(),
    };
    let card = compose(width, height, &card_text, hero.as_ref());
    encode_png(&card)
}

/// Everything one card renders from. Borrowed: the caller owns the model.
struct Card<'a> {
    site: &'a str,
    title: &'a str,
    description: Option<&'a str>,
    author: Option<&'a str>,
}

/// Render one opaque RGB card. Pure function of its arguments plus the
/// compiled-in font, palette, and layout constants.
fn compose(
    width: u32,
    height: u32,
    text: &Card<'_>,
    hero: Option<&image::DynamicImage>,
) -> RgbaImage {
    let width = width.max(1);
    let height = height.max(1);
    let metrics = Metrics::scaled(height);
    let mut card = RgbaImage::from_pixel(
        width,
        height,
        Rgba([BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 255]),
    );
    draw_accent_bar(&mut card, metrics.accent_bar);

    let hero_x = if hero.is_some() {
        let x = (width as f32 * (1.0 - HERO_FRACTION)).round() as u32;
        // `min` first: on a card narrower than the scaled accent bar the
        // clamp bounds would otherwise invert and panic.
        x.clamp(metrics.accent_bar.min(width), width)
    } else {
        width
    };
    if let Some(hero) = hero {
        let region = (hero_x, 0, width - hero_x, height);
        draw_hero_cover(&mut card, region, hero);
    }

    let left = (metrics.accent_bar as f32 + metrics.padding).min(width as f32);
    let right = if hero.is_some() {
        (hero_x as f32 - metrics.hero_gap).max(left)
    } else {
        (width as f32 - metrics.padding).max(left)
    };
    let text_width = right - left;
    if text_width < metrics.min_text_width {
        return card;
    }

    let regular = regular_font();
    let bold = bold_font();
    let identity = regular.as_scaled(PxScale::from(metrics.identity_size));
    let title_font = bold.as_scaled(PxScale::from(metrics.title_size));
    let description_font = regular.as_scaled(PxScale::from(metrics.description_size));
    let byline = regular.as_scaled(PxScale::from(metrics.byline_size));

    let layout = layout_text(
        &identity,
        &title_font,
        &description_font,
        text,
        &metrics,
        text_width,
    );
    // The text block is vertically centred, never above the padding; the
    // byline is pinned to the bottom-left of the text column.
    let top = ((height as f32 - layout.block_height) / 2.0).max(metrics.padding);
    let mut y = top;
    draw_line(&mut card, &identity, &layout.identity, left, y, ACCENT);
    y += metrics.identity_line_height + metrics.gap_after_identity;
    for line in &layout.title {
        draw_line(&mut card, &title_font, line, left, y, FOREGROUND);
        y += metrics.title_line_height;
    }
    if !layout.description.is_empty() {
        y += metrics.gap_before_description;
        for line in &layout.description {
            draw_line(&mut card, &description_font, line, left, y, MUTED);
            y += metrics.description_line_height;
        }
    }
    if let Some(author) = text.author.filter(|author| !author.trim().is_empty()) {
        let baseline_y = (height as f32 - metrics.padding).max(metrics.padding);
        draw_line(&mut card, &byline, author, left, baseline_y, MUTED);
    }
    card
}

/// Wrapped text plus its measured block height: the deterministic layout,
/// testable without touching pixels.
struct TextLayout {
    identity: String,
    title: Vec<String>,
    description: Vec<String>,
    block_height: f32,
}

/// Wrap every text element to the column width and measure the stack.
fn layout_text(
    identity: &Scaled<'_>,
    title: &Scaled<'_>,
    description: &Scaled<'_>,
    card: &Card<'_>,
    metrics: &Metrics,
    max_width: f32,
) -> TextLayout {
    // The identity line is capped at one line and ellipsized like any
    // other: a long site title must never run into the hero region.
    let identity_text = wrap_text(identity, &card.site.trim().to_uppercase(), max_width, 1)
        .into_iter()
        .next()
        .unwrap_or_default();
    let title_lines = wrap_text(title, card.title, max_width, TITLE_MAX_LINES);
    let description_lines: Vec<String> = card
        .description
        .map(|text| wrap_text(description, text, max_width, DESCRIPTION_MAX_LINES))
        .unwrap_or_default();
    let mut block_height = metrics.identity_line_height + metrics.gap_after_identity;
    block_height += title_lines.len() as f32 * metrics.title_line_height;
    if !description_lines.is_empty() {
        block_height += metrics.gap_before_description
            + description_lines.len() as f32 * metrics.description_line_height;
    }
    TextLayout {
        identity: identity_text,
        title: title_lines,
        description: description_lines,
        block_height,
    }
}

/// Draw one line of text with its baseline at `y`.
fn draw_line(card: &mut RgbaImage, font: &Scaled<'_>, text: &str, x: f32, y: f32, color: [u8; 3]) {
    // Baseline sits `ascent` below the line top.
    let baseline = y + font.ascent();
    let mut pen = x;
    for character in text.chars() {
        let id = font.glyph_id(character);
        let glyph = Glyph {
            id,
            scale: font.scale(),
            position: point(pen, baseline),
        };
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                blend(card, px, py, color, coverage);
            });
        }
        pen += font.h_advance(id);
    }
}

/// Alpha-composite one coverage sample over an opaque card pixel.
///
/// Rounds half-up on integer-scaled coverage so the same inputs always
/// produce the same bytes; out-of-bounds samples are dropped (glyph
/// outlines routinely extend past the column edge).
fn blend(card: &mut RgbaImage, x: i32, y: i32, color: [u8; 3], coverage: f32) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= card.width() || y >= card.height() {
        return;
    }
    let alpha = coverage.clamp(0.0, 1.0);
    let dst = card.get_pixel_mut(x, y);
    for channel in 0..3 {
        let blended = color[channel] as f32 * alpha + dst[channel] as f32 * (1.0 - alpha);
        dst[channel] = (blended + 0.5) as u8;
    }
    dst[3] = 255;
}

/// Measure one string with per-glyph advances (no kerning, no shaping).
fn measure(font: &Scaled<'_>, text: &str) -> f32 {
    text.chars()
        .map(|character| font.h_advance(font.glyph_id(character)))
        .sum()
}

/// Greedy word wrap at `max_width`, capped at `max_lines`.
///
/// Deterministic: whitespace runs normalize to single spaces, words longer
/// than the column hard-break by character, and overflow past the cap is
/// ellipsized on the final line rather than silently dropped.
fn wrap_text(font: &Scaled<'_>, text: &str, max_width: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if measure(font, &candidate) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if measure(font, word) > max_width {
            // Hard-break an oversized word (a long URL, say) by character.
            let mut chunk = String::new();
            for character in word.chars() {
                let mut candidate = chunk.clone();
                candidate.push(character);
                if !chunk.is_empty() && measure(font, &candidate) > max_width {
                    lines.push(std::mem::take(&mut chunk));
                }
                chunk.push(character);
            }
            current = chunk;
        } else {
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = ellipsize(font, last, max_width);
        }
    }
    lines
}

/// Shorten one line so `line + "…"` fits, dropping whole characters from
/// the end. The rule is total: an unlengthy line keeps just the ellipsis.
fn ellipsize(font: &Scaled<'_>, line: &str, max_width: f32) -> String {
    let chars: Vec<char> = line.trim_end().chars().collect();
    let mut keep = chars.len();
    loop {
        let mut candidate: String = chars[..keep].iter().collect();
        candidate.push_str(ELLIPSIS);
        if keep == 0 || measure(font, &candidate) <= max_width {
            return candidate;
        }
        keep -= 1;
    }
}

/// Fixed accent bar down the left edge.
fn draw_accent_bar(card: &mut RgbaImage, bar_width: u32) {
    let width = bar_width.min(card.width());
    for y in 0..card.height() {
        for x in 0..width {
            card.put_pixel(x, y, Rgba([ACCENT[0], ACCENT[1], ACCENT[2], 255]));
        }
    }
}

/// Composite the hero into `region = (x, y, width, height)` with a
/// deterministic cover fit.
///
/// Policy (documented, no focal points): pick the largest source rectangle
/// matching the region's aspect ratio — the whole source on the
/// constrained axis — centre it, then Lanczos3-resize that crop to the
/// region. Integer math only, so the same source bytes always produce the
/// same crop.
fn draw_hero_cover(card: &mut RgbaImage, region: (u32, u32, u32, u32), hero: &image::DynamicImage) {
    let (region_x, region_y, region_width, region_height) = region;
    // Clip the region to the card: a caller asking for more than fits
    // paints what is visible instead of panicking.
    let region_width = region_width.min(card.width().saturating_sub(region_x));
    let region_height = region_height.min(card.height().saturating_sub(region_y));
    if region_width == 0 || region_height == 0 {
        return;
    }
    use image::GenericImageView as _;
    let (source_width, source_height) = hero.dimensions();
    if source_width == 0 || source_height == 0 {
        return;
    }
    // Compare `source_width / source_height` with `region_width /
    // region_height` in integers: the cross-multiplied forms fit in u64.
    let (crop_width, crop_height) = if (source_width as u64) * (region_height as u64)
        >= (source_height as u64) * (region_width as u64)
    {
        // Source is relatively wider: full height, cropped width.
        let width = ((source_height as u64 * region_width as u64) / region_height as u64).max(1);
        (width.min(source_width as u64) as u32, source_height)
    } else {
        // Source is relatively taller: full width, cropped height.
        let height = ((source_width as u64 * region_height as u64) / region_width as u64).max(1);
        (source_width, height.min(source_height as u64) as u32)
    };
    let crop_x = (source_width - crop_width) / 2;
    let crop_y = (source_height - crop_height) / 2;
    let fitted = hero
        .crop_imm(crop_x, crop_y, crop_width, crop_height)
        .resize_exact(
            region_width,
            region_height,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();
    for (x, y, pixel) in fitted.enumerate_pixels() {
        let dst = card.get_pixel_mut(region_x + x, region_y + y);
        let alpha = pixel[3] as f32 / 255.0;
        for channel in 0..3 {
            let blended = pixel[channel] as f32 * alpha + dst[channel] as f32 * (1.0 - alpha);
            dst[channel] = (blended + 0.5) as u8;
        }
        dst[3] = 255;
    }
}

/// Encode an opaque card as a deterministic PNG.
///
/// RGBA composition is flattened to RGB8 (the card is fully opaque, so no
/// information is lost) and compression/filter are pinned: the settings
/// are part of the artifact's identity, exactly like the WebP/AVIF encoder
/// settings in [`crate::images`].
fn encode_png(card: &RgbaImage) -> Result<Vec<u8>, BuildError> {
    use image::ImageEncoder as _;
    // Flatten RGBA to RGB8 (the card is fully opaque, so nothing is lost).
    let mut rgb = image::RgbImage::new(card.width(), card.height());
    for (dst, src) in rgb.pixels_mut().zip(card.pixels()) {
        dst.0 = [src[0], src[1], src[2]];
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| BuildError::Model {
        message: format!("could not encode social image PNG: {e}"),
    })?;
    Ok(bytes)
}

/// The bundled fonts, parsed once per call (parsing is cheap next to
/// rasterization, and per-call use keeps the generator free of global
/// state).
fn regular_font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_REGULAR).expect("bundled Roboto Regular subset parses")
}

fn bold_font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BOLD).expect("bundled Roboto Bold subset parses")
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_core::{
        ArtifactKind, ArtifactSpec, CollectionId, ContentId, Route, SiteModelBuilder, Slug,
        SourceRef,
    };

    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        use image::ImageEncoder as _;
        let mut image = image::RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8];
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .expect("fixture encodes");
        bytes
    }

    fn card_of(title: &str, description: Option<&str>, hero: Option<(u32, u32)>) -> RgbaImage {
        let hero =
            hero.map(|(w, h)| image::load_from_memory(&gradient_png(w, h)).expect("decodes"));
        compose(
            1200,
            630,
            &Card {
                site: "Signal",
                title,
                description,
                author: Some("Jeroen"),
            },
            hero.as_ref(),
        )
    }

    #[test]
    fn compose_is_deterministic_and_card_sized() {
        let first = card_of("A deterministic title", Some("And a description."), None);
        let second = card_of("A deterministic title", Some("And a description."), None);
        assert_eq!(first.dimensions(), (1200, 630));
        assert_eq!(first, second);
        // Every pixel is opaque: RGB8 encoding loses nothing.
        assert!(first.pixels().all(|pixel| pixel[3] == 255));
        // The accent bar is the first column.
        assert_eq!(
            *first.get_pixel(0, 300),
            Rgba([ACCENT[0], ACCENT[1], ACCENT[2], 255])
        );
    }

    #[test]
    fn encode_is_deterministic_and_valid_png() {
        let card = card_of("Title", None, None);
        let first = encode_png(&card).expect("encodes");
        let second = encode_png(&card).expect("encodes");
        assert_eq!(first, second);
        // PNG magic, and the encoded bytes decode back to card dimensions.
        assert_eq!(&first[..8], b"\x89PNG\r\n\x1a\n");
        let back = image::load_from_memory(&first).expect("decodes");
        assert_eq!(image::GenericImageView::dimensions(&back), (1200, 630));
    }

    #[test]
    fn wrap_is_greedy_deterministic_and_ellipsizes_overflow() {
        let font = regular_font();
        let scaled = font.as_scaled(PxScale::from(DESCRIPTION_SIZE));
        // Narrow column: several lines, all within the width.
        let lines = wrap_text(&scaled, "one two three four five six seven", 120.0, 10);
        assert!(lines.len() > 1, "expected wrapping: {lines:?}");
        assert!(lines.iter().all(|line| measure(&scaled, line) <= 120.0));
        assert_eq!(lines.join(" "), "one two three four five six seven");
        // Same call, same lines.
        assert_eq!(
            lines,
            wrap_text(&scaled, "one two three four five six seven", 120.0, 10)
        );
        // Caps and ellipsizes rather than dropping words silently.
        let capped = wrap_text(&scaled, "one two three four five six seven", 120.0, 2);
        assert_eq!(capped.len(), 2);
        assert!(capped[1].ends_with(ELLIPSIS), "got {capped:?}");
        assert!(measure(&scaled, &capped[1]) <= 120.0);
        // A single oversized word hard-breaks by character.
        let broken = wrap_text(&scaled, "aaaaaaaaaaaaaaaaaaaaaaaaaaaa", 60.0, 10);
        assert!(broken.len() > 1, "expected hard break: {broken:?}");
        assert!(broken.iter().all(|line| measure(&scaled, line) <= 60.0));
        // Empty and whitespace-only input produce no lines.
        assert!(wrap_text(&scaled, "   ", 120.0, 3).is_empty());
    }

    #[test]
    fn layout_stacks_identity_title_and_description() {
        let regular = regular_font();
        let bold = bold_font();
        let metrics = Metrics::scaled(signal_core::DEFAULT_SOCIAL_HEIGHT);
        let identity = regular.as_scaled(PxScale::from(metrics.identity_size));
        let title = bold.as_scaled(PxScale::from(metrics.title_size));
        let description = regular.as_scaled(PxScale::from(metrics.description_size));
        let layout = layout_text(
            &identity,
            &title,
            &description,
            &Card {
                site: "Signal",
                title: "Short",
                description: Some("Sub"),
                author: Some("A"),
            },
            &metrics,
            800.0,
        );
        assert_eq!(layout.identity, "SIGNAL");
        assert_eq!(layout.title, vec!["Short".to_string()]);
        assert_eq!(layout.description, vec!["Sub".to_string()]);
        assert_eq!(
            layout.block_height,
            IDENTITY_LINE_HEIGHT
                + GAP_AFTER_IDENTITY
                + TITLE_LINE_HEIGHT
                + GAP_BEFORE_DESCRIPTION
                + DESCRIPTION_LINE_HEIGHT
        );
        // No description: the gap and lines disappear from the stack.
        let bare = layout_text(
            &identity,
            &title,
            &description,
            &Card {
                site: "Signal",
                title: "Short",
                description: None,
                author: None,
            },
            &metrics,
            800.0,
        );
        assert!(bare.description.is_empty());
        assert_eq!(
            bare.block_height,
            IDENTITY_LINE_HEIGHT + GAP_AFTER_IDENTITY + TITLE_LINE_HEIGHT
        );
        // A long site title stays on one ellipsized line.
        let long_site = layout_text(
            &identity,
            &title,
            &description,
            &Card {
                site: "An extremely long site title that cannot fit in a narrow column at all",
                title: "Short",
                description: None,
                author: None,
            },
            &metrics,
            120.0,
        );
        assert!(
            long_site.identity.ends_with(ELLIPSIS),
            "{}",
            long_site.identity
        );
        assert!(measure(&identity, &long_site.identity) <= 120.0);
    }

    #[test]
    fn metrics_scale_the_whole_composition() {
        // The reference size is the identity; other heights scale every
        // metric, so the layout ratio (and therefore the fit) is preserved.
        let base = Metrics::scaled(signal_core::DEFAULT_SOCIAL_HEIGHT);
        assert_eq!(base.padding, PADDING);
        assert_eq!(base.accent_bar, ACCENT_BAR_WIDTH as u32);
        assert_eq!(base.title_size, TITLE_SIZE);
        let half = Metrics::scaled(signal_core::DEFAULT_SOCIAL_HEIGHT / 2);
        assert_eq!(half.padding, PADDING / 2.0);
        assert_eq!(half.accent_bar, (ACCENT_BAR_WIDTH / 2.0).round() as u32);
        assert_eq!(half.title_size, TITLE_SIZE / 2.0);
        // Even a degenerate height keeps a visible accent bar.
        assert_eq!(Metrics::scaled(1).accent_bar, 1);
    }

    #[test]
    fn hero_cover_crops_centered_and_fills_the_region() {
        // A wide hero into a tall region: full height, centred width crop.
        let wide = image::load_from_memory(&gradient_png(400, 100)).expect("decodes");
        let mut card = RgbaImage::from_pixel(200, 200, Rgba([0, 0, 0, 255]));
        draw_hero_cover(&mut card, (0, 0, 100, 200), &wide);
        // The region is fully painted (cover, never letterboxed).
        assert!(card
            .enumerate_pixels()
            .filter(|(x, _, _)| *x < 100)
            .all(|(_, _, pixel)| pixel[3] == 255));
        // Outside the region stays untouched.
        assert_eq!(*card.get_pixel(150, 10), Rgba([0, 0, 0, 255]));
        // Deterministic for identical inputs.
        let mut second = RgbaImage::from_pixel(200, 200, Rgba([0, 0, 0, 255]));
        draw_hero_cover(&mut second, (0, 0, 100, 200), &wide);
        assert_eq!(card, second);
    }

    fn site_with(
        root: &Path,
        hero: Option<&str>,
        description: Option<&str>,
    ) -> (SignalConfig, SiteModel) {
        let config = SignalConfig::from_toml_str(
            "[site]\ntitle = \"Signal\"\nbase_url = \"https://example.com/\"\nauthor = \"Site Author\"\n[social]\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        )
        .expect("config parses");
        let mut entry = ContentEntry::new(
            ContentId(1),
            CollectionId::new("posts"),
            SourceRef::new(CollectionId::new("posts"), "a.md"),
            Slug::new("a"),
            Route::new("/posts/a/".to_string()),
            "A title",
        );
        entry.description = description.map(str::to_string);
        entry.image = hero.map(str::to_string);
        let mut builder = SiteModelBuilder::new();
        builder.add_entry(entry);
        let model = builder.build().expect("builds");
        std::fs::create_dir_all(root.join("static/images")).expect("mkdir");
        std::fs::write(root.join("static/images/hero.png"), gradient_png(160, 90)).expect("write");
        (config, model)
    }

    #[test]
    fn resolve_writes_a_card_and_tracks_the_hero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (config, model) = site_with(root, Some("images/hero.png"), Some("Desc"));
        let spec = ArtifactSpec::new("social/posts/a.png", ArtifactKind::SocialImage);
        let bytes = resolve_social_image(root, &spec, &config, &model).expect("resolves");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            resolve_social_image(root, &spec, &config, &model).expect("resolves"),
            bytes
        );

        // A metadata-only card (no hero) differs, and a missing hero
        // reference is a clear error, not a silent fallback.
        let (config, model) = site_with(root, None, Some("Desc"));
        let plain = resolve_social_image(root, &spec, &config, &model).expect("resolves");
        assert_ne!(plain, bytes);
        let (config, model) = site_with(root, Some("images/missing.png"), None);
        let err = resolve_social_image(root, &spec, &config, &model).expect_err("missing hero");
        assert!(err.to_string().contains("missing.png"), "got: {err}");
    }

    #[test]
    fn resolve_rejects_non_social_paths_and_disabled_sites() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (config, model) = site_with(root, None, None);
        let spec = ArtifactSpec::new("social/posts/missing.png", ArtifactKind::SocialImage);
        let err = resolve_social_image(root, &spec, &config, &model).expect_err("unknown route");
        assert!(err.to_string().contains("does not resolve"), "got: {err}");
        let spec = ArtifactSpec::new("posts/a.png", ArtifactKind::SocialImage);
        let err = resolve_social_image(root, &spec, &config, &model).expect_err("not social");
        assert!(err.to_string().contains("does not resolve"), "got: {err}");
        let disabled = SignalConfig::from_toml_str("[site]\ntitle = \"T\"\n").expect("parses");
        let spec = ArtifactSpec::new("social/posts/a.png", ArtifactKind::SocialImage);
        let err = resolve_social_image(root, &spec, &disabled, &model).expect_err("not enabled");
        assert!(err.to_string().contains("[social]"), "got: {err}");
    }

    #[test]
    fn page_metadata_requires_base_url_and_participation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (config, model) = site_with(dir.path(), Some("images/hero.png"), None);
        let entry = model
            .lookup_by_route(&Route::new("/posts/a/"))
            .expect("entry");
        let meta = page_social_metadata(&config, entry).expect("participates");
        assert_eq!(meta.url, "/social/posts/a.png");
        assert_eq!(meta.absolute_url, "https://example.com/social/posts/a.png");
        assert_eq!((meta.width, meta.height), (1200, 630));

        // No base URL: no metadata (the config gate rejects the site
        // before rendering ever runs).
        let mut no_base = config.clone();
        no_base.site.base_url = None;
        assert!(page_social_metadata(&no_base, entry).is_none());
        // Opted-out pages and section roots produce nothing.
        let mut opted_out = entry.clone();
        opted_out
            .extra
            .insert("social_image".to_string(), serde_json::Value::Bool(false));
        assert!(page_social_metadata(&config, &opted_out).is_none());
        let mut section = entry.clone();
        section.section_root = true;
        assert!(page_social_metadata(&config, &section).is_none());
        // Disabled sites produce nothing.
        let disabled =
            SignalConfig::from_toml_str("[site]\ntitle = \"T\"\nbase_url = \"https://e.com/\"\n")
                .expect("parses");
        assert!(page_social_metadata(&disabled, entry).is_none());
    }

    #[test]
    fn hero_cover_handles_missing_and_degenerate_inputs() {
        let hero = image::load_from_memory(&gradient_png(160, 90)).expect("decodes");
        let mut card = RgbaImage::from_pixel(50, 50, Rgba([1, 2, 3, 255]));
        // Zero-sized regions are no-ops, and a region larger than the card
        // is clipped to it rather than panicking.
        draw_hero_cover(&mut card, (0, 0, 0, 10), &hero);
        assert_eq!(*card.get_pixel(49, 49), Rgba([1, 2, 3, 255]));
        draw_hero_cover(&mut card, (0, 0, 60, 10), &hero);
        assert_ne!(*card.get_pixel(10, 5), Rgba([1, 2, 3, 255]));
        // A region starting outside the card paints nothing.
        let untouched = card.clone();
        draw_hero_cover(&mut card, (60, 0, 10, 10), &hero);
        assert_eq!(card, untouched);
    }

    #[test]
    fn compose_survives_degenerate_dimensions() {
        // Cards narrower than the scaled accent bar, one-pixel cards, and
        // absurdly small cards must render (or draw nothing) — never panic.
        let hero = image::load_from_memory(&gradient_png(40, 20)).expect("decodes");
        for (width, height) in [(1, 1), (5, 630), (800, 1), (40, 20), (1600, 900)] {
            let card = compose(
                width,
                height,
                &Card {
                    site: "Signal",
                    title: "Title",
                    description: Some("Description"),
                    author: Some("A"),
                },
                Some(&hero),
            );
            assert_eq!(card.dimensions(), (width, height), "{width}×{height}");
        }
    }
}
