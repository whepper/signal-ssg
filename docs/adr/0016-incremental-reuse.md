# ADR 0016: Manifest consumption and incremental artifact reuse

## Status

Accepted (slice 12B: incremental reuse). Stale pruning followed in slice 12C
(0017); generation compatibility in 14B (0019); template-set invalidation in
14C (0020).

## Context

Slice 12A records a complete deterministic manifest; slice 11 made every
artifact independently resolvable. This slice consumes the manifest to
skip unchanged artifacts. The design constraint from the review: no
mutable model, no persistent DAG, propagation implicit through digests.

## Decision

- Reuse is a total per-artifact predicate over (spec, current inputs,
  previous record, existing output): record present under the stable
  path, kind equal, canonical input lists equal, every current input
  digest equal to its recorded digest, existing output file hashing to
  the recorded output digest. Any doubt rebuilds; the predicate never
  errors.
- Previous manifests are untrusted: absent, unreadable, corrupt, or
  wrong-schema manifests fall back to full builds. Manifest paths are
  lookup keys only — current spec paths drive all reads and writes, so
  corruption cannot redirect output (containment from slice 11 holds).
- Reused artifacts skip resolve and write; their recorded output digests
  carry forward. The new manifest is therefore byte-identical to a clean
  build's manifest: reuse leaves no trace (no markers, counts, or
  timestamps persisted).
- New artifacts (no record), kind changes, missing/modified outputs, and
  tampered records all rebuild. Stale paths (in manifest, absent from plan)
  were left alone in this slice; pruning landed in 12C (0017).
- Observability is runtime-only: planned/reused/rebuilt counts plus the
  rebuilt path set on `BuildSummary`, printed by the CLI. Nothing about
  reuse is persisted.
- Static assets reuse by the same rule: current source digest must equal
  the recorded output digest (they coincide for verbatim passthrough).
- Reuse is additionally gated on generation compatibility: the previous
  manifest's identity must exactly match the running binary's. An absent
  identity (a manifest predating the field) is incompatible and never
  inferred from the schema version — see
  `0019-generation-behavior-identity.md`.
- Template-rendered artifacts record the complete loaded template set
  (`TemplateSet`), so any template change — including `{% extends %}` and
  `{% include %}` targets — invalidates them. Precise per-artifact closures
  are deliberately not attempted: MiniJinja caches loaded templates and
  exposes no per-render hook — see `0020-template-dependency-closure.md`.

## Consequences

- Identical second builds resolve nothing and rewrite nothing (proven by
  read-only-output builds succeeding).
- Invalidation precision follows the recorded inputs exactly: title edits
  skip the sitemap (routes+lastmod only); tag edits skip the main feed
  (no tags in items); config edits rebuild everything except search and
  statics.
- `BuildSummary` gains `reused`/`rebuilt`/`rebuilt_paths`; `pages_written`
  now counts actually-written artifacts (identical in full builds).

## Follow-ups

Stale-output pruning (0017), generation compatibility (0019), and
template-set invalidation (0020) all landed. Fine-grained config scoping
remains a possible future refinement; exact per-artifact template closures are
deliberately not attempted.
