# ADR 0025: Completing the feed family, opt-in robots.txt, and the themed 404

## Status

Accepted (blog compatibility slice 2: artifact-set completion).

## Context

The real-build comparison against the external compatibility blog pinned the
remaining artifact-set gaps precisely: Hugo publishes RSS at
`<section>/index.xml` (including for a section whose only member is its
`_index.md`) and at `<taxonomy>/index.xml` (the label listing, one item per
term stamped with the newest member's date), serves a template-derived
`robots.txt` with an absolute `Sitemap:` line, and renders a themed
`404.html` through the site layout. Signal generated the main feed and
per-term feeds only, had no robots concept, and its artifact model had no
representation for a rendered page that is not a route.

## Decision

- **Feeds.** `[feed]` now plans the whole feed family: the main feed, one
  `SectionFeeds` spec per configured collection (`<prefix>/index.xml`), and
  — with `[taxonomy]` — one label-index feed (`<root>/index.xml`) inside
  `TaxonomyFeeds` alongside the existing term feeds. All four serialize
  through the existing `channel_xml`/`FeedItem` machinery; item selection
  reuses the same model ordering queries the HTML listings use (section
  membership via `entries_in_collection_by_date`, label items via
  `topic_terms`), so feeds and pages can never disagree on membership or
  order. Channel copy follows the one scoped pattern the reference uses
  ("{Scope} on {Site}" / "Recent content in {Scope} on {Site}"), and the
  section feed's scope resolves through the same `section_title` rule the
  section page context uses (shared helper, extracted to avoid a second
  implementation). Empty sections publish empty feeds — that is what the
  reference does, and an absent feed is less predictable than an empty one.
- **Feed dispatch.** A single `feed_identity(path, config)` function (in
  the shared-derivations module alongside template selection) maps an RSS
  output path to its kind: main, section (by collection route prefix),
  taxonomy-labels, or term (by slug). Resolution and manifest recording
  both go through it. Taxonomy checks run before collection-prefix checks
  so a taxonomy rooted inside a collection prefix still dispatches
  correctly. Query keys `feed:section:{collection}` and `feed:labels` digest
  the consumed item projections, so invalidation precision follows the
  data: a tag edit rebuilds the label feed (new term) but not the section
  feed (items carry no tags); an unrelated title edit rebuilds neither feed
  it is not a member of.
- **robots.txt.** `ArtifactKind::Robots` plus an empty presence-gated
  `[robots]` table. Content is a fixed deterministic allow-all policy, plus
  `Sitemap: <base>/sitemap.xml` only when `site.base_url` exists — the
  same condition under which the sitemap is planned, so robots never
  references an artifact that is not there. Per-agent/disallow rules stay
  out until a real site requires them.
- **Themed 404.** `ArtifactKind::NotFound`, planned at the fixed output
  path `404.html` when `site.not_found_template` names a template. A new
  kind rather than a `Page`: a 404 has no entry behind it, no route (the
  hosting layer maps unknown paths to it), no canonical URL, and no
  active-menu state — modeling it as a routeless, contentless rendered
  artifact makes those absences structural, not special-cased. Its context
  is site-level only (`site_title`, `base_url`, `menus` resolved against
  the never-matching `/404.html` so items render inactive), and its inputs
  are `TemplateSet` + `Config`: content edits never rebuild it. The output
  path is not configurable (static-host convention), the template is.
  Redirects, aliases, and other special pages remain out of scope; a second
  concrete case would generalize this pattern, a third might name it.

## Consequences

- `[feed]` sites gain new artifacts on rebuild (section + label feeds);
  main/term feed bytes are unchanged. `GENERATION_BEHAVIOR_VERSION` was
  bumped to `5`, so the first post-upgrade build resolves everything fresh.
- A collection routed at `/` now collides with the main feed at
  `index.xml` (and with the home page at `index.html`, as before) —
  collision validation fails the build with a clear diagnostic, never
  silently drops either feed.
- The sample-site fixture opts into `[robots]`, `not_found_template`, and
  the feed family, so the golden tree covers all of it; the minimal fixture
  stays unconfigured, proving every addition is genuinely opt-in.

## Deferred

Author feeds, feed copy customization, per-agent robots rules, and any
general special-page registry.
