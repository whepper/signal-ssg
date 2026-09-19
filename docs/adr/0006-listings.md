# ADR 0006: Section and home projections

## Status

Accepted (slice 2: home + section lists).

## Context

Listings must come from the frozen `SiteModel` without a graph engine, a
query DSL, or generator-owned state. The migrated Hugo site orders section
lists `.ByDate.Reverse`, shows the first featured article plus up to 8 latest
on home (`first N` limits — no paginator exists), hardcodes the articles
header text in the template, and renders `_index.md` bodies as section content.

## Decision

- One small model query: `entries_in_collection_by_date` — dated newest
  first, undated last, slug-then-`ContentId` tie-breaks. Deterministic by
  construction; documented on the method.
- `EntrySummary` (title, description, date, route, tags, reading time) as the
  only listing data templates receive. Section roots are excluded from
  summaries and from `EntryPages`; they belong to the section page itself.
- `SectionIndex(collection, prefix)` plans one `CollectionIndex` spec;
  `Home(collection, limit)` plans one `Home` spec. Section title/description
  resolve `_index` entry → collection config → fallback; home renders the
  newest featured entry plus capped recents from `site.home_collection`.
- Promoted to typed data per the extra rule: `ContentEntry.featured` /
  `section_root`, `FrontMatter.featured`, collection `title`/`description` /
  `section_template`, site `home_collection`/`home_template`. Everything else
  stays in `extra`.

## Consequences

- Templates never see the model; generators never touch the filesystem.
- Every collection now always yields a section page; sites without one
  (minimal fixture) gain it via the `section.html` default.
- No pagination framework: proven unnecessary for this site.

## Deferred

Taxonomy pages (slice 4), metadata formatting (slice 5), feeds/sitemap
(slice 6), hero/row image fields with the same promotion rule.
