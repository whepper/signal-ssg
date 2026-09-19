---
title: Concepts
description: The model behind Signal sites and builds.
---

Signal treats a website as data that is compiled into static artifacts.

## Site

A Signal site owns its content, templates, static assets, and `signal.toml`. The Signal engine is an independent consumer of that site.

## Content

Markdown files become normalized content entries. Identity, source path, slug, and output route are separate concepts.

## Collections

A collection is a named set of content with a source directory and route prefix:

```toml
[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

An entry under `content/posts/` can therefore become a page under `/posts/`.

## Site model

Signal ingests and validates source material, then freezes an immutable site model. Generators query that model rather than reading rendered HTML back from disk.

## Artifacts

Generators plan lightweight artifact specifications. Later stages resolve and write them.

Artifacts can be pages, section indexes, taxonomy pages, RSS feeds, a sitemap, a search index, or static files.

## Determinism

Source discovery, model indexes, generated artifacts, and manifest structures are ordered consistently. The goal is that identical inputs produce identical output.

Signal does not make performance claims yet.

## Incremental builds

Successful builds write `.signal/manifest.json`. Signal can reuse an artifact only when generation is compatible, its recorded inputs still match, and the existing output still matches its recorded digest.

A missing or unusable manifest disables reuse and falls back to a full build.

## Rendering

Templates receive explicit values. HTML templates are auto-escaped, and the renderer does not expose filesystem or network loading.

## Static output

There is no Signal runtime in production. The final result is ordinary static files.
