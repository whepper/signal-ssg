# Configuration

Signal reads `signal.toml` from the site root.

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

`description` is the optional site-wide description, exposed to every
template-rendered page as `site_description` for use as the fallback behind
a page's own `description`.

`base_url` is optional and is used where absolute URLs are required, including canonical metadata, feeds, sitemap data, OpenGraph metadata, and JSON-LD.

`home_collection` enables generation of the home page. `home_template` defaults to `home.html`.

`author` is the site-wide default author. `date_format` controls presentation while stored dates remain `YYYY-MM-DD`.

`not_found_template` opts the site into a themed `404.html`: the named
template renders at the fixed output path `404.html` (the convention static
hosts use for unknown paths). The page receives `site_title`, `base_url`,
and `menus` — with no route, no canonical URL, and no active menu item,
because an error page is not one of the site's routes.

## Collections

```toml
[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
template = "post.html"
title = "Posts"
description = "Articles and updates."
```

When omitted, `source` defaults to `content/<name>` and `route_prefix` defaults to `/<name>/`.

## Taxonomy

```toml
[taxonomy]
route_prefix = "/topics/"
title = "Topics"
```

Taxonomy generation is enabled when this section exists.

## Feeds

```toml
[feed]
limit = 20
```

Feeds are opt-in. The feed family covers the main site feed (`index.xml`),
one feed per collection (`<prefix>/index.xml`), the taxonomy label-index
feed (`<root>/index.xml`), and one feed per taxonomy term — all generated
from normalized content under the same `limit`. A collection or term with no
qualifying members still publishes its (empty) feed.

## Robots

```toml
[robots]
```

Presence of the (currently empty) `[robots]` table opts the site into
generating `robots.txt`: a deterministic allow-all policy for every
crawler, plus a `Sitemap:` reference whenever `site.base_url` is configured
and the sitemap therefore exists. Per-agent or disallow rules are not
implemented; a real requirement would extend this table.

## Sitemap

The sitemap needs no configuration beyond `site.base_url`: when a base URL
exists, `sitemap.xml` is generated from the explicit public route inventory
(entries, sections, home, taxonomy). Drafts, feeds, and static assets never
enter the inventory.

## Search index

The search index needs no configuration: `index.json` is always generated
from normalized plain-text projections (title, description, tags, date,
route, plain text) under a versioned schema. Signal owns extraction,
structural normalization, and serialization; tokenization, ranking,
filtering, and the search UI remain outside the engine.

## Images

```toml
[images]
widths = [640, 1280, 1920]
formats = ["avif", "webp"]
```

Opt-in. When `widths` is non-empty, every content-referenced raster
source (PNG, JPEG/JPG, or WebP under `static/`) gains one `{stem}-{width}.{ext}`
derivative per width per format: deterministically resized
(aspect-preserving Lanczos3, never upscaled — requests at or above the
source width emit source dimensions) and encoded (lossless WebP; AVIF at
fixed quality 70 / speed 10 via ravif — see `docs/adr/0031-avif-and-picture.md`).
The supported derivative source formats are PNG, JPEG/JPG, and WebP; the
supported derivative output formats are WebP and AVIF. The legacy singular
`format = "webp"` remains valid for one format;
`formats`, when non-empty, wins over `format`, and setting both is an
error. Unknown formats fail; duplicates are deduplicated; author order
never affects output. Embedding pages and responsive homepage heroes
depend on these derivatives, so editing a source rebuilds exactly its
derivatives and consumers;
removing a format prunes exactly its artifacts while the surviving
format reuses. Unchanged derivatives reuse byte-identically across
builds. SVG, GIF, and other assets are never rasterized and stay
verbatim static outputs. Absent (or empty `widths`) plans nothing and
leaves output byte-identical. Rendered pages embed the derivatives as
responsive markup: one planned format renders a responsive `<img>`
(`srcset` of actual widths, `sizes="100vw"`, intrinsic dimensions);
several render `<picture>` with one `<source type=…>` per format
(AVIF first) and a WebP fallback `<img>`. Entry-page heroes are exposed to
templates as `responsive_image`. The homepage exposes the same resolved value
as `featured.responsive_image`, but only for the actual entry selected by the
featured-hero rule; `featured.image`/`featured.image_alt` remain the fallback
when the hero is absent, external, non-raster, or the image pipeline is off.
`signal explain <asset> --width W [--format F]` describes each derivative's
format-grouped responsive representation, and `signal explain index.html`
lists the selected hero source and every derivative consumed by the homepage.

