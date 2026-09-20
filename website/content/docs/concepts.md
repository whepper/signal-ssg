---
title: Concepts
description: The site model, artifacts, determinism, and incremental builds.
---

Signal treats a website as data that is compiled into static artifacts. This page explains the ideas behind the commands; hands-on material is in [Getting started](/docs/getting-started/), and command details are in the [CLI reference](/docs/cli/).

## Site

A Signal site owns its content, templates, static assets, and `signal.toml`. The Signal engine is an independent consumer of that site — nothing site-specific lives inside Signal. The expected inputs are:

| Input | Purpose |
|---|---|
| `signal.toml` | Identity, base URL, collections, taxonomy, feeds, options |
| `content/` | Markdown sources with front matter (per-collection sources) |
| `templates/` | MiniJinja HTML templates for pages, listings, taxonomy, 404 |
| `static/` | Files copied verbatim into the output tree |

See [Configuration](/docs/configuration/) and [Deployment](/docs/deployment/).

## Content

Markdown files become normalized content entries. Identity, source path, slug, and output route are separate concepts: the *source* (`content/posts/hello-world.md`) is stable, the *slug* (`hello-world`) is human-readable, and the *route* (`/posts/hello-world/`) is the output address. See [Content](/docs/content/).

## Collections

A collection is a named set of content with a source directory and route prefix:

```toml
[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

An entry under `content/posts/` can therefore become a page under `/posts/`. Each collection yields one page per entry plus one section listing.

## Site model

Signal ingests and validates source material, then freezes an immutable site model. Generators query that model rather than reading rendered HTML back from disk. Downstream stages only ever see `&SiteModel`: content cannot change mid-build.

## Artifacts

Generators plan lightweight artifact specifications — output path plus kind, not rendered bytes. Later stages resolve and write them.

Artifacts can be pages, section indexes, the home page, taxonomy pages, RSS feeds (main, section, taxonomy-label index, and per-term), a sitemap, a robots file, a search index, a themed not-found page, or static files. Every output file corresponds to exactly one specification; static assets are first-class artifacts too, so collisions between generated and static paths fail validation before anything is written.

## Validation before output

After planning and before writing anything, Signal validates structured internal references — Markdown links and images, front-matter images, and menu targets — against the inventory of what the build will generate. A broken reference fails the build with the output tree and manifest untouched. See [Reference validation](/docs/validation/).

## Determinism

Source discovery, model indexes, generated artifacts, and manifest structures are ordered consistently. The goal is that identical inputs produce identical output: two clean builds of the same site with the same Signal build are byte-identical, including the manifest.

Signal does not make performance claims yet.

## Incremental builds

Successful builds write `.signal/manifest.json`. Signal can reuse an artifact only when generation is compatible, its recorded inputs still match, and the existing output still matches its recorded digest. Timestamps are never consulted.

A missing or unusable manifest disables reuse and falls back to a full build. `signal build --explain` shows each decision and its reason without writing anything. The full model — inputs, reasons, pruning, failure semantics — is documented in [Incremental builds](/docs/builds/).

## Rendering

Templates receive explicit values. HTML templates are auto-escaped, and the renderer does not expose filesystem or network loading. See [Templates](/docs/templates/).

## Static output

There is no Signal runtime in production. The final result is ordinary static files. During development, `signal serve` rebuilds through the same production pipeline and serves the output locally.
