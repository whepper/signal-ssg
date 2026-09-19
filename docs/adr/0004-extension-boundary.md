# ADR 0004: Extension boundary (no plugins yet)

## Status

Accepted.

## Context

Plugin systems (WASM, native dynamic libraries, remote content, CMS hooks)
are frequently requested for SSGs, but each carries versioning, sandboxing,
and determinism costs. Building one before the compiler pipeline exists would
freeze the wrong abstraction.

## Decision

Ship no plugin system in bootstrap. Extension points are limited to the
`Generator` and `Renderer` traits (plain Rust, synchronous, no `async`
traits, no dynamic libraries). Crate boundaries (`signal-generators`,
`signal-render`) admit future WASM or native backends without changing core.

## Consequences

- Small, reviewable API surface.
- Deterministic builds are not threatened by host-plugin skew.
- Future proposals must show why a trait implementation is insufficient.

## Deferred

WASM plugins, native plugins, remote content, CMS integration, image
pipelines, i18n, and a search engine/UI remain out of scope. (A static search
*index* shipped in ADR 0012; the engine and UI stay external.)
