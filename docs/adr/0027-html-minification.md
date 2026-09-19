# ADR 0027: Opt-in HTML minification as a post-render output step

## Status

Accepted (blog-compatibility slice: closes gaps.md #13).

## Context

The reference blog builds with `hugo --minify`. Signal emitted
template-formatted HTML only. The requirement is smaller deterministic
output with unchanged observable behavior — not Hugo's exact bytes.

## Decision

- Configuration: `[output] minify_html = true`, off by default. A new
  `OutputConfig` table on `SignalConfig` (whole-config digest covers it,
  so no special manifest input is needed).
- Placement: `template rendering → rendered HTML → HTML minification →
  artifact/output`. The minifier runs in `signal-cli` artifact resolution
  (`finalize_html`) on finished rendered strings only — entry pages,
  listings, home, taxonomy, and the themed 404. Feeds, sitemap, search
  JSON, robots.txt, and static bytes never reach it. `signal-core` stays
  free of HTML parsing; template contexts and Markdown parsing are
  untouched. No generalized output-transformation framework.
- Library: `simple-minify-html` 0.17 (MIT, `#![deny(unsafe_code)]`,
  deps `aho-corasick`/`memchr`/`rustc-hash` already in the tree). A real
  HTML parser, never regexes. Chosen over `minify-html` 0.18, which pulls
  in the oxc JS toolchain and lightningcss — a large surface for a
  feature that must explicitly *not* do JS/CSS processing. The `js`/`css`
  features stay disabled, so inline scripts and styles are trimmed at
  most, never reinterpreted.
- Safety configuration: library defaults plus `keep_comments = true`.
  Conditional, SSI, license, and template comments are not provably safe
  to remove and cost little. `<pre>` subtrees (highlighted code, Mermaid
  sources), `<textarea>`, `<script>`, and `<style>` contents are preserved
  by the parser; entity re-encoding is decoding-identical.
- Invalidation: toggling the flag moves the whole-config digest, so every
  artifact already naming `Config` rebuilds; search index and static files
  stay reused. `GENERATION_BEHAVIOR_VERSION` bumped to `8`.

## Consequences

- Minified pages are ~10–20% smaller with identical text content,
  metadata, links, scripts, JSON-LD values, Mermaid sources, and
  copy-button/code structure (verified against the real blog output).
- Whitespace/attribute serialization (optional closing tags omitted,
  safe unquoting, minimal entity encoding) differs from Hugo's minifier
  by implementation. Both are valid HTML5 with identical browser
  behavior; no byte-parity work will be done.

## Deferred

CSS/JS minification, compression, fingerprinting, and SRI remain out of
scope (gap #9 covers the asset-pipeline question).
