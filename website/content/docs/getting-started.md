---
title: Getting started
description: Build your first Signal site, from empty directory to served website.
---

This guide takes you from an empty directory to a generated, served Signal site. By the end you will have created content, templates, and configuration, built the site, inspected the build plan, validated it, and served it locally.

If you prefer reference material first, see [Concepts](../concepts/), [Configuration](../configuration/), [Content](../content/), and [Templates](../templates/). Command-line details live in the [CLI reference](../cli/).

## Install

Build the CLI from the Signal repository:

```sh
cargo build --release
```

The resulting binary is:

```text
target/release/signal
```

Confirm it works:

```sh
target/release/signal --version
```

```text
signal 1.2.0
```

## Create a site

A Signal site is a directory with content, templates, static assets, and a small TOML configuration. Create:

```text
my-site/
├── content/
│   └── posts/
├── templates/
├── static/
└── signal.toml
```

Start with this `signal.toml`:

```toml
[site]
title = "My Site"
base_url = "https://example.com/"

[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

A *collection* is a named set of content with a source directory and a route prefix. Here, every Markdown file under `content/posts/` becomes a page under `/posts/`. Every option is documented in [Configuration](../configuration/).

## Add content

Create `content/posts/hello-world.md`:

```markdown
---
title: Hello, Signal
date: 2026-09-19
description: My first Signal page.
---

# Hello, Signal

This page is generated from Markdown.

Next, read about [deterministic builds](../../posts/deterministic-builds/).
```

Front matter carries the page's metadata (`title` is required); the body is Markdown. Signal accepts both YAML front matter (`---`) and TOML front matter (`+++`). The full field reference is in [Content](../content/).

## Add templates

A minimal entry template is `templates/post.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{{ title }} · {{ site_title }}</title>
</head>
<body>
  <article>
    <h1>{{ title }}</h1>
    {% if date_formatted %}<p>{{ date_formatted }}</p>{% endif %}
    {{ content | safe }}
  </article>
</body>
</html>
```

Templates are MiniJinja files that receive explicit values — `title`, `site_title`, rendered `content`, and much more. `{{ content | safe }}` inserts the already-rendered Markdown body (the `safe` marker is required because HTML templates auto-escape everything else). Which template each page uses, and every available value, is documented in [Templates](../templates/).

You will also want a section template (`templates/section.html`) for the `/posts/` listing page. A minimal one:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{{ title }} · {{ site_title }}</title>
</head>
<body>
  <h1>{{ title }}</h1>
  <ul>
  {% for e in entries %}
    <li><a href="{{ e.route }}">{{ e.title }}</a></li>
  {% endfor %}
  </ul>
</body>
</html>
```

Set the collection's `template` to its filename when using a non-default template.

## Build

```sh
target/release/signal build --root my-site --out my-site/dist
```

Signal reports planned, reused, rebuilt, and pruned artifacts. The output is just static files — any static file server can serve the generated directory. How Signal decides what to rebuild is explained in [Incremental builds](../builds/).

To preview incremental decisions without writing anything:

```sh
target/release/signal build --root my-site --out my-site/dist --explain
```

`--explain` runs the same validation as a build, then prints which artifacts would be reused, which would be rebuilt and why, and which stale outputs would be pruned.

## Serve

```sh
target/release/signal serve --root my-site --out my-site/dist
```

`serve` builds once, serves the output at `http://127.0.0.1:3000/` (`--host`/`--port` override), watches source inputs, and rebuilds with the same pipeline as `build`. Failed rebuilds are reported while the previous output keeps serving. There is no live reload: refresh the browser after a rebuild.

## Validate

```sh
target/release/signal check --root my-site
```

This validates `signal.toml`, lists the configured collections, and checks that internal references (Markdown links/images, front-matter images, menu targets) resolve to generated routes or assets. External URLs are never fetched. What is covered — and what is deliberately out of scope — is documented in [Reference validation](../validation/).

## Next steps

- [Concepts](../concepts/): the site model, artifacts, and determinism.
- [Configuration](../configuration/): every `signal.toml` option.
- [Content](../content/): front matter, Markdown features, ordering.
- [Templates](../templates/): template selection and rendering contexts.
- [CLI reference](../cli/): every command, flag, and exit code.
- [Incremental builds](../builds/): manifests, reuse reasons, pruning.
- [Deployment](../deployment/): publishing the output.
