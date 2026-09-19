---
title: Getting started
description: Build your first Signal site.
---

This guide takes you from an empty directory to a generated Signal site.

## Install

Build the CLI from this repository:

```sh
cargo build --release
```

The resulting binary is:

```text
target/release/signal
```

## Create a site

Create:

```text
my-site/
├── content/
│   └── posts/
├── templates/
├── static/
└── signal.toml
```

Start with:

```toml
[site]
title = "My Site"

[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

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
```

Signal accepts both YAML front matter (`---`) and TOML front matter (`+++`).

## Add a template

A minimal entry template is:

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

Set the collection's `template` to its filename when using a non-default template.

## Build

```sh
target/release/signal build --root my-site --out my-site/dist
```

Signal reports planned, reused, rebuilt, and pruned artifacts.

The output is just static files. Any static file server can serve the generated directory.

## Validate

```sh
target/release/signal check --root my-site
```

This validates `signal.toml` and lists the configured collections.

Next: [Concepts](../concepts/), [Configuration](../configuration/), [Content](../content/), and [Templates](../templates/).
