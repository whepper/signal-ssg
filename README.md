# Signal

[![CI](https://github.com/whepper/signal-ssg/actions/workflows/ci.yml/badge.svg)](https://github.com/whepper/signal-ssg/actions/workflows/ci.yml)

> **1.0.0**

Signal is a deterministic static site generator written in Rust.

Write your content in Markdown, define your site with templates and a small TOML configuration, and Signal turns it into a completely static website.

```text
content + templates + static + signal.toml
                    ↓
                 signal build
                    ↓
              deterministic output
```

## Why Signal?

Signal is built around a few simple ideas:

- **Content is data.** Markdown is normalized into an owned site model before pages are generated.
- **Deterministic output.** Discovery, ordering, routes, generated data, and manifests are stable.
- **Safe rendering.** Templates run against explicit in-memory contexts with HTML auto-escaping.
- **Incremental builds.** Unchanged artifacts can be reused from the previous build manifest.
- **Static by default.** The result is plain files that can be served from any static host.

## Get started

A Signal site looks like this:

```text
my-site/
├── content/
│   └── posts/
│       └── hello-world.md
├── templates/
│   ├── home.html
│   ├── post.html
│   └── section.html
├── static/
└── signal.toml
```

Build it with:

```sh
signal build
```

Or explicitly choose the site root and output directory:

```sh
signal build --root my-site --out dist
```

Validate a site's configuration and internal references with:

```sh
signal check --root my-site
```

`check` ingests the site and verifies that Markdown links/images,
front-matter images, and menu targets resolve to generated routes or
assets. `signal build` enforces the same invariant before writing.
External URLs are never fetched. Reference-style links, template-literal
URLs, and raw HTML are currently out of scope for validation.

Serve a site locally with rebuilds on source changes:

```sh
signal serve --root my-site --out dist
```

`serve` performs a normal build, serves the output over HTTP
(`http://127.0.0.1:3000/` by default), watches `signal.toml`, content,
templates, and static inputs, and rebuilds through the same pipeline as
`signal build`. Failed rebuilds are reported while the previous output
keeps serving. There is no live reload: refresh the browser after a
rebuild.

## Configuration

The smallest useful configuration is:

```toml
[site]
title = "My Site"
base_url = "https://example.com/"

[collections.posts]
source = "content/posts"
route_prefix = "/posts/"
```

See the [configuration guide](docs/configuration.md) for all currently supported options.

## How builds work

Successful builds write `.signal/manifest.json`. On the next build, Signal can reuse an artifact only when the previous generation is compatible and every recorded input digest still matches. Stale outputs are reconciled as **previous manifest − current plan**.

Template-rendered artifacts depend on the **complete loaded template set**, so inheritance or include changes invalidate affected outputs conservatively. Reuse is a total per-artifact predicate over recorded digests; **mtimes are never consulted**.

A failed build leaves the previous manifest in place, but the build is not **whole-tree transactional**: artifacts that were already written are not automatically rolled back.

Inspect what an incremental build would do without changing anything:

```sh
signal build --root my-site --out dist --explain
```

`--explain` runs the same validation as `build`, then prints which
artifacts would be reused, which would be rebuilt and why, and which
stale outputs would be pruned.

## Documentation

- [Getting started](docs/getting-started.md)
- [Concepts](docs/concepts.md)
- [Configuration](docs/configuration.md)
- [Content](docs/content.md)
- [Templates](docs/templates.md)
- [Features](docs/features.md)
- [Deployment](docs/deployment.md)
- [Release notes](docs/releases/)
- [Architecture](../ARCHITECTURE.md)
- [Architecture Decision Records](docs/adr/)
- [Hugo migration notes](docs/migration/)

## Current status

Signal 1.0 is stable. The current release is **1.0.0**: the pipeline,
manifest schema, and CLI contract documented here are frozen, and future
1.x releases preserve compatibility with them.

Implemented today include Markdown ingestion, YAML/TOML front matter, collections, home and section pages, topics/taxonomy pages, RSS feeds (main, section, taxonomy label-index, and term), sitemap and robots.txt generation, a themed 404 page, search-index generation, related entries, template inheritance, deterministic manifests, hash-based incremental reuse, optional HTML output minification, opt-in Git-derived last-modified dates, and hardened output/path handling.

Image processing, pagination, aliases, and plugins are deliberately deferred.

## Development

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## License

Apache-2.0. See [LICENSE](LICENSE).
