---
title: Content
description: Front matter, Markdown features, slugs, drafts, and ordering.
---

Signal content is Markdown with front matter. Each file becomes one normalized content entry: an identity, a route, metadata, and a rendered body.

## Front matter

YAML:

```yaml
---
title: Hello, Signal
description: A short summary.
date: 2026-09-19
lastmod: 2026-09-20
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
| `author` | Entry-specific author (falls back to the site author) |
| `draft` | Exclude the entry from the build |
| `featured` | Make the entry eligible for the home-page hero |
| `image` | Hero image (validated against static assets) |
| `image_alt` | Hero image alternative text |
| `topics` / `tags` | Taxonomy terms, merged (`topics` first) in authored order |
| `slug` | Override the filename-derived slug |

Invalid dates, unsafe routes, and unsupported URL schemes are rejected rather than silently rewritten — see [Reference validation](../validation/).

Topic order is significant: templates render `tags` in the order written (first-occurrence deduplicated), while taxonomy indexes and term grouping use canonical sorted order either way.

Unknown fields are preserved verbatim and exposed to templates through the `extra` context key (see [Templates](../templates/)), so site-specific fields such as `repo` or `eyebrow` need no engine support.

When `[git] last_modified` is enabled, `lastmod` may also be derived from the last commit that touched the source file; an explicit front-matter `lastmod` always wins over the derived value. See [Configuration](../configuration/).

## Slugs and routes

An entry's slug defaults to its filename (`hello-world.md` → `hello-world`); `slug` overrides it. The route is the collection's `route_prefix` plus the slug, always with a trailing slash: `content/posts/hello-world.md` in a collection with `route_prefix = "/posts/"` becomes `/posts/hello-world/`.

## Drafts

`draft: true` excludes the entry from the build entirely: no page, no listing membership, no feed or search presence. The build summary reports how many drafts were skipped.

## Featured entries

`featured: true` makes an entry eligible for the home-page hero. The home page (enabled with `site.home_collection`) renders one featured entry plus recent summaries; see [Templates](../templates/) for the `featured` and `recent` values.

## Sections

A collection can use `_index.md` as its section-root document. When present, its title, description, body, and extras flow into the section listing page; otherwise the listing falls back to the collection's configured `title` and `description`.

## Markdown presentation

Signal renders a deterministic Markdown subset:

- **Headings** gain stable fragment anchors, and a table-of-contents projection is available to templates as `toc`. Duplicate headings receive deterministic unique ids.
- **Fenced code blocks** render with syntax highlighting and a copy affordance. A `mermaid` block keeps its diagram source readable without JavaScript: the escaped source is emitted both inside `pre.mermaid` (the element a client renderer targets) and inside a collapsed `details.mermaid-source` fallback. Signal never renders diagrams itself; the site owns the Mermaid JavaScript, gated per page by the `has_mermaid` template value.
- **Alerts** (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`) render as semantic `aside` callouts with a title label and an ARIA role: `role="alert"` for the urgent `warning`/`caution` kinds, `role="note"` for the advisory kinds. Unknown designators stay ordinary blockquotes.
- **Links and images** use inline Markdown form: `[text](../../posts/deterministic-builds/)` and `![alt](../../favicon.svg)`. Relative destinations and `http(s)` are allowed; executable or unsupported schemes such as `javascript:`, `data:`, and `vbscript:` are neutralized before rendering. Author raw HTML is detached at parse — it is not passed through unchanged.

Internal links, images, and front-matter images are validated against what the build will actually generate; a broken internal reference fails the build before anything is written. See [Reference validation](../validation/).

## URLs

Relative destinations and `http(s)` are allowed. Executable or unsupported schemes such as `javascript:`, `data:`, and `vbscript:` are rejected.

Document-relative links resolve against the page's own route directory, so from `/docs/x/` the reference `../concepts/` means `/docs/concepts/`. Root-relative links (`/posts/a/`) always work. Fragment links (`/posts/a/#install`, or local `#usage`) are checked against the target page's headings.

## Ordering

Generated listings use deterministic newest-first ordering for dated entries, followed by stable ordering for undated entries. Related entries rank by shared-topic overlap with recency tie-breaks.
