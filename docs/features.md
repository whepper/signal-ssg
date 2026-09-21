# Features

Signal 1.0 includes the core pieces needed for a content-oriented static site.

## Pages

- Home page with optional featured and recent entries
- Collection section pages
- Individual entry pages
- Taxonomy index and term pages
- Themed not-found page (`404.html`), when `site.not_found_template` is set

## Metadata

Canonical URLs, OpenGraph metadata, and JSON-LD are generated when the required inputs exist. Missing metadata is omitted rather than fabricated.

Entries can also expose related entries (strongest shared-topic overlaps,
capped) and, when `[git] last_modified` is enabled, last-modification dates
derived from repository history.

## Feeds, sitemap, and robots

Signal can generate a family of RSS feeds from normalized site data: the
main feed, one feed per collection (section), the taxonomy label-index feed,
and one feed per term. It also generates a sitemap from the route inventory
and, when `[robots]` is set, a deterministic allow-all `robots.txt`.

## Search

Signal can generate a versioned static `index.json` containing searchable plain-text projections. Signal owns extraction, structural normalization, and serialization only: tokenization, ranking, filtering, and the search UI remain outside the engine. The index is a pure projection of the normalized model, so it rebuilds only when searchable content changes — template, configuration, asset, and fenced-code edits reuse it. `signal explain index.json` reports the artifact's kind, declared query input, document count, and reuse/rebuild decision.

## Markdown

Markdown is parsed into owned derivatives including HTML, headings, TOC data, code highlighting, plain text, word count, and URL information.

## Navigation

`menus.main` provides deterministic navigation and per-page active state.

## Static assets

Files under `static/` are copied to the output tree as planned static artifacts.

## Image derivatives

With `[images]` configured (`widths` plus `format = "webp"` or
`formats = ["avif", "webp"]`), content-referenced PNG/JPEG sources gain
deterministic resized derivatives (`hero-640.webp`, `hero-640.avif`, …):
aspect-preserving, never upscaled, one artifact per source × width ×
format, tracked as build inputs on both the derivative artifacts and the
embedding pages, reused byte-identically when unchanged, and explainable
per derivative via `signal explain <asset> --width W [--format F]`.
Rendered pages express them as responsive markup: a single planned
format renders a responsive `<img>`; several render `<picture>` with
AVIF-first `<source>` elements and a WebP fallback `<img>`.

## Social images

With `[social]` configured, every entry page gains a deterministic
`social/<route-path>.png` social image (a sharing card) composed from the
site title, the page title, its optional description and author, and its
optional PNG/JPEG hero (centred cover crop). It is referenced from
`og:image` and `twitter:image` and participates in incremental builds like
any other artifact: page metadata, the hero, or the configured dimensions
rebuild it; disabling `[social]` prunes it. `signal explain
social/<path>.png` describes the decision.

## Diagnostics

`signal check` and `signal explain <asset>` report evidence-based
diagnostics over the publishing model: unreferenced raster assets, sources
far larger than the largest representation Signal generates from them,
configured widths that clamp to the same output, and missing or empty
image alternative text. Diagnostics are advisory — `warning` or `info`,
never a build failure — state measured facts rather than advice, and are
derived from the same analysis in both commands, so they always agree.
They are not artifacts: nothing is written, pruned, or recorded in the
manifest. Byte-level "derivative larger than its source" is deliberately
not reported, because `check` has no generated output to measure.

## HTML output

Template-rendered pages can be minified with `[output] minify_html = true`
(off by default): deterministic, HTML-aware whitespace reduction that
preserves `pre`/`code` contents, Mermaid sources, `textarea`, scripts,
styles, JSON-LD, entities, attributes, and comments. Feeds, sitemap, search
index, robots.txt, and static files are never minified.

## Incremental builds

Signal records source, configuration, template, and output digests in a schema-versioned build manifest.

## Security

Routes, output paths, author URLs, Markdown HTML, and template execution all have explicit safety boundaries.

## Deferred

Cropping, art direction, pagination, aliases/redirects, plugins, CMS integration, and a built-in search UI are not implemented in the current release. Diagnostics are advisory only: no byte-level derivative-size analysis (it needs generated output), no configurable thresholds, and no automatic optimization or rewriting of assets.
