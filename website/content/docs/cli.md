---
title: CLI reference
description: Every Signal command, flag, exit code, and output.
---

Signal has five commands. `serve` reuses the production build pipeline; `check`, `inspect`, `explain`, and `build --explain` are read-only views over the same validation every build requires.

## signal build

Build a site root into an output directory.

```sh
signal build --root my-site --out dist
```

| Flag | Default | Meaning |
|---|---|---|
| `--root` | `.` | Site root containing `signal.toml` |
| `--out` | `dist` | Output directory |
| `--explain` | off | Explain the plan without changing anything (see below) |

On success Signal prints a summary:

```text
Signal build complete
  planned: 28
  reused: 12
  rebuilt: 16
  pruned: 0
  pages written: 16
  drafts skipped: 0
  static files: 2
  out: dist
```

Exit code is `0` on success and nonzero on any failure — invalid content or configuration, output collisions, broken internal references, write errors. A failed build leaves the previous manifest in place so the next build reconciles from it.

## signal build --explain

Explain the build plan without writing, pruning, or persisting:

```sh
signal build --root my-site --out dist --explain
```

`--explain` runs the same validation a build requires — including reference validation — then prints which artifacts would be reused, which would be rebuilt and why, and which stale outputs would be pruned. It resolves nothing, writes nothing, and never touches `.signal/manifest.json`. A site the build would reject is rejected by `--explain` identically.

```text
Build plan
==========

Summary:
  artifacts: 28
  reuse:     12
  rebuild:   16
  stale:     0

Reuse:
  css/site.css
  ...

Rebuild:
  posts/beta/index.html
    reason: entry changed: /posts/beta/
  ...

Prune:
  (none)
```

The output is deterministic: explaining twice is byte-identical. Every reason is documented in [Incremental builds](../builds/).

## signal check

Validate `signal.toml` and internal references, and report collections:

```sh
signal check --root my-site
```

| Flag | Default | Meaning |
|---|---|---|
| `--root` | `.` | Site root containing `signal.toml` |

`check` loads configuration, ingests the site, runs the same structural validation as a build (config gates, output-path and route validation, template loading) plus the same reference validation — but resolves no artifacts, writes nothing, prunes nothing, and leaves the manifest untouched:

```text
site: My Site
collection: posts
references: 6 checked (1 external skipped)
assets: 8 discovered (3 referenced, 3 resolved, 0 missing, 0 unsafe; 6 derivatives, 0 social images)
diagnostics: 1 warning, 2 info
  warning: source is 4000×3000; the largest generated representation is 1280px wide
    images/hero.png
  info: no content entry references this asset
    images/old-photo.jpg
```

Exit code is nonzero when the site is invalid. The one documented exception: the output-filesystem alias probe needs an output directory, so it runs in `build`/`--explain` only. Everything else a build rejects, `check` rejects identically. See [Reference validation](../validation/).

