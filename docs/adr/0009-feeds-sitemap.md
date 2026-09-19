# ADR 0009: Feeds and sitemap as serialized projections

## Status

Accepted (slice 6: RSS + sitemap).

## Context

Feeds and sitemap need absolute URLs, machine dates, and valid XML — all
derivable from normalized data. The tempting shortcuts (parse rendered
HTML, scan the output directory, template XML by string concatenation)
each break the architecture or invite escaping bugs.

## Decision

- Generators plan `Rss`/`Sitemap` specs from `&SiteModel` like any other
  projection. Resolution serializes through `quick-xml` event writers
  (balanced tags by construction, escaping at the boundary), bypassing
  MiniJinja — fixed schemas serialize cleaner that way, and the
  plan/resolve/write boundary is unchanged.
- `[feed]` opts in (item selection is editorial); `limit` caps items
  (default 20). Sitemap follows `base_url` with no extra table. Feeds
  without `base_url` fail loudly; sitemap without it skips quietly (no
  promise was made).
- Main feed covers regular entries site-wide; term feeds reuse the
  taxonomy index. The label-list index feed and section-list feeds stay
  out: the former would need non-model dates, the latter add no selection
  semantics.
- Sitemap URLs come from an explicit route inventory (entries, prefixes,
  home, taxonomy). `<lastmod>` is `last_modified`, else `date`, else
  omitted — never filesystem or build time. No `changefreq`/`priority`.

## Consequences

- Feed/sitemap bytes are deterministic for identical inputs (no timestamps
  from the machine anywhere in the pipeline).
- `quick-xml` joins `serde`/`comrak`/`minijinja` as a boundary dependency:
  confined to `signal-generators`, used for writing (and test parsing) only.

## Deferred

Section-list feeds, search index, robots/404.
