# ADR 0007: Topics taxonomy presentation

## Status

Accepted (slice 3: topics taxonomy).

## Context

The migrated site has one taxonomy (Hugo `topics`) with a handful of live
terms across two collections. Term URLs fold case (`Hello World` →
`hello-world`) while
labels keep it; the terms index sorts case-insensitively; term pages list
members newest-first site-wide. The model's `tags` index already stores the
substrate.

## Decision

- No terminology split: `tags` stays the model concept; one `[taxonomy]`
  config table presents it (root, title, index/term templates). A second
  taxonomy would extend the config first — no generic framework now.
- `slugify` in `signal-core` owns label→slug folding, with per-term tests
  against Hugo's observed output. Label and slug are separate fields on
  `TopicSummary`; only the slug enters routes.
- Queries reused/extended minimally: existing `by_tag` index, new
  `entries_tagged_by_date` (shared date comparator) and `all_tags`.
  Case-insensitive term ordering lives in the generator (`topic_terms`),
  next to the slug-collision error.
- Templates mirror Hugo's split: `topics.html` (index with counts),
  `topic.html` (term rows reusing `EntrySummary` markup).

## Consequences

- `/topics/` + 6 term URLs, labels, order, counts, and member order are all
  parser-verified identical to Hugo.
- Empty terms are unrepresentable (terms derive from entries), matching Hugo.
- Drafts cannot leak into taxonomy (they never enter the model).

## Deferred

Taxonomy feeds (slice 6 with other feeds); any second taxonomy.