`diagnostics` are advisory: evidence-based observations over the publishing model (unreferenced raster assets, oversized sources, widths that clamp to the same output, missing or empty image alt text). They never fail a build, and for a clean site the summary stays one line with no detail. See [Reference validation](../validation/#diagnostics-advisory).

## signal inspect

Inspect one published page's resolved, bounded context as deterministic JSON:

```sh
signal inspect --root my-site --format json /posts/hello-world/
signal inspect --root my-site --format json content/posts/hello-world.md
```

| Argument/flag | Default | Meaning |
|---|---|---|
| `--root` | `.` | Site root containing `signal.toml` |
| `page` | — | Canonical route or model source reference |
| `--format` | `json` | Machine-readable output format; only `json` is currently public |

`inspect` returns the public `signal.inspect/v1` projection: stable source
reference, route and URL, publication state, effective metadata, headings and
fragment ids, resolved outbound links, known inbound links, referenced source
assets, the existing shared-tag related projection, and existing page-scoped
advisory diagnostics. It reuses the same validation and diagnostic analysis as
the other read-only commands. It does not need an output directory, build
artifacts, a manifest, or network access.

Every collection is bounded to 100 items and reports `total` and `truncated`.
The command does not include body HTML/plain text, unknown front matter,
templates, arbitrary repository files, binary assets, or the whole site. Drafts
are not exposed: only entries present in the published model can be selected.
An unknown, draft, or malformed page fails before JSON is emitted.

This is authoritative context, not an editing API. An agent can read and edit
the source normally, use `inspect` to avoid reconstructing resolved routes,
metadata, anchors, and relationships, then run `check` and review the diff.
Editorial suggestions remain the agent's judgement; Signal does not produce a
content-quality score. The repository's `docs/inspection.md` guide contains
the full schema, real fixture output, bounds, and security/privacy boundary.

MCP is not required to use this interface. A client can invoke the CLI
on demand; Signal does not need to run continuously. An MCP adapter is
deferred unless a real client workflow demonstrates that direct CLI access is
materially worse.

## signal explain

Explain the build plan, one asset, one image derivative, one generated social image, or any other planned artifact, without building:

```sh
signal explain --root my-site --out dist
signal explain --root my-site --out dist images/hero.jpg
signal explain --root my-site --out dist images/hero.jpg --width 640 --format webp
signal explain --root my-site --out dist social/posts/example.png
signal explain --root my-site --out dist index.json
```

| Argument/flag | Default | Meaning |
|---|---|---|
| `--root` | `.` | Site root containing `signal.toml` |
| `--out` | `dist` | Output directory (for manifest-aware reuse/rebuild decisions) |
| `target` | — | Source asset, derivative output, social image, or any planned output path (`index.json`, `sitemap.xml`, …). Omit for the whole plan |
| `--width` | — | Explain the derivative at this width (requires `target`) |
| `--format` | `webp` | Derivative format (requires `--width`) |

With no target it prints the same plan as `signal build --explain`. With a target it prints that artifact's measured facts and reuse/rebuild decision, plus any diagnostics about it (assets and derivatives):

```text
Asset
=====
  path: images/hero.png
  ...
Decision:
  reuse

Diagnostics:
  warning: source is 4000×3000; the largest generated representation is 1280px wide
```

A clean asset has no `Diagnostics:` section.

Any planned artifact that is not an asset, derivative, or social image explains by its output path. The search index is the motivating case: `signal explain index.json` describes the planned artifact — its kind, its declared query input, and its document count — and whether it would be reused or rebuilt:

```text
Artifact
========
  path: index.json

Kind:
  SearchIndex

Inputs:
  Query(search_documents)

Documents:
  13

Decision:
  reuse
```

A rebuild names the reason from the same reason model `build --explain` uses (`Decision: rebuild: query changed: search_documents`). `explain index.json` describes the *planned artifact* only; it does not run browser search, tokenize, or rank — that remains the client's concern (see [Reference validation](../validation/) and ADR 0034). The same output-path lookup also explains `sitemap.xml`, `index.xml`, `robots.txt`, `404.html`, and rendered pages.

`explain` is read-only: it resolves nothing, writes nothing, prunes nothing, and never touches the manifest — the same guarantee as `build --explain`.

## signal serve

Serve a site root locally with rebuilds on source changes:

```sh
signal serve --root my-site --out dist
```

| Flag | Default | Meaning |
|---|---|---|
| `--root` | `.` | Site root containing `signal.toml` |
| `--out` | `dist` | Output directory to generate and serve |
| `--host` | `127.0.0.1` | Interface to bind |
| `--port` | `3000` | Port to bind (`0` picks an ephemeral port and reports it) |

`serve` performs a normal production build, serves the output over HTTP, watches `signal.toml`, content, templates, and static inputs (plus `.git` only with opt-in git dates), and rebuilds through the same pipeline as `signal build` on coalesced changes. The output directory itself is never watched, so Signal's own writes cannot retrigger builds.

Failed rebuilds are reported while the previous output keeps serving; the next successful edit rebuilds and recovers. There is no live reload: refresh the browser after a rebuild. Only `GET` and `HEAD` are served; directory routes resolve to their `index.html`; the themed `404.html` is served with a 404 status when configured.
