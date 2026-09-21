---
title: Reference validation
description: What Signal checks before writing, and what it deliberately does not.
---

After planning and before writing anything, Signal validates structured internal references against the inventory of what the build will generate. A broken reference fails the build with the output tree and manifest untouched — exactly like invalid content or an output collision.

`signal check` runs the same validation without building (see the [CLI reference](../cli/)). External destinations are classified and skipped: the build never fetches the network.

## What is validated

- **Markdown links** (`[text](../../posts/deterministic-builds/)`): the target must be a generated route or a generated file. `/foo` is not silently treated as `/foo/` — canonical trailing-slash routes are matched exactly.
- **Document-relative links** (`../concepts/`, `beta/`): resolved against the source page's route directory. Traversal above the site root is rejected, including percent-encoded traversal.
- **Fragments** (`/foo/#installation`, local `#usage`): the page must exist *and* the fragment must be one of its structural heading ids.
- **Queries** (`/foo/?page=2`): the query never affects existence; the target page must exist.
- **Markdown images** (`![alt](../../favicon.svg)`): must resolve to a static asset, not a page.
- **Front-matter images** (`image: images/a.svg`): validated against the static asset set. A missing image fails the build before anything is written.
- **Menu targets**: internal `[menus.main]` destinations must resolve to generated routes, with the same normalization the renderer uses.

Diagnostics name the source file, the source route, the target, and the reason:

```text
broken internal reference in "content/posts/a.md" (route /posts/a/):
  "/projects/foo/": target does not exist
```

Reasons are `target does not exist`, `invalid internal reference` (root escape, or the routeless `404.html`), and `fragment does not exist`.

## Diagnostics (advisory)

Validation answers "is this site correct?" and fails the build when it is not. Diagnostics answer "what looks wasteful or missing?" and never fail anything. `signal check` reports them; `signal explain <asset>` reports the ones about that asset.

```text
diagnostics: 2 warnings, 3 info
  warning: hero image /images/hero.png has no image_alt
    /posts/example/
  warning: source is 4000×3000; the largest generated representation is 1280px wide
    images/hero.png
  info: no content entry references this asset
    images/old-photo.jpg
```

Every diagnostic states a measured fact, carries a stable code (`oversized-source`, `redundant-derivative-width`, `unreferenced-asset`, `hero-alt-missing`, `image-alt-empty`), and has one of two severities:

- **warning** — an actionable inefficiency or content gap: a source far larger than anything Signal publishes, or a front-matter hero with no `image_alt`.
- **info** — an observation that may be entirely intentional: an unreferenced raster asset, configured widths that clamp to the same output, or an empty body-image `alt` (the correct marker for a decorative image).

What Signal deliberately does **not** diagnose:

- **Byte-level efficiency** ("this derivative is larger than its source"). That needs generated output, which `check` does not have; computing it would make `check` encode images or read a possibly-stale output tree, and `check` and `explain` could then disagree.
- **Template-referenced assets.** The model tracks content references, not `<link>`/`<script>` URLs, so only raster assets are reported as unreferenced — CSS, JS, SVG, and favicons are never flagged.
- **Heroes whose template does not render them.** Signal cannot know whether a template uses the hero, so it reports the missing `image_alt` rather than guessing.

Diagnostics are not artifacts: nothing is written, pruned, or recorded in the manifest, and computing them cannot change a single output byte or reuse decision. Conditions that are genuinely wrong — a missing or unsafe reference, a malformed image, an output collision, an invalid configuration — remain hard errors that fail the build exactly as before.

## Deliberate limitations

These are documented boundaries, not oversights:

- **Reference-style, shortcut, bare-URL, and autolink Markdown forms** render but are invisible to the checker — extraction covers inline link and image form only.
- **Literal URLs inside templates** (scripts, stylesheets) are never parsed; only model-represented menu URLs are checked.
- **Author raw HTML** is detached at Markdown parse, so it contributes no references.
- **Fragments on generated listing/taxonomy/feed pages** are not checked: the renderer may introduce anchors the model cannot see.
- **`404.html`** exists as an artifact but is not a routable page; referencing it is reported invalid rather than silently accepted.
- **External URLs** (`http(s)`, `mailto:`, `tel:`, protocol-relative, other schemes) are skipped, counted only.

Write links in inline form, keep template URLs correct by inspection, and prefer page-level references. The repository's own CI double-checks the generated HTML as a backstop for template-introduced URLs.

## A note on hosting under a subpath

Root-relative links (`/posts/a/`) resolve from the domain root. That is correct when the site is served from its own domain, but it breaks on project hosting under a subpath (for example GitHub Pages at `example.github.io/my-site/`, where `/posts/a/` misses the `/my-site/` prefix).

This site itself is hosted under a subpath, so its content links are document-relative (`../builds/`, `../posts/a/`): they resolve against the linking page's route directory and keep working under any base URL, including local `signal serve` and subpath deployments. Prefer document-relative links unless the site will only ever be served from a domain root.

## Content rules that follow

Because validation is structural and pre-render:

- Link to canonical routes with trailing slashes (`/posts/a/`, not `/posts/a`).
- Keep images under `static/` and reference the output-relative path (`/images/a.svg` for `static/images/a.svg`).
- Front-matter `image` values are literal paths: `#` and `?` are path characters there, not fragment/query separators.
- Fragments must match a real heading on the target page — renaming a heading breaks inbound fragment links, and the build will tell you.
