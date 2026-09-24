# ADR 0033: Asset and image diagnostics (A6)

## Status

Accepted (A6: one pure analysis over the publishing model, rendered by
`check` and `explain`; no artifact, input, manifest, or dependency-model
change).

## Context

A1–A5 made Signal's asset and publishing model complete enough that
`check` and `explain` already hold most of the facts needed to describe a
site: the plan enumerates every artifact, `AssetReport` counts discovered,
referenced, resolved, unreferenced, derivative, and social artifacts, and
`explain <asset>` measures size, digest, dimensions, derivatives, and
referrers. A5.1 confirmed the architecture is coherent and identified
diagnostics as the natural next step.

The risk is obvious: a diagnostics feature is exactly the kind of thing
that grows a second publishing system — rule registries, configurable
thresholds, per-check suppression, a `Diagnostic` artifact kind, or a new
manifest table. A6 must not do that.

## Decision

### Diagnostics are an analysis, not artifacts

`signal-cli::diagnostics::analyze` is a pure function over values the
pipeline already has — planned specs, the frozen model, the
configuration, and image *headers* (dimensions) for sources with planned
derivatives. It returns `Vec<Diagnostic>`.

- No `ArtifactKind::Diagnostic`. Diagnostics are never planned, resolved,
  written, or pruned.
- No manifest field. Computing diagnostics cannot change reuse: the
  manifest schema is untouched, and a build after a `check` still reuses
  every artifact.
- No `InputRef` variant and no dependency edge. Diagnostics read the
  graph; they are not in it.

`check` and `explain` both call `analyze`; `explain` filters the same
result by subject. There is one condition per diagnostic, in one place.

### A closed set, with documented conditions

The model is a closed enum of five variants, not a plugin framework:

| Variant | Condition | Severity |
|---|---|---|
| `UnreferencedAsset` | a planned `Static` spec whose path is a derivable raster source and which no entry references | info |
| `OversizedSource` | a source with planned derivatives whose decoded width is ≥ 3× the largest actual candidate width | warning |
| `RedundantDerivativeWidth` | ≥ 2 configured widths clamp to one actual width for a source, producing duplicate artifacts | info |
| `HeroAltMissing` | `entry.image` is set and `entry.image_alt` is absent or blank | warning |
| `ImageAltEmpty` | a rendered body `<img>` has a blank `alt` | info |

Each carries a stable `code`, a `subject` (asset path or route), and a
`message` that states a measured fact rather than advice.

### Two severities, no `error`

`warning` and `info` only. Hard failures remain `BuildError` and are
unchanged — a missing or unsafe reference, a malformed image, an output
collision, or an invalid configuration fails before diagnostics are
computed, so there is nothing left to report as an error. Optimization
observations never fail a build.

### False-positive policy

A diagnostic is emitted only where the model can prove the condition:

- **Unreferenced** is limited to derivable raster sources (`png`/`jpg`/`jpeg`/`webp`).
  CSS, JS, SVG, and favicons are commonly referenced from templates, which
  the model does not track, so reporting them would be a false positive.
- **Oversized** requires a configured `[images]` pipeline. With no
  derivatives there is no publishing requirement, so Signal must not
  invent a maximum display width. The 3× factor tolerates 2×-DPR source
  material.
- **Redundant** is informational: configuration is user intent.
- **Alt**: Signal's Markdown model cannot distinguish an omitted alt from
  an explicitly empty one (Comrak renders both `alt=""`), so a blank body
  alt is informational — an empty alt is the correct decorative marker.
  The hero is the case the model *can* distinguish (`image_alt` present or
  absent), so its absence is a warning.

### Determinism

Diagnostics sort by `(severity, subject, code)`. Every input is a
`BTreeMap`/`BTreeSet` or a sorted walk; no hash iteration, filesystem
order, time, or environment.

## Consequences

- `check` gains a diagnostics summary and detail lines; `explain` gains a
  `Diagnostics:` section for the explained subject, omitted entirely when
  there is nothing to report, so a clean asset's record is byte-identical
  to its pre-A6 form.
- Diagnostics change no output bytes, no manifest, and no reuse decision.
- `analyze` is infallible and best-effort: a source that cannot be read or
  probed is skipped, because validation already reported real failures.

## Deferred

Byte-level derivative efficiency ("derivative is larger than its source",
"one representation is larger than another at the same dimensions") needs
*generated* output, which `check` does not have. Computing it would either
make `check` encode images (expensive, and a different kind of work than
validation) or read a possibly-stale output tree, breaking the
`check`/`explain` agreement rule. It is deliberately not implemented.

Also deferred: hero-alt diagnostics gated on template usage (Signal cannot
know whether a template renders the hero); a diagnostics flag on `build`
(`build` is not a diagnostic surface — it presents the plan, not advice);
and any user-configurable thresholds or suppression.
