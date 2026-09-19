# ADR 0014: Complete artifact plans, path containment, independent resolution

## Status

Accepted (slice 11: artifact boundary corrections, from Architecture Review 1.0).

## Context

Architecture Review 1.0 found the plan → resolve → write pipeline described
slightly less than the build produced, and resolution was not independently
callable:

1. Static assets were copied *after* the artifact loop, outside the plan —
   so the plan did not describe the complete output, collision validation
   could not see `static/index.json` vs the search index, and a future
   manifest could not enumerate (or delete) stale assets.
2. HTML artifact resolution was inlined in the build loop, closing over
   orchestration locals (`homes`, `terms`, feed projections) — no single
   artifact could be regenerated without the full loop.
3. Route components (front-matter slugs, filename stems, config route
   prefixes) were unvalidated, so `slug: "../../evil"` or a file named
   `...md` could write outside the output directory.

## Decision

- **Plan completeness.** Static files are enumerated (deterministically,
  symlinks skipped) during planning as `ArtifactKind::Static` specs —
  path-only, never bytes. The invariant is now: *every output file
  corresponds to exactly one ArtifactSpec.* Generated/static path
  collisions fail in validation, before anything is written.
- **Independent resolution.** `resolve_artifact(spec, config, root, model,
  renderer) -> bytes` is the single dispatch point for every artifact kind.
  HTML resolution (`resolve_html_artifact`) is self-contained: cheap
  deterministic projections (`topic_terms`, home picks) are recomputed
  inside rather than passed through loop-local state. Static resolution
  reads the source; data artifacts serialize from the model. The build loop
  is now uniformly resolve → write.
- **Path containment.** `validate_route_segment`/`validate_route` in
  `signal-core` reject `.`, `..`, empty, separator, backslash, whitespace,
  and control-character segments at the semantic boundary (slugs at
  ingest, collection/taxonomy prefixes before planning). `write_artifact`
  enforces component-based containment at the write boundary: only normal
  relative components are writable. Traversal input fails the build with a
  diagnostic; it is never silently rewritten. Since slice 14D the boundary
  is also filesystem-aware: it resolves the output root once, refuses
  symlinked ancestor directories (and, for writes, a final-component
  symlink), and stale pruning compares filesystem identities so an aliased
  current artifact is never deleted — see ADR 0017.

## Consequences

- The plan is a complete, serializable description of the output — the
  prerequisite for a manifest (input/query digests, skip-on-match,
  delete-on-absence) without introducing a build DAG.
- Byte-for-byte outputs are unchanged for valid sites (golden-verified);
  only previously-invalid paths now fail.
- Route segments reject whitespace: URLs with raw spaces were never
  well-formed, and this is enforced where routes are born.

## Deferred

The manifest itself (digests, skip logic, pruning) — the next slice. Menu
URLs accept no `.`/`..` segments for consistency, but are hrefs, never
output paths.
