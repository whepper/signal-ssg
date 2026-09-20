# Architecture

The canonical architecture document is [`ARCHITECTURE.md`](../../ARCHITECTURE.md)
at the repository root. It describes the canonical pipeline, the
command/stage contract, `BuildPlan` and execution separation, declared
artifact inputs, manifest semantics, pruning, reference validation, and
what Signal deliberately does not attempt to be.

The [ADRs](adr/) contain the detailed rationale and history behind these
boundaries.

## Crate responsibilities (summary)

| Crate | Responsibility |
|---|---|
| `signal-core` | Site model, identities, queries, configuration, artifact specifications |
| `signal-markdown` | Markdown and front-matter parsing; owned derivatives |
| `signal-render` | Renderer abstraction and MiniJinja implementation |
| `signal-generators` | Pure projections from `&SiteModel` to artifacts |
| `signal-cli` | Filesystem boundary, discovery, orchestration, writing, manifest |

See `ARCHITECTURE.md` §10 for the full boundary table.
