---
title: Why deterministic builds matter
date: 2026-09-19
description: Predictable output makes static publishing easier to reason about, cache, verify, and deploy.
topics:
  - Architecture
  - Determinism
---

# Why deterministic builds matter

A static site is easiest to operate when the same inputs lead to the same outputs.

Signal treats determinism as a build property. Source discovery is sorted, model indexes are ordered, generated artifacts are planned consistently, and the build manifest is deterministic.

That matters beyond aesthetics. Stable output makes it easier to compare builds, reason about changes, cache generated artifacts, and investigate unexpected differences.

## The build model

Signal separates content normalization from artifact generation:

```text
Markdown → SiteModel → ArtifactSpec → rendered bytes
```

Generators query normalized data and plan the artifacts that should exist. They do not inspect previously rendered HTML.

## Incremental reuse

A successful build records input and output digests in `.signal/manifest.json`.

On a later build, Signal can reuse an artifact when its recorded inputs still match and its existing output still matches the recorded digest.

The manifest is build state, not site content. Delete it and the next build simply performs a full build.
