# ADR 0003: Rendering boundary (MiniJinja behind a trait)

## Status

Accepted.

## Context

Templates need real power, but template engines easily leak into domain logic
(engine types in core, filesystem loaders, unescaped HTML, ambient context).

## Decision

Hide MiniJinja behind `signal-render::Renderer` with explicit `RenderContext`
value bags. Templates load from in-memory strings only (no filesystem or
network access); HTML auto-escaping is on for `.html` templates; engine errors
surface as owned `RenderError`. Template dependencies are tracked via the
loader closure / recorded template set — never magic field-level tracking.

## Consequences

- `signal-core` has no template-engine dependency (pinned by tests).
- Rendering stays synchronous and auditable.
- Swapping engines later touches one crate.

## Follow-up

Template dependency recording and manifest integration landed in slices
12A/14C. It is deliberately conservative: template-rendered artifacts depend
on the whole loaded template set (`InputRef::TemplateSet`) rather than
loader-closure tracking, because MiniJinja 2.x exposes no per-render
dependency hook — see `0020-template-dependency-closure.md`.
