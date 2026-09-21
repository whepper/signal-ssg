# Content

Signal content is Markdown with front matter.

## Front matter

YAML:

```yaml
---
title: Hello, Signal
description: A short summary.
date: 2026-09-19
author: Jane Doe
topics:
  - Rust
  - Static Sites
featured: true
image: /images/hello.png
image_alt: Signal logo
---
```

TOML:

```toml
+++
title = "Hello, Signal"
date = 2026-09-19
topics = ["Rust", "Static Sites"]
+++
```

## Fields

| Field | Purpose |
|---|---|
| `title` | Page title; required |
| `description` | Short summary |
| `date` | Publication date, `YYYY-MM-DD` |
| `lastmod` | Last-modification date |
| `author` | Entry-specific author |
| `draft` | Exclude the entry from the build |
| `featured` | Make the entry eligible for the home-page hero |
| `image` | Hero image |
| `image_alt` | Hero image alternative text |
| `topics` / `tags` | Taxonomy terms, merged (`topics` first) in authored order |
| `slug` | Override the filename-derived slug |

Topic order is significant: templates render `tags` in the order written
(first-occurrence deduplicated), while taxonomy indexes and term grouping
use canonical sorted order either way.

Unknown fields are preserved verbatim and exposed to templates through the
`extra` context key (see [Templates](templates.md)), so site-specific fields
such as `repo` or `toc` need no engine support.

When `[git] last_modified` is enabled, `lastmod` may also be derived from the
last commit that touched the source file; an explicit front-matter `lastmod`
always wins over the derived value.

## Sections

A collection can use `_index.md` as its section-root document.

## Markdown presentation

Signal currently supports deterministic heading anchors and TOC projection, syntax-highlighted code blocks, Mermaid source blocks for client-side rendering, semantic Markdown alerts, and safe author-controlled links and images.

Author raw HTML is not passed through unchanged.

Mermaid blocks keep the diagram source readable without JavaScript: the
escaped source is emitted twice — inside `pre.mermaid` (the element a site's
client renderer targets and replaces) and inside a collapsed
`details.mermaid-source` fallback. Signal never renders diagrams itself; the
site/theme owns the Mermaid JavaScript, gated per page by the `has_mermaid`
context key.

Alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`)
render as semantic `aside` callouts with a title label and an ARIA role:
`role="alert"` for the urgent `warning`/`caution` kinds, `role="note"` for
the advisory kinds. Unknown designators stay ordinary blockquotes.

## URLs

Relative destinations and `http(s)` are allowed. Executable or unsupported schemes such as `javascript:`, `data:`, and `vbscript:` are rejected.

Local image destinations (Markdown body images and the front-matter hero
`image`) are asset references: they must resolve to a file under
`static/`, which Signal copies into the output and tracks as a build
dependency of the embedding page. Missing or escaping references fail the
build; external URLs are passed through and never fetched. When
`[images]` requests derivatives, body images for raster sources
additionally render responsive markup (single format: `srcset` of actual
widths, `sizes="100vw"`, intrinsic dimensions; several formats:
`<picture>` with AVIF-first `<source>` elements and a WebP fallback)
and heroes gain a `responsive_image` template value; without derivatives
the markup stays exactly as authored. When `[social]` is configured,
pages gain a generated social image (a sharing card), referenced from `og:image` and
`twitter:image`; a page opts out with `social_image: false` in front
matter. See
[Concepts](concepts.md) and `docs/adr/0028-first-class-assets.md`.

## Ordering

Generated listings use deterministic newest-first ordering for dated entries, followed by stable ordering for undated entries.
