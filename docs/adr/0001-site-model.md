# ADR 0001: SiteModel, not SiteGraph

## Status

Accepted.

## Context

Content relationships (references, translations, tags) feel graph-like, which
tempts a generic graph engine with traversal primitives.

## Decision

Model the site as an immutable collection of ordinary indexed Rust structures
(`BTreeMap`/`BTreeSet`) with purpose-built query functions
(`entries_in_collection`, `entries_tagged`, `translations_of`,
`referencing`, `lookup_by_route`).

## Consequences

- Simple, predictable, borrow-friendly: everything is `&SiteModel`.
- Deterministic ordering falls out of ordered maps/sets.
- No traversal framework to maintain or misuse.

## Deferred

Introduce graph traversal infrastructure only when a real requirement (not a
metaphor) demands it.
