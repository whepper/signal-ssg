# ADR 0030: Responsive images (A3)

## Status

Accepted (A3: render A2 derivatives as responsive markup; no artifact-model change).

## Context

A2 plans `DerivedImage` artifacts with clamped dimensions (requests at
or above the source width emit source dimensions, never upscaled
pixels) and records `DerivedImage{source,width,format}` inputs on both
derivative artifacts and embedding pages. Rendering still emits A1-era
markup: Comrak `<img src alt>` for body images, site templates reading a
plain `image` URL string for heroes. The A2 assessment concluded no new
artifact/dependency model is needed — A3 consumes the derivative system
in HTML.

Two questions had to be settled first: clamped-width representation in
`srcset`, and where responsive selection lives.

## Decision

### Phase 2: preserve requested derivative identity (Option A)

`hero-1920.webp` generated from a 1600px source keeps its output path
(plan stability: plan shape never depends on file bytes, so
`artifact_inputs` stays pure), but every consumer exposes its **actual**
dimensions (`1600w`), never the requested label. Same-actual-width
candidates dedupe deterministically, keeping the smallest requested
width. Option B (skip oversized derivatives) was rejected: plan shape
would depend on decoded bytes, and `artifact_inputs` — pure by design,
shared with manifest recording — could not know which widths survived.

### Selection lives in core, bytes at the boundary

- `signal-core::asset`: `DerivativeView { output, width (requested),
  actual_width, actual_height }` (replaces the CLI-local
  `PlannedDerivativeView`, same shape), `ResponsiveCandidate`,
  `ResponsiveImage { source, candidates, default_src, width, height,
  sizes, srcset }`, `responsive_image()` (sort by actual width, dedupe
  keeping smallest requested width, default = largest actual width,
  `sizes = "100vw"`), `DEFAULT_SIZES`. URL encoding reuses
  `image_src_url`, so `srcset` strings are presentation-ready.
- `signal-cli::images::responsive_for_source()`: decode once, build
  views, call the pure selector. Shared by body rewriting, hero
  contexts, and `explain` — one selection implementation.
- No new `[images]` keys: `sizes` defaults to `"100vw"` with no
  override in A3 (a layout DSL is explicitly out of scope).

### Rendering integration

- **Body images** (Markdown): a focused tag rewriter
  (`signal-cli::responsive`) upgrades Comrak `<img src alt>` tags
  whose `src` resolves to a derivable source with planned derivatives
  to `<img src srcset sizes width height alt>`. External,
  unresolvable, non-raster, and unconfigured cases leave the tag
  byte-identical (A1 behavior preserved exactly). Applied in
  `entry_context` and `section_context` (section-root bodies) — the two
  contexts whose artifacts already name the consumed derivative inputs,
  so no input change is required. Over-coverage is impossible in
  practice: derivative bytes are a pure function of already-declared
  inputs (source bytes, parameters, config).
- **Hero images** (front matter): existing `image`/`image_alt` keys are
  untouched (template compatibility); a new `responsive_image` key
  carries `{ src, srcset, sizes, width, height, alt, candidates }` when
  the hero has planned derivatives, absent otherwise (templates gate
  with `{% if responsive_image %}`). The homepage extends only its
  selected featured summary with `featured.responsive_image`, resolved
  from that full entry through the same `responsive_hero` path. Its
  `Home` artifact then names the selected entry, hero source, and hero
  derivatives that can appear in the rendered representation. Other
  listing, feed, related, and taxonomy summaries render no responsive
  metadata and keep query-only coverage, so dependencies reflect actual
  byte consumption.
- **No `<picture>` in A3**: with only WebP derivatives, a responsive
  `<img>` is sufficient; introducing `<picture>` now would be ceremony
  for one format. The `candidates` list on the hero context is the
  extension point A4 (`<source type=…>`) builds on.

### Reuse and versioning

Page inputs are unchanged, yet configured-site page bytes change (new
markup) — the textbook behavior-version case, so
`GENERATION_BEHAVIOR_VERSION 11` (one gated rebuild; unconfigured sites
rebuild once with byte-identical output). Derivative artifacts reuse
uninterrupted across the upgrade.

### Implementation correction: responsive homepage featured heroes

The original A3 rule intentionally left every listing query-only. A later
compatibility exercise showed that the one listing image Signal actually
projects as a full hero is the homepage's selected featured entry. The
homepage therefore extends that one `featured` object with the existing
`ResponsiveHero` value and declares the exact source/derivative inputs it
renders. This is a correction to the placement rule, not a second image
pipeline: entry and homepage heroes both call `responsive_hero`, share the
same format ordering, fallback selection, dimensions, and alt behavior, and
omit the key for external, non-raster, unconfigured, and absent heroes.
Behavior version `15` covers the new context shape and dependency semantics.

### Validation and explanation

No new validator: reference validation, derivative source probing, and
output-collision checks already guarantee every `srcset` candidate
resolves, exists, is unique-width, and carries actual dimensions — by
construction, on all paths. `check` output is unchanged in shape;
`explain <derivative>` gains a Responsive section (candidates, default,
sizes) rendered from the same selection the rewrite uses.

## Consequences

- `signal check`/`explain`/build agree on responsive output because all
  three share enumeration, selection, and probing.
- Unconfigured sites: specs, bytes, and manifests identical to A2
  (pinned by the untouched golden suite plus an explicit A2==A3 test).
- Known non-goals preserved: AVIF, cropping, art direction, OG images,
  `loading=`/`decoding=` attributes (deferred; no partial hints).

## Deferred

A4 alternate formats: second `DerivativeFormat` variant; hero
`candidates` group by format into `<source>` elements; body rewriter
gains `<picture>` emission. A5 art direction: further spec parameters,
same `InputsChanged` mechanics. A6 social images: new generated kind on
the `DerivedImage` precedent.
