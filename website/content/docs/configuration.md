---
title: Configuration
description: Every signal.toml option, table by table.
---

Signal reads `signal.toml` from the site root. This page documents every table. Start with the smallest useful configuration:

```toml
[site]
title = "My Site"

[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

## Site settings

```toml
[site]
title = "My Site"
description = "Notes on building deterministic static sites."
base_url = "https://example.com/"
home_collection = "posts"
home_template = "home.html"
not_found_template = "404.html"
author = "Jane Doe"
date_format = "%-d %B %Y"
```

`title` is required.

`description` is the optional site-wide description, exposed to every template-rendered page as `site_description` for use as the fallback behind a page's own `description`.

`base_url` is optional and is used where absolute URLs are required, including canonical metadata, feeds, sitemap data, OpenGraph metadata, and JSON-LD. Set it to the final public origin so feeds and canonical tags contain correct absolute URLs — see [Deployment](../deployment/).

`home_collection` enables generation of the home page from that collection's entries (one featured hero plus recent summaries). `home_template` defaults to `home.html`.

`author` is the site-wide default author, used when an entry sets no `author` of its own.

`date_format` controls presentation while stored dates remain `YYYY-MM-DD`. It accepts a strftime-style subset (`%Y`, `%m`, `%d`, `%-d`, `%B`, `%b`, `%%`); see [Templates](../templates/). Formatting never depends on machine locale or timezone.

`not_found_template` opts the site into a themed `404.html`: the named template renders at the fixed output path `404.html` (the convention static hosts use for unknown paths). The page receives `site_title`, `base_url`, and `menus` — with no route, no canonical URL, and no active menu item, because an error page is not one of the site's routes.

## Collections

```toml
[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
template = "post.html"
section_template = "section.html"
title = "Posts"
description = "Articles and updates."
```

When omitted, `source` defaults to `content/<name>` and `route_prefix` defaults to `/<name>/`.

- `template` selects the entry template (default `post.html`).
- `section_template` selects the listing template (default `section.html`).
- `title` and `description` describe the section listing and fall back behind a section-root document (`_index.md`) when one exists.

Each collection produces one entry page per Markdown file plus one section listing page. Route prefixes must be safe, normalized route segments; invalid prefixes fail the build fast — and `signal check` rejects them identically.

## Taxonomy

```toml
[taxonomy]
route_prefix = "/topics/"
title = "Topics"
index_template = "topics.html"
term_template = "topic.html"
```

Taxonomy generation is enabled when this section exists. It produces an index page listing every term (label, route, member count) plus one page per term with its member summaries, newest-first. Terms come from entry `topics`/`tags`. The index and term template names default to `topics.html` and `topic.html`.

## Feeds

```toml
[feed]
limit = 20
```

Feeds are opt-in and additionally require `site.base_url` for absolute item URLs — an explicit-but-unsatisfiable feed configuration fails the build rather than silently dropping promised feeds. The feed family covers the main site feed (`index.xml`), one feed per collection (`<prefix>/index.xml`), the taxonomy label-index feed (`<root>/index.xml`), and one feed per taxonomy term — all generated from normalized content under the same `limit`. A collection or term with no qualifying members still publishes its (empty) feed.

## Robots

```toml
[robots]
```

Presence of the (currently empty) `[robots]` table opts the site into generating `robots.txt`: a deterministic allow-all policy for every crawler, plus a `Sitemap:` reference whenever `site.base_url` is configured and the sitemap therefore exists. Per-agent or disallow rules are not implemented; a real requirement would extend this table.

## Sitemap

The sitemap needs no configuration beyond `site.base_url`: when a base URL exists, `sitemap.xml` is generated from the explicit public route inventory (entries, sections, home, taxonomy). Drafts, feeds, and static assets never enter the inventory.

## Search index

The search index needs no configuration: `index.json` is always generated from normalized plain-text projections (title, description, tags, date, URL, plain text) under a versioned schema. Search UI and search-engine behavior remain outside the engine.

## Output

```toml
[output]
minify_html = true
```

Off by default. When enabled, every template-rendered HTML artifact (entry pages, section listings, home, taxonomy pages, the themed 404) is passed through deterministic HTML-aware minification after rendering: insignificant whitespace collapses, while `pre`/`code` contents (highlighted code blocks, Mermaid sources), `textarea`, inline scripts and styles, JSON-LD, entities, attributes, and comments keep their exact semantics. The JavaScript/CSS minification features of the underlying library stay disabled — inline scripts and styles are trimmed at most, never reinterpreted.

The flag rides the whole-config digest, so toggling it rebuilds exactly the artifacts that already depend on configuration (template-rendered pages); the search index and static files stay reused. Non-HTML artifacts (feeds, sitemap, search index, robots.txt, static files) are never minified, and source content is untouched.

## Menus

```toml
[menus.main]
items = [
  { label = "Home", url = "/" },
  { label = "Posts", url = "/posts/" },
  { label = "Topics", url = "/topics/" },
  { label = "Source", url = "https://github.com/example/project" },
]
```

Internal URLs are validated as internal routes and active state is resolved per page: the item whose normalized route equals the current page's route renders active. Templates receive the resolved menu as `menus.main`; see [Templates](../templates/). Internal menu targets must resolve to generated routes — a dangling menu entry fails the build just like a broken Markdown link. See [Reference validation](../validation/).

## Git metadata

```toml
[git]
last_modified = true
```

Opt-in. When enabled, entries without a front-matter `lastmod` get their `last_modified` from the last commit that touched the source file (author date, `YYYY-MM-DD`). Git is advisory: without a repository or the `git` binary the build proceeds without derived dates. This affects the "updated" displays, JSON-LD `dateModified`, and sitemap `lastmod`.

## Related entries

```toml
[related]
limit = 3
```

Entry pages receive up to `limit` (default 3) related entries — the strongest shared-topic overlaps, newest-first within equal overlap. The projection is always available to entry templates as `related`; this table only tunes the cap.

## Static assets

`static/` needs no configuration: every file beneath it is planned as a first-class artifact and copied verbatim to the same relative output path (`static/css/site.css` → `css/site.css`). Symlinks under `static/` are skipped. A static file that collides with a generated path fails validation before anything is written.

## Images

```toml
[images]
widths = [640, 1280, 1920]
formats = ["avif", "webp"]
```

Opt-in. When `widths` is non-empty, every content-referenced PNG/JPEG source gains one `{stem}-{width}.{ext}` derivative per width per format: deterministically resized (aspect-preserving, never upscaled — requests at or above the source width emit source dimensions) and encoded (lossless WebP; AVIF at fixed settings). The singular `format = "webp"` remains valid for WebP-only sites; `formats`, when non-empty, wins over `format`, and setting both is an error. Editing a source rebuilds exactly its derivatives and the pages that embed them; removing a format prunes exactly its artifacts while the surviving format reuses; unchanged derivatives reuse byte-identically. SVG, GIF, and other assets are never rasterized. Absent (or empty `widths`) plans nothing. One planned format renders a responsive `<img>` (`srcset`, `sizes="100vw"`, intrinsic dimensions); several render `<picture>` with one `<source>` per format (AVIF first) and a WebP fallback `<img>`.

## Social images

```toml
[social]
width = 1200
height = 630
```

Opt-in. When `[social]` is present, every entry page generates one deterministic PNG social image (a 1200 × 630 sharing card by default) at `social/<route-path>.png`, composed from the site title, the page title, its optional description and author, and its optional PNG/JPEG hero image (centred cover crop). The bundled font is compiled in, so social images are identical on every machine. Entry pages reference the social image from `og:image` and `twitter:image` (`twitter_card = summary_large_image`), and templates can read `social_image` for `url`, `absolute_url`, `width`, and `height`; the hero still supplies `json_ld` and `image_absolute_url`. Section roots and listing pages never get one, and a page opts out with front-matter `social_image: false`. Because Open Graph images must be publicly resolvable, `[social]` requires `site.base_url`; `enabled = false` keeps the table but plans nothing. Social images participate in incremental builds like every artifact: editing a title, description, hero, or dimension rebuilds the social image and its page; disabling the feature prunes them and restores hero-based metadata. See `docs/adr/0032-generated-social-images.md`.

## Validation

```sh
signal check
```

Signal rejects invalid dates, unsafe routes, conflicting output paths, and unsupported URL schemes rather than silently rewriting them. `signal check` evaluates the same validity conditions as `signal build` without writing anything, and also reports advisory **diagnostics** over the publishing model — unreferenced raster assets, oversized sources, widths that clamp to the same output, and missing or empty image alt text (`warning` or `info`, never a build failure). See [Reference validation](../validation/) and the [CLI reference](../cli/).