## Social images

```toml
[social]
width = 1200
height = 630
```

Opt-in. When `[social]` is present, every entry page generates one
deterministic PNG social image (a 1200 × 630 sharing card by default) at
`social/<route-path>.png` (e.g. `/posts/example/` →
`social/posts/example.png`), composed from the site title, the page title,
the optional description, the optional author, and the optional PNG/JPEG
hero image (`image`), which is composited with a centred cover crop. The
font is bundled and compiled in, so social images are identical on every
machine; text wraps deterministically and over-long titles ellipsize
rather than disappear. Entry pages then reference the social image from
`og:image` and `twitter:image` (with `twitter_card = summary_large_image`
and `social_image` — `url`, `absolute_url`, `width`, `height` — available
to templates); the hero still supplies `json_ld` and `image_absolute_url`.
`width`/`height` default to 1200 × 630 and must be 1..=4096. Section roots
and listing pages never get one, and any page can opt out with front-matter
`social_image: false` (or opt in with `true`). Because Open Graph images
must be publicly resolvable, `[social]` requires `site.base_url`;
`enabled = false` keeps the table but plans nothing. Social images
participate in incremental builds exactly like every other artifact:
editing a title, description, hero, or dimension rebuilds the social image
and its page; disabling the feature prunes them and restores hero-based
metadata. See `docs/adr/0032-generated-social-images.md`.

## Output

```toml
[output]
minify_html = true
```

Off by default. When enabled, every template-rendered HTML artifact (entry
pages, section listings, home, taxonomy pages, the themed 404) is passed
through deterministic HTML-aware minification after rendering: insignificant
whitespace collapses, while `pre`/`code` contents (highlighted code blocks,
Mermaid sources), `textarea`, inline scripts and styles, JSON-LD, entities,
attributes, and comments keep their exact semantics. The JavaScript/CSS
minification features of the underlying library stay disabled — inline
scripts and styles are trimmed at most, never reinterpreted.

The flag rides the whole-config digest, so toggling it rebuilds exactly the
artifacts that already depend on configuration (template-rendered pages);
the search index and static files stay reused. Non-HTML artifacts (feeds,
sitemap, search index, robots.txt, static files) are never minified, and
source content is untouched.

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

Internal URLs are validated as internal routes and active state is resolved per page.

## Git metadata

```toml
[git]
last_modified = true
```

Opt-in. When enabled, entries without a front-matter `lastmod` get their
`last_modified` from the last commit that touched the source file (author
date, `YYYY-MM-DD`). Git is advisory: without a repository or the `git`
binary the build proceeds without derived dates. This affects the "updated"
displays, JSON-LD `dateModified`, and sitemap `lastmod`.

## Related entries

```toml
[related]
limit = 3
```

Entry pages receive up to `limit` (default 3) related entries — the
strongest shared-topic overlaps, newest-first within equal overlap. The
projection is always available to entry templates as `related`; this table
only tunes the cap.

## Static assets

`static/` needs no configuration: every file beneath it is planned as a
first-class artifact and copied verbatim to the same relative output path
(`static/css/site.css` → `css/site.css`). Symlinks under `static/` are
skipped. A static file that collides with a generated path fails validation
before anything is written.

## Validation

```sh
signal check
```

Signal rejects invalid dates, unsafe routes, conflicting output paths, and unsupported URL schemes rather than silently rewriting them.

`signal check` also reports advisory **diagnostics** over the publishing model — unreferenced raster assets, sources far larger than the largest representation Signal generates, configured widths that clamp to the same output, and missing or empty image alt text. Diagnostics are `warning` or `info`, never a build failure, and never change output or reuse. See `docs/adr/0033`.
