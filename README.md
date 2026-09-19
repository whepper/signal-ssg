# Signal

[![CI](https://github.com/whepper/signal-ssg/actions/workflows/ci.yml/badge.svg)](https://github.com/whepper/signal-ssg/actions/workflows/ci.yml)

> **Beta · 0.1.0-beta.1**

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

Validate a site's configuration with:

```sh
signal check --root my-site
```

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

Template-rendered artifacts depend on the **complete loaded template set**, so inheritance or include changes invalidate affected outputs conservatively. Reuse **consults mtimes** only for diagnostics; mtimes are not part of the reuse predicate.

A failed build leaves the previous manifest in place, but the build is not **whole-tree transactional**: artifacts that were already written are not automatically rolled back.

## Documentation

- [Getting started](docs/getting-started.md)
- [Concepts](docs/concepts.md)
- [Configuration](docs/configuration.md)
- [Content](docs/content.md)
- [Templates](docs/templates.md)
- [Features](docs/features.md)
- [Deployment](docs/deployment.md)
- [Architecture](docs/architecture.md)
- [Architecture Decision Records](docs/adr/)
- [Hugo migration notes](docs/migration/)

## Current status

Signal is in beta. The current release is **0.1.0-beta.1** and compatibility may change before 1.0.

Implemented today include Markdown ingestion, YAML/TOML front matter, collections, home and section pages, topics/taxonomy pages, RSS, sitemap generation, search-index generation, template inheritance, deterministic manifests, hash-based incremental reuse, and hardened output/path handling.

Image processing, pagination, aliases, and plugins are deliberately deferred.

## Development

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## License

Apache-2.0. See [LICENSE](LICENSE).
