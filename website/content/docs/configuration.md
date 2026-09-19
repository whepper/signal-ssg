---
title: Configuration
description: Configure a Signal site with signal.toml.
---

Signal reads `signal.toml` from the site root.

## Site settings

```toml
[site]
title = "My Site"
base_url = "https://example.com/"
home_collection = "posts"
home_template = "home.html"
author = "Jane Doe"
date_format = "%-d %B %Y"
```

`title` is required.

`base_url` is optional and is used where absolute URLs are required, including canonical metadata, feeds, sitemap data, OpenGraph metadata, and JSON-LD.

`home_collection` enables generation of the home page. `home_template` defaults to `home.html`.

`author` is the site-wide default author. `date_format` controls presentation while stored dates remain `YYYY-MM-DD`.

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

Feeds are opt-in. The main feed and taxonomy term feeds are generated from normalized content.

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

## Validation

```sh
signal check
```

Signal rejects invalid dates, unsafe routes, conflicting output paths, and unsupported URL schemes rather than silently rewriting them.
