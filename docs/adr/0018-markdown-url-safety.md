# ADR 0018: Author URL safety at the Markdown AST boundary

## Status

Accepted (slice 14A: security fix; no manifest, reuse, or filesystem changes).

## Context

Slice 13 demonstrated that author Markdown link and image destinations were
emitted verbatim: `[x](javascript:alert(1))` produced a live
`href="javascript:alert(1)"` and `![x](javascript:alert(1))` a live `src`.
This contradicted Signal's existing policy for front-matter images
(`signal_core::resolve_image_url`, which rejects every scheme but
`http(s)`) and the deliberate decision to detach author raw HTML. Comrak
decodes HTML entity references (numeric and named) before rendering, so
`javascript&#58;`, `&#106;avascript:`, and `data&colon;` all arrived as
their literal schemes.

## Decision

- One shared allow-list predicate in `signal-core::meta`:
  `is_safe_author_url(value, UrlUse)` — the single definition of a safe
  author URL. Relative, site-root-relative, and protocol-relative
  destinations plus `http`/`https` always pass; `mailto:` passes for links
  but not image sources; every other scheme is rejected. Values that are
  empty or contain whitespace/control characters are rejected. Scheme
  detection is case-insensitive and operates on the parsed `scheme:rest`
  shape.
- `resolve_image_url` now delegates its scheme/whitespace decision to that
  predicate with `UrlUse::Image`, so front-matter images and Markdown image
  sources cannot drift apart. No new dependency and no new crate edge:
  `signal-markdown` already depends on `signal-core`.
- Enforcement happens at the Markdown AST boundary, before any HTML is
  generated (`neutralize_unsafe_urls` in `signal-markdown`): a rejected
  link or image node is unwrapped, leaving its inline children (link text,
  image alt text, nested emphasis) as ordinary text; the node itself — and
  therefore its destination — is detached. No `href`/`src` is emitted for a
  rejected destination, and surrounding nodes are untouched. Post-render
  HTML filtering is explicitly not used.

## Consequences

- Browser-visible output can never contain an author-controlled
  `javascript:`, `data:`, or `vbscript:` destination in `href`/`src`.
- Author content is preserved as text rather than silently dropped: a
  rejected link becomes its link text, a rejected image its alt text.
- This protects URL destinations only. It does not sanitize HTML (raw HTML
  remains detached), does not claim general Markdown safety, and does not
  change templates, the manifest, incremental reuse, or pruning.
- `RenderedBody.links`/`images` (naive source scans, consumed by no
  artifact) are deliberately unchanged in this slice; the security boundary
  is the AST that becomes HTML.
