# ADR 0026: Hero image and alt text in entry summaries

## Status

Accepted (theme-port slice: presentation data in projections).

## Context

The theme port exposed that `EntrySummary` — the single projection behind
home heroes, article cards, section listings, term pages, and related
entries — carried no image data. The reference blog's home hero renders the
featured article's hero image, and its cards can do the same. Templates had
the full entry's `image`/`image_alt` only on entry pages, so every listing
surface silently dropped heroes: the port rendered a decorative fallback
where the reference shows the article image.

## Decision

- `EntrySummary` gains `image: Option<String>` and `image_alt:
  Option<String>`, populated from the entry through the existing
  `signal_core::image_src_url` serialization rule (the single helper
  extracted from the entry-context logic, so summary and page markup can
  never disagree on encoding). Absence omits the keys, per the
  never-fabricate convention.
- No new query keys or input kinds: summaries already flow into the
  existing `summaries:*`, `tagged:*`, `related:*`, `home:*`, `feed:*`, and
  `feed:labels` digests, so image edits invalidate exactly the artifacts
  that embed them (listing, term, related, feed, and label projections).
  A dedicated digest test pins this.
- `GENERATION_BEHAVIOR_VERSION` bumped to `7`; the sample-site frozen
  manifest regenerated.

## Consequences

- Heroes, cards, and listings render images from the shared projection;
  no template reaches back into full entries.
- Sites whose templates ignore the new keys see byte-identical HTML but
  new query digests on first rebuild (one clean invalidation, then reuse).

## Deferred

Responsive image variants (`srcset`/sizes) and any image processing remain
out of scope (gap #9 covers the asset pipeline question).
