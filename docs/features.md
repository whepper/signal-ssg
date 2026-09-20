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

Signal can generate a versioned static `index.json` containing searchable plain-text projections. Search UI and search-engine behavior remain outside the engine.

## Markdown

Markdown is parsed into owned derivatives including HTML, headings, TOC data, code highlighting, plain text, word count, and URL information.

## Navigation

`menus.main` provides deterministic navigation and per-page active state.

## Static assets

Files under `static/` are copied to the output tree as planned static artifacts.

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

Image processing, pagination, aliases/redirects, plugins, CMS integration, and a built-in search UI are not implemented in the current release.
