# ADR 0015: Manifest recording and deterministic input digests

## Status

Accepted (slice 12A: manifest recording; no skipping, no pruning).
Extended by 0016 (reuse), 0017 (pruning), 0019 (generation identity), and
0020 (template-set invalidation).

## Context

Architecture Review 1.0 concluded incremental builds are viable with a
flat per-artifact digest manifest — no mutable model, no persistent DAG —
once the artifact boundary corrections (slice 11) landed. This slice
records that manifest; a later slice consumes it.

## Decision

- `.signal/manifest.json` (pretty-printed, atomic temp+rename write) is
  produced only after every artifact resolves and writes. It is build
  state: never planned, never served, excluded from sitemap/search/output
  enumeration by the same rule.
- One digest representation: lowercase hex SHA-256 over canonical JSON
  (structs in field order, `BTreeMap`s, ordered `Vec`s). No `Debug`, no
  `HashMap` iteration, no timestamps, no mtimes, no pointers.
- Entry digests hash an explicit semantic subset (identity, route, slug,
  title, description, dates, author, image, tags, flags, whole body).
  `translation_group`, `language`, and `references` are excluded — no
  current artifact consumes them; adding such a consumer must extend the
  digest input.
- Query digests hash consumed projections, never bare IDs
  (`EntrySummary`/`TopicSummary`/`FeedItem`/`SitemapUrl`/`SearchDocument`
  values, including date-formatted fields so `date_format` changes
  invalidate listings). Keys are `family:param` with fixed disjoint
  families (`summaries:`, `home:`, `topic_terms`, `tagged:`, `feed:main`,
  `feed:term:`, `routes_inventory`, `search_documents`).
- Config digest is whole canonical `SignalConfig` (deliberately coarse;
  only the digest is stored, never config values, so sensitive values
  leave no trace in the file).
- Templates record the full loaded set (`name → digest`); template-rendered
  artifacts depend on that set as a whole (`TemplateSet`, slice 14C), so
  `{% extends %}` / `{% include %}` changes invalidate soundly. Per-artifact
  exact closures remain deferred (deliberate precision trade-off, ADR 0020).
- Artifact inputs are explicit `InputRef`s
  (`Entry`/`Query`/`TemplateSet`/`Config`/`Static`) derived from the same
  resolution code that produces the bytes (template selection helpers are
  shared, not duplicated). (`Template { name }` remains only for reading
  pre-14C manifests.) Static inputs name their source; their digest is the
  content hash. Menus need no separate variant: menu config is inside the
  whole-config digest recorded on every HTML artifact.
- Output digests are SHA-256 of exact written bytes, collected during the
  resolve/write loop (no re-reads, no accumulation beyond 32 bytes each).
- The manifest also records a generation-behavior identity (`engine_version`
  = the Cargo package version; `behavior_version` = the maintained
  `GENERATION_BEHAVIOR_VERSION`). It is a build-wide compatibility field,
  never an artifact input — see
  `0019-generation-behavior-identity.md`.

## Consequences

- The manifest completely describes one build's inputs and outputs and is
  byte-identical across independent builds (frozen golden for the sample
  fixture proves it).
- Invalidation compares recomputed digests against these records (0016);
  nothing about a build branches on the manifest it writes.
- Exact per-artifact template closure is not attempted; template-rendered
  artifacts depend on the whole loaded set (0015 Templates bullet, ADR 0020).

## Follow-ups

All of these landed: skipping/reuse (0016), pruning (0017), generation
identity (0019), template-set invalidation (0020). Fine-grained config scoping
remains a possible future refinement. No DAG ever: propagation stays implicit
through digest comparison.
