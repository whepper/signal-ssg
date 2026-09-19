# Architecture

Signal turns a site into static artifacts through a fixed pipeline.

```text
sources
  ↓
ingestion
  ↓
validation / normalization
  ↓
immutable SiteModel
  ↓
queries / generators
  ↓
ArtifactSpec
  ↓
rendering / resolution
  ↓
output
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

Artifact specifications contain output paths and kinds, not rendered bytes.

Templates receive explicit contexts and HTML templates are auto-escaped.

Filesystem writes and stale removals use a dedicated contained boundary with symlink checks.

## Determinism

Source discovery and generated artifacts are sorted. Ordered collections are used where output order matters. The build manifest is deterministic.

## Incremental builds

The manifest records generation identity plus relevant input and output digests. Reuse requires compatibility and matching inputs and output bytes.

Stale outputs are derived from the previous manifest minus the current artifact plan.

## Security model

Signal rejects unsafe route components and unsupported author URL schemes, detaches author raw HTML, and refuses unsafe output-tree traversal.

## Deeper design records

The [ADRs](adr/) contain the detailed rationale and history behind these boundaries.
