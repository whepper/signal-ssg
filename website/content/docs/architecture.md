---
title: Architecture
description: How Signal turns a site into static artifacts.
---

Signal turns a site into static artifacts through a fixed pipeline. Every command — `build`, `check`, `serve`, `build --explain` — shares the same upstream construction and validation; commands differ only in where they stop. The command/stage contract is:

| Command | Ingest | Structural validation | Reference validation | Plan | Execute | Prune | Manifest |
|---|---|---|---|---|---|---|---|
| `build` | yes | yes | yes | yes | yes | yes | yes |
| `serve` | yes | yes | yes | yes | yes | yes | yes |
| `check` | yes | yes | yes | no | no | no | no |
| `build --explain` | yes | yes | yes | yes | no | no | no |

The invariant: no command calls a site valid while skipping validation a real build requires. See the [CLI reference](../cli/).

```text
config
  ↓
ingest
  ↓
validated specs (config gates, output paths, routes, templates)
  ↓
reference validation
  ↓
incremental BuildPlan
  ↓
execute
  ↓
prune
  ↓
manifest
```

## Crates

| Crate | Responsibility |
|---|---|
| `signal-core` | Site model, identities, queries, configuration, artifact specifications |
| `signal-markdown` | Markdown and front-matter parsing; owned derivatives |
| `signal-render` | Renderer abstraction and MiniJinja implementation |
| `signal-generators` | Pure projections from `&SiteModel` to artifacts |
| `signal-cli` | Filesystem boundary, discovery, orchestration, writing, manifest |

## Important boundaries

The core model is immutable after normalization. Generators do not mutate it or read rendered HTML.

Artifact specifications contain output paths and kinds, not rendered bytes. Dependencies are declared per artifact as inputs (entries, query projections, the template set, configuration, static sources) and recorded in a disposable build manifest — there is no persistent dependency graph.

Templates receive explicit contexts and HTML templates are auto-escaped. Templates have no filesystem or network reach.

Filesystem writes and stale removals use a dedicated contained boundary with symlink checks. No artifact can escape the output directory.

## Determinism

Source discovery and generated artifacts are sorted. Ordered collections are used where output order matters. The build manifest is deterministic, and two clean builds of the same site are byte-identical, including the manifest. Date formatting is locale- and timezone-independent by construction.

## Incremental builds

The manifest records generation identity plus relevant input and output digests. Reuse requires compatibility and matching inputs and output bytes — timestamps are never consulted.

Stale outputs are derived from the previous manifest minus the current artifact plan. The full model, including per-artifact rebuild reasons, is documented in [Incremental builds](../builds/).

## Reference validation

Structured internal references are validated against the planned output before anything is written. See [Reference validation](../validation/).

## Security model

Signal rejects unsafe route components and unsupported author URL schemes, detaches author raw HTML, refuses unsafe output-tree traversal, and forbids `unsafe` Rust in every crate. The engine performs no network access.

## Non-goals

Signal deliberately does not attempt: plugins, remote content, further image codecs beyond WebP/AVIF, image cropping or art direction, alternate social-card themes or dimensions, remote image services, a search UI, CMS integration, dynamic runtime behavior, redirects/aliases, pagination, a custom template language, a custom Markdown parser, a graph traversal engine, or async builds. Each would undermine determinism, auditability, reproducibility, explicit planning, or fail-closed behavior.

Generated artifacts that stay inside that model are in scope: first-class assets, deterministic image derivatives (WebP and AVIF), responsive `srcset`/`<picture>` markup, and generated Open Graph/Twitter social images.

## Deeper design records

Detailed rationale and history live in the repository's ADRs under `docs/adr/`.
