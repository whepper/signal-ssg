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

## Validation

```sh
signal check
```

Signal rejects invalid dates, unsafe routes, conflicting output paths, and unsupported URL schemes rather than silently rewriting them.
