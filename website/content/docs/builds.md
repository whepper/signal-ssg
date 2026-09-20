---
title: Incremental builds
description: Manifests, reuse rules, rebuild reasons, pruning, and failure semantics.
---

Every successful build writes `.signal/manifest.json`: a schema-versioned, deterministic record of what was built, from what, and with what result. The next build consults it to reuse what is unchanged, rebuild what is not, and prune what no longer belongs.

## What invalidates

Each artifact declares its inputs: content entries, named query projections (listings, feeds, taxonomy, search, related entries), the complete loaded template set, the whole configuration, and static sources. An artifact is reused only when all of the following hold — any uncertainty rebuilds:

- the previous manifest is usable (present, well-formed, current schema);
- the recorded generation identity exactly matches the running binary;
- the manifest record exists under the same path with the same artifact kind;
- the declared input lists are equal;
- every current input digest matches the recorded one;
- the existing output is a regular file whose bytes hash to the recorded output digest.

Timestamps are never consulted — only content hashes and exact bytes. Deleting `.signal/` costs work, never correctness: the next build simply rebuilds everything.

Invalidation is deliberately coarse in two places, both sound by construction:

- **Templates:** any change to the loaded template set invalidates every template-rendered artifact. There is no per-page include-closure tracking.
- **Configuration:** the configuration digest is whole-config; any setting change invalidates configuration-consuming artifacts.

Static assets are precise: one changed file invalidates exactly that file.

## Rebuild reasons

`signal build --explain` reports one reason per rebuild (see the [CLI reference](/docs/cli/)):

| Reason | Meaning |
|---|---|
| `no usable manifest` | First build, or unreadable/corrupt manifest — full build by design |
| `generation changed` | A different Signal version or behavior version built last time |
| `manifest record missing` | New artifact with no previous record |
| `artifact kind changed` | The same path now yields a different kind of artifact |
| `inputs changed` | The dependency structure changed (e.g. a section gained or lost inputs) |
| `entry changed: <route>` | A consumed entry's digest changed |
| `query changed: <key>` | A consumed projection (listing, feed, taxonomy, search, related) changed |
| `template set changed` | Any loaded template changed |
| `configuration changed` | The configuration digest changed |
| `static file changed: <path>` | A static source changed |
| `output missing` / `output changed` | The on-disk output vanished or no longer matches its digest |
| `input error: <message>` | An input could not be digested — fails the decision, never silent |

## Pruning

Stale outputs are `previous manifest − current plan`: artifacts the previous build recorded but the current plan no longer contains. Pruning runs only against a usable previous manifest, never from corrupt metadata, and only after all current artifacts wrote successfully. Missing stale outputs are ignored; stale directories fail safely; unsafe paths are skipped; parent directories are never removed.

## Failure semantics

A failure in resolution, writing, or pruning returns an error and leaves the previous manifest in place, so the next build reconciles from it. Source, template, and static discovery are fail-closed: an unreadable directory aborts the build before any write, prune, or manifest replacement, so an incomplete inventory can never be mistaken for intentional deletion. The manifest is replaced atomically, so a failure never leaves a half-written manifest.

The build is **not** whole-tree transactional: artifacts written before a mid-build failure remain on disk until a later build rewrites or prunes them. Recovery is via the next manifest-driven build — which is also how `signal serve` recovers from failed rebuilds.

## What the manifest is

The manifest is disposable build state, like `.git/`: never publish it (see [Deployment](/docs/deployment/)), never edit it by hand, and never treat it as a source of truth that could make an invalid site look valid. Every plan check runs before the manifest is consulted, so an invalid site fails regardless of what the manifest records.
