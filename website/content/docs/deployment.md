---
title: Deployment
description: Publish the static output produced by Signal.
---

Signal produces static files. There is no runtime service to deploy.

## Static hosting

```sh
signal build --root my-site --out dist
```

Publish the contents of `dist/` using any static hosting service.

## GitHub Pages

The repository's own website is designed to be built by Signal and published through GitHub Pages. This makes the project itself a living example of a Signal site.

## Base URL

Set:

```toml
[site]
base_url = "https://example.com/"
```

Use the final public origin here so canonical metadata and generated feeds can contain the correct absolute URLs.

## Reproducible builds

For repeatable publishing, build from a known Signal release or revision and treat `dist/` as generated output.
