# ADR 0002: Build manifest, not build DAG

## Status

Accepted. The deferred manifest later arrived as slices 12A–12C: a flat
per-artifact manifest (ADR 0015), incremental reuse (0016), stale pruning
(0017), generation identity (0019), and template-set invalidation (0020). No
build DAG was ever introduced, as this ADR requires.

## Context

Incremental rebuilds need to know what to redo, which suggests keeping a live
dependency graph. A persistent mutable DAG easily becomes a second source of
truth that can disagree with content.

## Decision

Keep no persistent mutable build DAG. Future incremental builds derive
dependencies from generation (input hashes, query identities/result digests,
template dependency sets, artifact metadata) and persist them in a disposable
`.signal/` manifest. The `SiteModel` stays authoritative; deleting `.signal/`
must never change correctness.

## Consequences

- Semantic references stay query substrate, never implicit build edges.
- Cache corruption is recoverable by deletion.
- Bootstrap implements only route/output-path validation, no cache logic.

## Follow-ups

Manifest schema, hashing, skip logic, generation identity, and template-set
invalidation all landed (ADRs 0015–0020). This ADR's core decision — no
persistent mutable build DAG — still holds.
