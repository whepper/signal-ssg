# Concepts

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

Artifacts can be pages, section indexes, taxonomy pages, RSS feeds (main,
section, taxonomy-label index, and per-term), a sitemap, a robots file, a
search index, a themed not-found page, or static files.

## Assets

An asset is a static file a page embeds: an image, stylesheet, script, or
similar file under `static/`. Content references it from Markdown body
images or the front-matter hero `image`; Signal resolves the reference to
a source path, plans the file as a static artifact, copies it verbatim
into the output, and records the page-to-asset edge in the build plan —
editing the asset rebuilds exactly the pages that embed it.

Source assets (files under `static/`), asset references (authored
strings), generated assets (derived outputs such as resized images),
and output assets (planned specs plus bytes) are distinct concepts.

With `[images]` configured, content-referenced PNG/JPEG sources
additionally gain generated derivatives (`hero-640.webp`,
`hero-640.avif`, …): deterministically resized, never upscaled, tracked
as `DerivedImage` build inputs on both the derivative artifacts and the
embedding pages — one artifact per source × width × format (`format =
"webp"` stays valid for WebP-only sites; `formats = ["avif", "webp"]`
opts into both). Rendered pages express them as responsive markup —
one planned format renders a responsive `<img>` (`srcset` of actual
widths, `sizes="100vw"`, intrinsic dimensions), several render
`<picture>` with AVIF-first `<source>` elements and a WebP fallback —
and heroes are available to templates as `responsive_image`. Cropping and
art direction are future work, not present behavior.

Generated social images (A5, ADR 0032) are a third generated-artifact
kind: when `[social]` is configured, each participating entry page gains
one `social/<route-path>.png` social image built from page metadata plus
an optional raster hero, and the page's `og:image`/`twitter:image`
reference it. It consumes its entry digest, the configuration, and the
hero bytes — never the page's HTML digest.

Diagnostics (A6, ADR 0033) are not a fourth artifact kind. `signal check`
and `signal explain` derive a small set of advisory observations —
unreferenced raster assets, sources far larger than the largest
representation, configured widths that clamp to the same output, and
missing or empty image alt text — from the same model and plan the build
uses. They are measured evidence, not advice: nothing is written, pruned,
or recorded, and no diagnostic can fail a build.

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
