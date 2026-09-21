# ADR 0029: Image derivatives (A2)

## Status

Accepted (A2 design; implements the A1-deferred generated-asset edge).

## Context

A1 (ADR 0028) made source assets first-class build inputs but kept
output identity-based: `images/hero.jpg` in, `images/hero.jpg` out.
The deferred generated-asset edge — `post.md └── hero-1280.webp └──
hero.jpg` — needs a derivative representation, a producer, dependency
tracking, non-identity output paths, dimensions, and manifest reuse,
without disturbing any A1 semantics for sites that request no
derivatives.

## Design questions (answered explicitly)

### Should derivatives be a new artifact kind, or should `Static` be generalized?

A new kind: `ArtifactKind::DerivedImage`. Generalizing `Static` would
blur the one invariant that makes `Static` sound — output bytes equal
source bytes, so the source digest doubles as the output digest.
Derivatives break that invariant by construction (decode → resize →
encode), carry transformation parameters, and resolve through a
producer rather than a copy. The new kind reuses every existing
mechanism unchanged (`SpecPlan` validation, `resolve_artifact`
dispatch, manifest records, `write_artifact`, pruning, `check` /
`explain` agreement); only the per-kind arms are new.

### Where should image transformation happen?

```text
signal-core (pure, no I/O, no encoder):
  DerivativeSpec identity (source, width, format)
  output naming, parameter validation, format-support predicate
  target-dimension math, [images] config surface

signal-cli (filesystem boundary, owns the encoder):
  dimension detection, resizing, WebP encoding
  derivative spec planning, source validation, resolution
```

`signal-core` gains no `image` dependency and no `std::fs` use; the
`image` crate (decode PNG/JPEG, Lanczos3 resize, lossless WebP encode)
lives in `signal-cli` only, behind a small `images` module boundary so
no codec details leak into the planner.

## Decision

- **Request surface:** an opt-in `[images]` table — `widths`
  (e.g. `[640, 1280, 1920]`) and `format = "webp"`. Absent (or empty
  widths) plans nothing: unconfigured sites are byte-identical to A1.
  `format` accepts only `"webp"`; anything else fails in the shared
  config gates with a diagnostic naming the supported set. Widths are
  sorted/deduplicated; `0` fails; there is no other author syntax and no
  Markdown change in A2.
- **Planning is pure:** for every content-referenced raster source
  (extension `png`/`jpg`/`jpeg`, matched against the planned `Static`
  inventory raw-then-decoded) crossed with every configured width, plan
  one `DerivedImage` spec. SVG, GIF, and other assets are never
  rasterized — they stay verbatim `Static` outputs. References that
  match no source are skipped here and fail later in reference
  validation, as in A1.
- **Identity and naming:** `DerivativeSpec { source, width, format }`;
  output `{stem}-{width}.webp` beside the source (`hero.jpg` →
  `hero-640.webp`). Same source + same parameters always yield the same
  path; different widths/formats yield different paths. Cross-source
  collisions (`a.png` + `a.jpg` → `a-640.webp`, or a committed static
  shadowing a derivative output) fail in the existing logical-duplicate
  validation before anything is written. No content hashing in A2;
  `output_path_for_source` and the single `derivative_output_path`
  function remain the fingerprinting hook.
- **No-upscale clamp:** requested widths at or above the source width
  produce the source dimensions (never fabricated pixels); the output
  path keeps the requested label. Rationale: planning stays pure (plan
  shape never depends on file bytes), resolution stays total, and the
  policy matches Signal's fail-closed honesty — outputs never claim
  detail the source lacks. A future milestone may dedupe or warn on
  clamped duplicates; A2 documents them.
- **Dependencies reuse the A1 lesson:** a new
  `InputRef::DerivedImage { source, width, format }` names the
  derivative edge on both the derivative artifact and the embedding
  pages/sections. Parameters ride the reference itself, so parameter
  changes surface as `InputsChanged`; the byte comparison reuses the
  manifest `assets` source-digest map (raw-then-decoded lookup), so no
  derivative is ever compared against page HTML. A distinct
  `RebuildReason::DerivativeChanged` keeps explanations precise.
- **Reuse inputs:** source digest (via `assets`) + spec parameters (via
  canonical input lists) + whole-config digest is already covered by
  `[images]` living in `SignalConfig`… plus the behavior gate:
  `GENERATION_BEHAVIOR_VERSION 9 → 10`. The encoder has no tuning knob
  in A2 (fixed lossless WebP, fixed Lanczos3); any future encoder or
  dependency change that can alter bytes MUST bump the behavior version
  — recorded as a code comment at the producer boundary.
- **Resolution is a procedural mirror** (`resolve_derived_image`):
  derive the `DerivativeSpec` from the output path with the same
  enumeration planning uses (the `feed_identity` precedent), read the
  source raw-then-decoded, decode (magic-sniffed; content/extension
  mismatches fail clearly), clamp dimensions, resize (skipped when the
  target equals the source dims), lossless-WebP encode.
- **Validation is shared:** config shape in `validate_config_gates`;
  derivative source probing (exists, supported, decodable) in one new
  function called from both `validated_plan` and
  `validated_plan_for_check` — so `build`, `check`, and `explain` agree
  by construction. `check` additionally reports planned derivative
  counts on the existing asset line.
- **Rendering is unchanged in A2:** HTML carries no new URLs (no
  `srcset`/`<picture>` — that is A3). Pages name derivative inputs
  because they will embed them; A2 proves the graph, A3 consumes it.
  `explain <asset> --width W --format webp` (and bare derivative
  output paths) renders Source / Input dims+format / Derivative
  dims+format / Output / Dependencies / Action (`generate`) / Decision
  from the same plan the build uses.

## Consequences

- New dependency: `image` (decode + resize + WebP) in `signal-cli`
  only, default-features off (`png`, `jpeg`, `webp`). No system
  libraries (pure Rust). No AVIF, no cropping, no filters, no remote
  images, no optimization heuristics.
- Unconfigured sites: identical specs, identical bytes, identical
  manifests except the behavior-version gate (one rebuild, as with every
  behavior change).
- Known A2 roughness (documented, not fixed): clamped widths can emit
  byte-identical files under different labels; listings (Home/Taxonomy)
  keep query-only coverage until A3 embeds responsive markup.

## Deferred

A3 (`srcset`/`<picture>`, width/height attributes) consumes
`DerivativeSpec` outputs and page inputs without changing the
artifact/dependency model. AVIF is a second `DerivativeFormat` variant
plus an encoder arm. Cropping/focal points are further spec parameters
(same `InputsChanged` mechanics). OG/social images are a new generated
kind following the `DerivedImage` precedent.
