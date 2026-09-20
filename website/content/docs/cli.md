---
title: CLI reference
description: Every Signal command, flag, exit code, and output.
---

Signal has four commands. `serve` reuses the production build pipeline; `check` and `build --explain` are read-only views over the same validation every build requires.

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
```

Exit code is nonzero when the site is invalid. The one documented exception: the output-filesystem alias probe needs an output directory, so it runs in `build`/`--explain` only. Everything else a build rejects, `check` rejects identically. See [Reference validation](../validation/).

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
