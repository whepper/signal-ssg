# Getting started

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

### Release binaries

Push a tag such as `v1.0.0` to publish prebuilt binaries to the repository's
[GitHub Releases page](https://github.com/whepper/signal-ssg/releases). The
release workflow provides archives for:

- Linux: `x86_64-unknown-linux-gnu`
- macOS Intel: `x86_64-apple-darwin`
- macOS Apple Silicon: `aarch64-apple-darwin`
- Windows: `x86_64-pc-windows-msvc`

Each archive includes the `signal` executable, `LICENSE`, and `README.md`. The
release also includes a `SHA256SUMS` file for checksum verification. To use a
release archive, download it, extract it, and place the resulting executable
somewhere on your `PATH`.

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

To preview incremental decisions without writing anything:

```sh
target/release/signal build --root my-site --out my-site/dist --explain
```

## Serve

```sh
target/release/signal serve --root my-site --out my-site/dist
```

`serve` builds once, serves the output at `http://127.0.0.1:3000/`
(`--host`/`--port` override), watches source inputs, and rebuilds with
the same pipeline as `build`. Failed rebuilds are reported while the
previous output keeps serving. There is no live reload: refresh the
browser after a rebuild.

## Validate

```sh
target/release/signal check --root my-site
```

This validates `signal.toml`, lists the configured collections, and checks
that internal references (Markdown links/images, front-matter images, menu
targets) resolve to generated routes or assets. External URLs are never
fetched.

Next: [Concepts](concepts.md), [Configuration](configuration.md), [Content](content.md), and [Templates](templates.md).
