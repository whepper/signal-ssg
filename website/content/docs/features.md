---
title: Features
description: What Signal's current beta can do.
---

Signal's current beta includes the core pieces needed for a content-oriented static site.

## Pages

- Home page with optional featured and recent entries
- Collection section pages
- Individual entry pages
- Taxonomy index and term pages

## Metadata

Canonical URLs, OpenGraph metadata, and JSON-LD are generated when the required inputs exist. Missing metadata is omitted rather than fabricated.

## Feeds and sitemap

Signal can generate a main RSS feed, taxonomy term feeds, and a sitemap from normalized site data.

## Search

Signal can generate a versioned static `index.json` containing searchable plain-text projections. Search UI and search-engine behavior remain outside the engine.

## Markdown

Markdown is parsed into owned derivatives including HTML, headings, TOC data, code highlighting, plain text, word count, and URL information.

## Navigation

`menus.main` provides deterministic navigation and per-page active state.

## Static assets

Files under `static/` are copied to the output tree as planned static artifacts.

## Incremental builds

Signal records source, configuration, template, and output digests in a schema-versioned build manifest.

## Security

Routes, output paths, author URLs, Markdown HTML, and template execution all have explicit safety boundaries.

## Deferred

Image processing, pagination, aliases/redirects, plugins, CMS integration, and a built-in search UI are not implemented in the current beta.
