# Deployment

Signal produces static files. There is no runtime service to deploy.

## How a site uses Signal

Signal is a generic engine. A site owns its inputs; Signal consumes them
and writes a generated output tree:

```text
Signal engine
    ↓
site configuration
    ↓
content
    ↓
templates
    ↓
static assets
    ↓
generated output
```

The expected site-level inputs are:

| Input | Purpose |
|---|---|
| `signal.toml` | Site configuration: identity, base URL, collections, taxonomy, feeds, options |
| `content/` | Markdown sources with YAML or TOML front matter (per-collection sources) |
| `templates/` | MiniJinja HTML templates for pages, listings, taxonomy, 404, and feeds hooks |
| `static/` | Files copied verbatim into the output tree (CSS, JS, fonts, images, favicon) |

Nothing else is required, and nothing site-specific lives inside Signal:
all identity, structure, and presentation come from these inputs.

## Static hosting

```sh
signal build --root my-site --out dist
```

Publish the contents of `dist/` using any static hosting service.

`dist/.signal/` is Signal's build state (the incremental-build manifest).
It is disposable, never part of the website, and must **not** be
published: exclude it from the deployment artifact, exactly like `.git/`.
The public website is everything else in `dist/`.

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

For repeatable publishing, build from a known Signal release or revision and treat `dist/` as generated output. Two clean builds of the same site with the same Signal build are byte-identical, including the manifest.
