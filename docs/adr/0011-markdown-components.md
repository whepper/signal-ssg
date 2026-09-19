# ADR 0011: Markdown presentation components at the AST level

## Status

Accepted (slice 8: code blocks, Mermaid, alerts).

## Context

Code highlighting, diagram blocks, and callouts must reach rendered HTML
and the normalized model identically, without HTML post-processing, a
second parser, or raw-HTML passthrough (Signal's content model carries
none — Comrak's default omit behavior is preserved by detaching author
HTML nodes before render).

## Decision

- One Comrak parse per document, then in-place AST transforms strictly
  inside `signal-markdown`: user HTML detached first (so the required
  `render.unsafe_` passthrough only ever exposes Signal-generated nodes),
  alert blockquotes spliced into `<aside>` wrappers with their real
  children moved (inline Markdown, lists, code, and headings keep working
  with correct anchors), fenced code blocks value-swapped for their final
  HTML. A single render pass with the heading adapter then produces HTML,
  headings, and code metadata together.
- Highlighting composes Comrak's `SyntectAdapter` directly (class spans,
  no inline styles, shared process-wide instance): no new highlighter
  dependency, no theme config, deterministic output. Unknown languages
  fall back deterministically; per-block filename/linenos/hl-lines
  attributes stay out (no live usage observed).
- Copy affordance is server-rendered static markup (`div.code-block` +
  `data-language` + `button[data-code-copy]`); behavior stays in site
  assets. Mermaid source is preserved as escaped text in `pre.mermaid`
  for the site's client renderer, duplicated inside a collapsed
  `details.mermaid-source` fallback so the diagram stays meaningful without
  JavaScript; `has_mermaid` gates loader inclusion
  from normalized `code_blocks` metadata — templates never parse HTML.
- Alerts are the five GitHub kinds with fixed labels and an ARIA `role`
  (`alert` for warning/caution, `note` for the rest); unknown markers
  stay plain blockquotes. No extension framework: three concrete
  capabilities, two private modules.

## Consequences

- New `RenderedBody.code_blocks` (`language` + `source`) serves the
  diagram gate today and a future search index without touching HTML.
- `signal-core` gains no parser dependency; the public Markdown API
  surface is unchanged apart from richer owned data.

## Deferred

Search, menus, image processing, per-block code attributes if live usage
ever requires them.
