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

## Related entries

Entry pages can expose related entries: the strongest shared-topic overlaps, capped by `[related] limit` (default 3), newest-first within equal overlap. The projection is a pure function of the normalized model, so a page rebuilds when any entry it lists changes. See [Templates](../templates/).

## Feeds, sitemap, and robots

Signal generates a family of RSS feeds from normalized site data: the main feed, one feed per collection (section), the taxonomy label-index feed, and one feed per term. It also generates a sitemap from the route inventory and, when `[robots]` is set, a deterministic allow-all `robots.txt`. See [Configuration](../configuration/).

## Search

Signal generates a versioned static `index.json` containing searchable plain-text projections. Signal owns extraction, structural normalization, and serialization only: tokenization, ranking, filtering, and the search UI remain outside the engine. The index is a pure projection of the normalized model, so it rebuilds only when searchable content changes — template, configuration, asset, and fenced-code edits reuse it. `signal explain index.json` reports the artifact's kind, declared query input, document count, and reuse/rebuild decision. See [Search](../search/) for the schema and how to query it.

## Markdown

Markdown is parsed into owned derivatives including HTML, headings, TOC data, code highlighting, Mermaid blocks, alerts, plain text, word count, and URL information. See [Content](../content/).

## Navigation

`menus.main` provides deterministic navigation and per-page active state. Internal menu targets are validated like any other internal reference. See [Configuration](../configuration/) and [Reference validation](../validation/).

## Static assets

Files under `static/` are copied to the output tree as planned static artifacts, covered by collision validation and the manifest like everything else.

## Image derivatives

With `[images]` configured (`widths` plus `format = "webp"` or `formats = ["avif", "webp"]`), content-referenced PNG/JPEG sources gain deterministic resized derivatives — aspect-preserving, never upscaled, one artifact per source × width × format. Rendered pages express them as responsive markup: a single planned format renders a responsive `<img>` (`srcset`, `sizes="100vw"`, intrinsic dimensions); several render `<picture>` with AVIF-first `<source>` elements and a WebP fallback. Front-matter heroes are available to templates as `responsive_image`. See [Configuration](../configuration/).

## Social images

With `[social]` configured, every entry page gains a deterministic `social/<route-path>.png` social image (a sharing card) composed from the site title, the page title, its optional description and author, and its optional PNG/JPEG hero (centred cover crop). It is referenced from `og:image` and `twitter:image` and participates in incremental builds like any other artifact: page metadata, the hero, or the configured dimensions rebuild it, and disabling `[social]` prunes it. `signal explain social/<path>.png` describes the decision. See [Configuration](../configuration/).

## Incremental builds

Signal records source, configuration, template, and output digests in a schema-versioned build manifest, plans per-artifact reuse with reason codes, and explains its decisions on demand. See [Incremental builds](../builds/) and the [CLI reference](../cli/).

## Validation

Internal links, images, front-matter images, and menu targets are validated against the planned output before anything is written; external URLs are never fetched. See [Reference validation](../validation/).

## Diagnostics

`signal check` and `signal explain <asset>` report evidence-based diagnostics over the publishing model: unreferenced raster assets, sources far larger than the largest representation Signal generates from them, configured widths that clamp to the same output, and missing or empty image alternative text. Diagnostics are advisory (`warning` or `info`, never a build failure), state measured facts rather than advice, and come from one shared analysis, so `check` and `explain` always agree. They are not artifacts: nothing is written, pruned, or recorded in the manifest. See [Reference validation](../validation/) and the [CLI reference](../cli/).

## HTML output

Template-rendered pages can be minified with `[output] minify_html = true` (off by default): deterministic, HTML-aware whitespace reduction that preserves `pre`/`code` contents, Mermaid sources, `textarea`, inline scripts and styles, JSON-LD, entities, attributes, and comments. Feeds, sitemap, search index, robots.txt, and static files are never minified. See [Configuration](../configuration/).

## Security

Routes, output paths, author URLs, Markdown HTML, and template execution all have explicit safety boundaries. See [Architecture](../architecture/).

## Deferred

Image cropping and art direction, alternate social-image dimensions or themes, pagination, aliases/redirects, plugins, CMS integration, live reload, and a built-in search UI are deliberately not implemented. Diagnostics are advisory only: no byte-level derivative-size analysis (it needs generated output), no configurable thresholds, and no automatic optimization or rewriting of assets. The [Architecture](../architecture/) page records the full non-goals list and why each conflicts with the engine's guarantees.
