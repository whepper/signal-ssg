---
title: Features
description: What Signal 1.0 can do, and what it deliberately does not.
---

Signal 1.0 includes the core pieces needed for a content-oriented static site. Each item below links to its guide.

## Pages

- Home page with optional featured and recent entries
- Collection section pages
- Individual entry pages
- Taxonomy index and term pages
- Themed not-found page (`404.html`), when `site.not_found_template` is set

See [Content](../content/) and [Templates](../templates/).

## Metadata

Canonical URLs, OpenGraph metadata, and JSON-LD are generated when the required inputs exist. Missing metadata is omitted rather than fabricated.

## Feeds, sitemap, and robots

Signal generates a family of RSS feeds from normalized site data: the main feed, one feed per collection (section), the taxonomy label-index feed, and one feed per term. It also generates a sitemap from the route inventory and, when `[robots]` is set, a deterministic allow-all `robots.txt`. See [Configuration](../configuration/).

## Search

Signal generates a versioned static `index.json` containing searchable plain-text projections. Search UI and search-engine behavior remain outside the engine.

## Markdown

Markdown is parsed into owned derivatives including HTML, headings, TOC data, code highlighting, Mermaid blocks, alerts, plain text, word count, and URL information. See [Content](../content/).

## Navigation

`menus.main` provides deterministic navigation and per-page active state. Internal menu targets are validated like any other internal reference. See [Configuration](../configuration/) and [Reference validation](../validation/).

## Static assets

Files under `static/` are copied to the output tree as planned static artifacts, covered by collision validation and the manifest like everything else.

## Incremental builds

Signal records source, configuration, template, and output digests in a schema-versioned build manifest, plans per-artifact reuse with reason codes, and explains its decisions on demand. See [Incremental builds](../builds/) and the [CLI reference](../cli/).

## Validation

Internal links, images, front-matter images, and menu targets are validated against the planned output before anything is written; external URLs are never fetched. See [Reference validation](../validation/).

## Security

Routes, output paths, author URLs, Markdown HTML, and template execution all have explicit safety boundaries. See [Architecture](../architecture/).

## Deferred

Image processing, pagination, aliases/redirects, plugins, CMS integration, live reload, and a built-in search UI are deliberately not implemented. The [Architecture](../architecture/) page records the full non-goals list and why each conflicts with the engine's guarantees.
