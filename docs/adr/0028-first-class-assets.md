# ADR 0028: First-class assets (A1)

## Status

Accepted (A1: asset model and pipeline integration; no transformations).

## Context

Signal treated static files as opaque planned outputs: `static/` was
enumerated into `ArtifactKind::Static` specs (ADR 0014), copied verbatim
through `resolve_artifact`, tracked as `InputRef::Static`, and validated
as link targets by `link_check`. But no edge connected content to the
assets it embeds:

1. A page embedding `hero.jpg` named no input for it, so editing the
   asset left embedding pages "reused" — correct bytes today (HTML
   carries only the `src`), but the wrong dependency for every future
   step that embeds asset facts (dimensions, `srcset`, fingerprints).
2. There was no shared vocabulary for *source asset* vs *asset reference*
   vs *output asset*, and no inventory answering "what references this
   asset?" for authors and operators.
3. `check` reported references but no asset counts; `--explain` reported
   artifact decisions but no per-asset record.

A2 (image derivatives), A3 (responsive rendering), A4 (generated social
images), and A5 (asset diagnostics) all need the `post.md └── hero.jpg`
edge first. The question was where to put it without a parallel build
model.

> Roadmap note (added later): the A-series shifted as work landed — A4
> became AVIF plus `<picture>` (ADR 0031) and A5 became generated social
> images (ADR 0032); asset diagnostics remain future work. The
> `Asset` vocabulary type described below was removed in the A1–A5
> architecture review: it had no consumer, and its `width`/`height`
> fields were reserved for a derivative model that shipped as
> `DerivativeSpec` instead.

## Decision

Extend the existing `Static` abstraction; introduce no parallel one:

- **Source asset** is a file under `static/` (the only input boundary;
  symlinks skipped, fail-closed discovery unchanged). **Asset reference**
  is an authored string (`body.images`, front-matter `image`) resolving
  to a source path. **Output asset** is the planned `Static` spec plus
  its bytes, written through `write_artifact`. **Generated asset**
  (resized/encoded derivatives) is reserved vocabulary only — no kind,
  no output, no processing in A1.
- **Reference resolution is pure and shared** (`signal-core::asset`):
  front-matter heroes are literal site-root paths; Markdown images split
  query/fragment, resolve document-relative against the containing
  route, and fail on root escape — the exact semantics `link_check`
  validates with. Filesystem matching tries the raw form, then the
  percent-decoded form, at both validation and digest time.
- **Dependencies reuse `InputRef::Static`** (`signal-cli::build_plan`):
  a `Page` names its entry's referenced assets; a `CollectionIndex`
  rendering a section-root body names that entry's assets. Listings keep
  query-only coverage (their summaries already carry the image reference
  string inside the query digest; no A1 bytes embed asset facts).
- **Digests gain an `assets` map** (`Manifest.assets: path → digest`):
  the reuse predicate compares `Static` inputs against it — never
  against a referencing page's HTML digest. `Static` artifact records
  keep working because their output digest IS the source digest.
- **MIME is a pure extension table** (`mime_for_path`); image dimensions
  are reserved `None` fields on `Asset`, not detected values.
- **`check` gains pure asset counts** (discovered / referenced /
  resolved / missing / unsafe) from specs plus model — no filesystem
  reads, same gates as the build. **`explain` gains a per-asset record**
  (source, type, byte size, referrers, output, copy action, reuse/rebuild
  decision) via a new `signal explain [target]` command; bare
  `signal explain` prints the whole plan exactly like `build --explain`.
- **Behavior version `8` → `9`**: page/section records change shape
  (new inputs) and reuse semantics (asset bytes invalidate embedders),
  so previous manifests rebuild once by gate, never by miscomparison.

## Consequences

- Editing `static/images/hero.jpg` rebuilds the asset plus exactly the
  pages embedding it (`static file changed: images/hero.jpg`), verified
  by plan, build, and golden tests. Output bytes for unchanged inputs
  are identical (golden tree unchanged; frozen manifest regenerated for
  the new record shape only).
- Missing/escaping asset references keep failing closed through
  `link_check` before any write, identically in `build`, `check`,
  `explain`, and the new `signal explain <asset>`.
- No new config surface, no new artifact kind, no new copy path, no new
  dependencies, no image processing of any kind.

## Deferred (explicit A2+ hooks)

- A2 derivatives: new output artifacts plus further `Static`-family (or
  successor) inputs on embedding pages; `output_path_for_source` is the
  single function fingerprinted paths extend; `Asset.width/height` fill
  in from detection.
- A3 responsive rendering: pages/listings embedding `srcset`/`sizes`
  extend the same input edges; no planner redesign.
- A4 generated social images: a generated-asset output derived from a
  source asset plus page metadata, recorded like any artifact.
- A5 diagnostics (oversized, unused, missing alt/dimensions, cache
  accounting) consume `AssetReport` / `asset_referrers` / manifest
  `assets` without new discovery.
