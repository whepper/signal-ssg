# ADR 0032: Generated social images (A5)

## Status

Accepted (A5: page metadata plus an optional hero image → one
deterministic PNG card per participating page, referenced from Open
Graph/Twitter metadata; no artifact/dependency-model change).

## Context

A1–A4 built a generated-artifact pipeline: first-class assets, resized
WebP/AVIF derivatives, responsive `srcset`/`<picture>` markup, per-triple
manifest reuse, and `check`/`explain` agreement over one plan. A5 adds a
new *kind* of generated artifact — one whose inputs are page metadata and
an optional source image, not a transformation of a static file:

```text
Static        source bytes → output bytes (verbatim)
DerivedImage  source image + parameters → resized/encoded image (A2)
SocialImage   page metadata + optional hero → generated PNG card (A5)
```

The motivating case is a blog article that should share as a proper card
(`og:image` / `twitter:image`) rather than a raw hero crop.

## Decision

### Artifact kind and identity

`ArtifactKind::SocialImage`. Output path is `social/<route-path>.png`
(`/posts/example/` → `social/posts/example.png`, `/` →
`social/index.png`), computed by `signal_core::social_image_path` with the
same raw-segment policy as `route_to_output_path`. Routes are unique, so
paths are unique; a route that would collide (`/index/` against `/`) is
rejected by the existing output-collision validation before anything is
written.

The spec deliberately carries **no** route: a page already claims its
route, and route validation requires route uniqueness per artifact.
Identity is inverted from the output path
(`signal_core::social_image_route`), exactly as `DerivedImage` inverts its
own — one inversion shared by planning, input derivation, resolution, and
`explain`, so all four agree on which page a card belongs to.

### Inputs: entry, configuration, hero source bytes

`artifact_inputs` declares:

- `InputRef::Entry { route }` — title, description, author, hero
  reference, and the per-page override (all inside `EntryDigestInput`);
- `InputRef::Config` — `[social]` dimensions and site identity
  (`site.title`, `site.author`), which is why a dimension change invalidates
  the card;
- `InputRef::Static { path }` — the hero source bytes, only when one is
  composited.

No new `InputRef` variant was needed: the social card reuses the existing
input vocabulary, and the page's HTML digest is never an input.

**No page → card edge.** A page embeds only the card's *URL*, which is a
pure function of the page's already-declared inputs (route + config).
Derivative bytes are equally pure functions, yet A2 added page → derivative
edges for over-coverage; A5 does not, because the card's URL cannot change
without either the entry digest or the config digest changing, both of which
already invalidate the page. This is the "dependencies reflect actual byte
consumption" rule, applied strictly; a test pins it (a page's recorded
inputs never name a `social/` path).

### Generator: pure-Rust text rasterization, bundled font

`signal-cli::social` is the pixel boundary (the `signal-cli::images`
precedent); `signal-core::social` holds pure identity, naming,
participation, and URL forms.

- **Rasterization**: `ab_glyph` (Apache-2.0, pure Rust, no system
  libraries) draws glyphs into an `image::RgbaImage`. Text advances are
  per-glyph `h_advance` (no kerning/shaping), coordinates are pure float
  math, and alpha compositing rounds half-up — deterministic.
- **Font**: a Latin subset of Roboto Regular + Bold, compiled in with
  `include_bytes!` (`crates/signal-cli/assets/fonts/`, Apache-2.0, with the
  upstream licence and the exact `pyftsubset` command recorded). Rendering
  never consults system fonts, so a card cannot change because the build
  machine changed. Glyphs outside the subset render as the missing-glyph
  box; no font autoselection (a documented limitation, not silent
  truncation).
- **Hero fit**: cover, documented, integer-only. Compare source and region
  aspect ratios by cross-multiplication, take the largest centred source
  rectangle matching the region's ratio, then Lanczos3-resize it to the
  region (the A2 filter). No focal points, no smart cropping.
- **Encoding**: RGB8 PNG with pinned compression (`Default`) and filter
  (`Adaptive`). RGBA composition is flattened to RGB8 because the card is
  fully opaque, so nothing is lost and no transparency semantics leak.

### Layout: a stable design, not a DSL

One fixed design, no per-element configuration: dark background, accent
bar, site title (uppercase, accent), bold title (up to 4 lines), optional
description (up to 3 lines), optional byline pinned bottom-left, optional
hero occupying the right ~42%. Constants live in `signal-cli::social`;
`Metrics::scaled` scales them all by the configured height, so any allowed
dimension is a faithful scaling of the reference 1200 × 630 composition
rather than a clipped one. Greedy word wrap normalizes whitespace,
hard-breaks oversized words by character, and ellipsizes overflow on the
final capped line — including the identity line, which is capped at one
line so a long site title can never run into the hero region. All of it is
directly unit-tested, and degenerate dimensions are tested for
non-panicking behavior.

### Configuration

```toml
[social]
width = 1200   # optional, default 1200
height = 630   # optional, default 630
```

Presence of `[social]` is the opt-in; `enabled = false` keeps the table but
plans nothing. Dimensions must be within 1‥=4096 (an allocation hazard is
not a feature). Social images need publicly resolvable URLs, so an enabled
`[social]` without `site.base_url` fails the build and `check` identically
— the feeds precedent — instead of emitting a relative `og:image`. Pages
opt out (or in) with front-matter `social_image: true|false`, which lives in
`extra` like every other site-specific key.

Participation: enabled site, real entry page (never a section-root
listing), no opt-out. Nothing else — no content-type or hero heuristic.

### Metadata

Rendering, not the generator, references the card. `entry_context` inserts:

- `og_image` → the card's absolute URL (superseding the hero: the card is
  purpose-built for sharing);
- `twitter_card` = `summary_large_image`, `twitter_title`,
  `twitter_description`, `twitter_image`;
- `social_image` = `{ url, absolute_url, width, height }` for templates
  that want `og:image:width`/`height`.

`image_absolute_url` and `json_ld` deliberately keep the article's own hero:
structured data describes the article's image, the share preview describes
the card. Disabled sites and non-participating pages keep exactly A4
metadata.

### Reuse and versioning

Behavior 12 → 13: `[social]` joins the configuration digest and the
`SocialImage` generator (font, layout, encoder) rides the behavior gate,
which is the card's reuse identity — so a future design change invalidates
cards without a new manifest field. Removing the feature prunes its cards
(empty directories are left, as with every prune) and restores A4 metadata.

### Check and explain

No new validator: dimension and `base_url` gates join the shared config
gates; hero existence/decodability is probed in `validate_social_sources`
on both the build and check paths; output collisions are the existing
checks. `check` gains a plan-derived card count. `explain
social/<path>.png` renders the card's page, dimensions, inputs, output,
and state — `reuse`/`rebuild: reason`, or `disabled`, `opted out`, `section
roots are listings` — from the same plan the build executes.

## Consequences

- `check`/`explain`/build agree on cards because all three share
  enumeration, inversion, probing, and planning.
- Unconfigured sites and opted-out pages produce byte-identical A4 output
  (pinned by tests); the one-time rebuild comes from the behavior gate.
- Known limitation: a Latin-only bundled font (no script fallback) and
  seconds-per-build PNG encoding for large sites (cached by reuse).

## Deferred

A6 alternate social dimensions/themes (`SocialConfig` grows named variants,
same spec/inputs mechanics); recursive derivative processing of the
generated card (explicitly not built — the card is a terminal artifact);
`og:image:alt` and other metadata refinements beyond the documented keys.
