# Blog compatibility gaps

Concrete Signal capabilities that were missing or insufficient for
reproducing the private Hugo blog's public behavior. Companion to
[blog.md](blog.md); nothing here is satisfied by the theme port alone. Each
item states the observable behavior the blog requires — not an implementation
design.

**Status after the HTML-minification slice:** gaps 1–8 and 10–13 are
resolved (generic engine capabilities, tests, and docs included); gap 9
(CSS pipeline) remains open by design.

## Resolved

1. **✅ Templates see non-typed front matter** — unknown front-matter fields
   survive ingestion on `ContentEntry.extra` and reach entry (and section)
   templates as the `extra` context key, deterministically (`BTreeMap`).
   Covers `toc`, `repo`, and `math` gating without per-field typing.
2. **✅ Git-derived last modification** — `[git] last_modified = true`
   derives `last_modified` from the last commit touching each source file
   (author date); explicit front-matter `lastmod` wins; Git is advisory
   (missing repo/binary → no derived dates, build succeeds). See
   [ADR 0024](../adr/0024-git-last-modified.md). Restores "Updated"
   bylines, JSON-LD `dateModified`, and sitemap `lastmod`.
3. **✅ Related entries** — entry templates receive `related`: up to
   `[related] limit` (default 3) summaries ranked by shared-topic overlap,
   newest-first on ties, section roots and the entry itself excluded.
   Manifest-tracked via a `related:{route}` query digest, so pages
   invalidate when any listed entry changes.
4. **✅ Section and taxonomy-label RSS feeds** — `[feed]` now plans the
   whole family: main feed, one feed per collection (including collections
   whose only member is the section root — an empty feed, matching the
   reference), the taxonomy label-index feed (one item per term, stamped
   with the newest member's date), and per-term feeds. Same `channel_xml`
   machinery, same ordering queries as the HTML listings, per-feed query
   digests for invalidation precision. See
   [ADR 0025](../adr/0025-feed-family-robots-notfound.md).
5. **✅ robots.txt** — `[robots]` opts into a deterministic allow-all
   policy plus a `Sitemap:` line emitted exactly when the sitemap exists
   (base URL configured).
6. **✅ Themed 404 page** — `site.not_found_template = "404.html"` renders
   the named template through the normal pipeline at the conventional
   output path, with site-level context (title, base URL, menus — all
   inactive) and no fabricated canonical URL or route. Inputs are the
   template set and config only, so content edits never rebuild it.
10. **✅ Site-level `description`** — `site.description` is configured once
    and exposed to every template-rendered page as `site_description`,
    the fallback behind a page's own `description`. It rides the existing
    whole-config digest, so changing it rebuilds exactly the artifacts
    that already depend on configuration (template-rendered pages, feeds,
    sitemap) while query-only artifacts like the search index stay reused.
11. **✅ Authored topic order preserved for display — implemented.**
    Front matter keeps `topics` then `tags` in written order
    (first-occurrence deduplicated); `ContentEntry` carries both the
    canonical sorted set (`tags`, for indexing/queries/digests) and the
    display list (`tag_order`). Summaries and the entry-page `tags`
    context key render authored order; taxonomy indexes, term grouping,
    ordering queries, and the search index keep canonical order. Reordering
    alone moves the entry digest, so eyebrows and "first topic" picks
    invalidate correctly.
12. **✅ Per-context date formatting** — templates use the `date_format`
    filter (`{{ date | date_format("%-d %B %Y") }}`) over the same
    strftime-style subset as `site.date_format`. Missing, non-string, and
    invalid dates render as the empty string. Deliberate difference from
    Hugo documented in [Templates](../templates.md#date-formatting):
    Signal uses the strftime-style subset, not Go reference-date layouts.
7. **✅ Alert ARIA roles** — alert asides carry `role="alert"` for
   warning/caution and `role="note"` for the advisory kinds.
8. **✅ Mermaid no-JS fallback** — the escaped diagram source is emitted
   twice: in `pre.mermaid` (the client renderer's target, unchanged) and in
   a collapsed `details.mermaid-source` fallback, so the diagram stays
   meaningful without JavaScript. `has_mermaid` gating is unchanged.
13. **HTML minification (resolved)** — `[output] minify_html = true`
    (off by default) minifies every template-rendered HTML artifact after
    rendering: deterministic, HTML-aware whitespace reduction that
    preserves code blocks, Mermaid sources, textarea, scripts, styles,
    JSON-LD, entities, attributes, and comments. Feeds, sitemap, search
    index, robots.txt, and static files are never minified. Toggling the
    flag rebuilds exactly the artifacts that already depend on
    configuration. Serialization differs from `hugo --minify` by
    implementation, not by semantics; see ADR 0027.

## Open (one remaining)

9. **No CSS/asset pipeline.**
   The blog concatenates its stylesheets, minifies, fingerprints, and serves
   them with SRI integrity attributes. Signal copies `static/` verbatim and
   has no concat/minify/fingerprint/integrity step. Workaround: pre-build the
   bundle externally and accept plain URLs without integrity attributes.

## Explicit non-gaps (accepted, documented differences)

Not listed as work: heading-fragment edge cases (BR-1), reading-time word
counting, same-date ordering tie-breaks (BR-1), raw-HTML removal instead of
escaping, smart-quote rendering, and the search-index JSON schema (the theme's
search JS is adapted to Signal's versioned schema in the port). Features the
blog's theme defines but its content never uses — shortcodes, KaTeX, code
attributes, `eyebrow` — are recorded in
[blog.md](blog.md#present-in-the-theme-never-exercised-by-content) and need no
Signal work today.
