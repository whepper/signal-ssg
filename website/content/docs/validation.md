---
title: Reference validation
description: What Signal checks before writing, and what it deliberately does not.
---

After planning and before writing anything, Signal validates structured internal references against the inventory of what the build will generate. A broken reference fails the build with the output tree and manifest untouched — exactly like invalid content or an output collision.

`signal check` runs the same validation without building (see the [CLI reference](/docs/cli/)). External destinations are classified and skipped: the build never fetches the network.

## What is validated

- **Markdown links** (`[text](/posts/deterministic-builds/)`): the target must be a generated route or a generated file. `/foo` is not silently treated as `/foo/` — canonical trailing-slash routes are matched exactly.
- **Document-relative links** (`../concepts/`, `beta/`): resolved against the source page's route directory. Traversal above the site root is rejected, including percent-encoded traversal.
- **Fragments** (`/foo/#installation`, local `#usage`): the page must exist *and* the fragment must be one of its structural heading ids.
- **Queries** (`/foo/?page=2`): the query never affects existence; the target page must exist.
- **Markdown images** (`![alt](/favicon.svg)`): must resolve to a static asset, not a page.
- **Front-matter images** (`image: images/a.svg`): validated against the static asset set. A missing image fails the build before anything is written.
- **Menu targets**: internal `[menus.main]` destinations must resolve to generated routes, with the same normalization the renderer uses.

Diagnostics name the source file, the source route, the target, and the reason:

```text
broken internal reference in "content/posts/a.md" (route /posts/a/):
  "/projects/foo/": target does not exist
```

Reasons are `target does not exist`, `invalid internal reference` (root escape, or the routeless `404.html`), and `fragment does not exist`.

## Deliberate limitations

These are documented boundaries, not oversights:

- **Reference-style, shortcut, bare-URL, and autolink Markdown forms** render but are invisible to the checker — extraction covers inline link and image form only.
- **Literal URLs inside templates** (scripts, stylesheets) are never parsed; only model-represented menu URLs are checked.
- **Author raw HTML** is detached at Markdown parse, so it contributes no references.
- **Fragments on generated listing/taxonomy/feed pages** are not checked: the renderer may introduce anchors the model cannot see.
- **`404.html`** exists as an artifact but is not a routable page; referencing it is reported invalid rather than silently accepted.
- **External URLs** (`http(s)`, `mailto:`, `tel:`, protocol-relative, other schemes) are skipped, counted only.

Write links in inline form, keep template URLs correct by inspection, and prefer page-level references. The repository's own CI double-checks the generated HTML as a backstop for template-introduced URLs.

## Content rules that follow

Because validation is structural and pre-render:

- Link to canonical routes with trailing slashes (`/posts/a/`, not `/posts/a`).
- Keep images under `static/` and reference the output-relative path (`/images/a.svg` for `static/images/a.svg`).
- Front-matter `image` values are literal paths: `#` and `?` are path characters there, not fragment/query separators.
- Fragments must match a real heading on the target page — renaming a heading breaks inbound fragment links, and the build will tell you.
