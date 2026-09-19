# ADR 0005: Ingestion and resolution placement

## Status

Accepted (first vertical slice).

## Context

The bootstrap left ingestion, front-matter formats, template selection, and
artifact resolution unplaced. The Hugo migration forced decisions: the live
site mixes YAML (`---`) and TOML (`+++`) front matter (including bare TOML
dates), uses filename-stem slugs with date prefixes, renders branch bundles
(`_index.md`) at section roots, and serves `static/` verbatim.

## Decision

- Front-matter parsing lives in `signal-markdown` (both delimiters,
  via an intermediate `serde_json::Value` so TOML datetimes normalize to
  strings). Unknown fields are preserved in `FrontMatter.extra`.
- `RenderedBody`/`Heading` live in `signal-core` as canonical owned data;
  `signal-markdown` produces them. Core still has no Comrak dependency.
- Filesystem ingest (`signal-cli::ingest`): per-collection discovery from
  config (`source`, default `content/<name>`), deterministic `ContentId`
  assignment over sorted `(collection, path)`, slug = `slug` field else
  filename stem verbatim, `_index.md`/`index.md` → collection root route,
  drafts skipped, Hugo `topics` merged into `tags`.
- Generators stay pure (`&SiteModel -> Vec<ArtifactSpec>`). Rendering and
  writing live in `signal-cli::build_site`: templates load from
  `<root>/templates` into `MiniJinjaRenderer` as strings (per-collection
  `template` config, default `post.html`), one artifact rendered and written
  at a time. Pre-rendered HTML reaches templates as `content` with an
  explicit `| safe` convention (auto-escape stays on).
- `static/` copies verbatim to the output root (no processing).

## Consequences

- Crate boundaries hold: core has no I/O, no Comrak, no MiniJinja (pinned by
  the existing invariant tests).
- Date-prefix slugs and section-root routes give Hugo-identical URLs without
  Hugo-compatible machinery.
- Reading time (`ceil(words/200)`) differs slightly from Hugo's rendered-word
  count; date formatting is raw strings — both accepted diffs for slice 5.

## Deferred

Heading IDs/TOC, taxonomy pages, feeds, sitemap, search, asset fingerprinting,
codeblock/mermaid/alert components, menu abstraction.
