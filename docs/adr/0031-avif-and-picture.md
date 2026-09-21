# ADR 0031: AVIF derivatives and `<picture>` sources (A4)

## Status

Accepted (A4: second derivative format plus format-grouped responsive
markup; no artifact/dependency-model change).

## Context

A3 renders each source's planned WebP derivatives as a responsive `<img>`
(`srcset` of actual widths, `sizes="100vw"`, intrinsic dimensions) via a
shared pure selector (`signal-core::responsive_image`) consumed by body
rewriting, hero contexts, and `explain`. A3 deferred alternate formats to
A4, predicting no new artifact model would be needed. A4 tests that
prediction: one source must yield two formats, exposed as `<picture>`
with per-format `<source>` elements and a valid `<img>` fallback.

## Decision

### Format as a closed enum variant (Phase 2)

`DerivativeFormat::Avif` joins `::WebP`. Derivative identity was already
the triple `(source, width, format)` (A2), so `hero.jpg + 640 + webp ≠
hero.jpg + 640 + avif` falls out of the existing model: distinct output
paths (`hero-640.webp` vs `hero-640.avif`), distinct artifacts, distinct
inputs, manifest reuse per triple. No second derivative model, no
format-specific dependency path, no codec registry.

### Configuration: `formats` beside `format` (Phases 2, 3, 18)

```toml
[images]
widths = [640, 1280, 1920]
formats = ["avif", "webp"]
```

- Legacy `format = "webp"` (and bare `[images] widths = […]`, which
  defaults to WebP) behaves exactly as in A2/A3: one format, plain
  responsive `<img>`, no new outputs. A4 never enlarges an existing
  site's build unless the author opts in.
- `formats`, when non-empty, wins over `format`; setting both is a
  configuration error (ambiguous intent fails clearly). `formats = []`
  behaves as absent. Unknown names fail in the shared gates;
  duplicates are deterministically deduplicated; author order never
  affects output (effective names sort alphabetically; planning and
  rendering order canonically AVIF-first).

### Encoder: ravif, fixed settings, no system libraries (Phase 4)

`signal-cli::images` gains AVIF behind the existing boundary; the
planner still reasons in `DerivativeSpec` terms, and no encoder types
leak into `signal-core`.

- **Crate**: `ravif 0.13` (BSD-3-Clause; powers the `cavif` tool) with
  `default-features = false`, plus `rgb`/`imgref` buffer types. The
  default features (`asm`, `threading`) are deliberately off: `asm`
  would require nasm at build time, and `threading` (rayon/rav1e
  multithreading) would make bytes a function of machine thread count.
  Without them rav1e is pure Rust, single-threaded, and needs no system
  libraries — the same install story as the rest of Signal.
- **Fixed identity**: quality 70, speed 10 (ravif scales 1–100 and
  1–10). No per-format configuration surface: these are build
  constants, and any change rides `GENERATION_BEHAVIOR_VERSION` like
  every other byte-affecting encoder change.
- **Cost, stated plainly**: single-threaded rav1e is slow (order of
  seconds per photographic derivative at 1600px scale, measured during
  development). Unchanged sources never re-encode (manifest reuse), and
  AVIF is opt-in — WebP-only builds pay nothing. Speed 10 (fastest) was
  chosen to keep site builds practical; the compression tradeoff is
  fixed and documented, not tuned per site.

### Determinism (Phase 5)

Same inputs (source bytes, dimensions, fixed settings) produce
byte-identical AVIF: pinned by an encode-twice unit test and an
independent-builds integration test (`.avif` bytes, HTML, manifests all
compared). Single-threading removes the thread-count variable; no
timestamps or nondeterministic headers were observed in the ravif
output.

### Responsive representation: groups, not parallel types (Phase 8)

`DerivativeView` and `ResponsiveCandidate` each gain a `format` field;
`ResponsiveImage` gains `sources: Vec<ResponsiveSource>` (one
`{format, mime, srcset, candidates}` group per planned format, ordered
AVIF-first) while the flat `candidates`/`srcset`/`default_src` fields
now describe the **fallback** group. The pure selector runs its existing
per-group dedupe (actual widths, smallest requested label wins) and
stays shared by rendering, heroes, and `explain`.

### `<picture>` only for multiple formats; WebP fallback (Phases 9–11)

- One planned format → the A3 `<img>`, byte-shape unchanged. Two →
  `<picture><source type …/>…<img …/></picture>`: one `<source>` per
  group in AVIF-first order (browsers take the first supported source,
  so AVIF wins where supported), then the fallback `<img>`.
- **Fallback decision (Phase 10): Option A, WebP fallback.** The
  fallback is always a generated derivative (never the original
  source): it stays responsive (`srcset`/`sizes`/intrinsic dimensions),
  stays inside the derived-artifact dependency closure, keeps
  near-universal browser support, and leaves the original asset
  reachable at its stable path. Concretely: the WebP group when
  planned, else the sole group (AVIF-only plans render a plain AVIF
  `<img>`). `alt` and carried-over attributes live on the inner `<img>`
  only — `<source>` elements must not carry them.

### Heroes share the representation (Phase 12)

`ResponsiveHero` gains `sources` (the core groups, serialized as-is)
and `has_picture`; templates branch on the flag and loop the groups.
No selection logic duplicated between body images and heroes.

### Validation and explanation reuse (Phases 16–17)

No AVIF-specific diagnostics: reference validation, source probing,
output-collision checks, and config gates already cover formats (each
new check is a loop over the format set, not a subsystem). `explain
<derivative>` renders the shared selection grouped by format
(`fallback:`, `sizes:`, then one section per group in `<source>`
order). `check` output shape is unchanged.

### Reuse across the upgrade

Behavior 11 → 12 (encoder identity, format-set config, rendering
shape). WebP bytes are untouched by the encoder change, but the gate
rebuilds once; derivative artifacts remain addressable per triple, so
removing a format prunes exactly its artifacts while the surviving
format reuses.

## Consequences

- `check`/`explain`/build agree on multi-format output because all
  three share enumeration, selection, and probing.
- Unconfigured and WebP-only sites render exactly A3 markup (pinned by
  dedicated backward-compatibility tests plus the untouched golden
  suite apart from the behavior-version line).
- Known cost: AVIF encode time (see above); known non-goals preserved
  (cropping, art direction, OG images, quality sliders, heuristics).

## Deferred

A5 art direction (further spec parameters, same `InputsChanged`
mechanics); A6 social images (new generated kind on the `DerivedImage`
precedent); A7 diagnostics (oversized/inefficient/unused assets — the
grouped views are the natural input).
