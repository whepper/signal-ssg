# ADR 0035: Related-content architecture (A8)

## Status

Accepted (A8 design spike: related entries stay a **page-local shared-tag
projection** over the canonical model, carried by the existing
`Query{related:<route>}` input. No new artifact kind, no new dependency
mechanism, no schema, manifest, or behavior change.)

## Context

Signal already ships related entries (added with the early vertical
slices; see `docs/features.md`, `docs/templates.md`,
`docs/configuration.md`, and `docs/compatibility/gaps.md` item 3): entry
templates receive `related`, the strongest shared-topic overlaps, capped via
`[related] limit` (default 3), ranked by shared-tag count and then the
standard newest-first listing order. It is tracked by a `related:<route>`
query digest, so a page invalidates when any entry it lists changes.

A8 was asked a narrower question than "add related content":

> What is the right source of truth, computation boundary, and dependency
> model for relatedness — and does it need a generated artifact?

Two things make this a real decision rather than a restatement:

1. **The feature already exists and works.** As with search in ADR 0034, the
   question is whether the existing half is the *right* half.
2. **"Related content" can mean two different architectures.** Page-local
   derived data (each page computes its own relationships at render time) and
   a dedicated generated artifact (one `related.json`-style payload consumed
   later) have opposite ownership boundaries, dependency shapes, and future
   costs.

Signal's standing boundary — the canonical model is the source of truth, and
generated output is never an internal API — makes the answer close to forced,
but A8 required evidence rather than assertion.

## Decision

**Source of truth.** The canonical `SiteModel` — the per-entry `tags` set and
the `by_tag` index it builds — plus the entry's own identity. Relatedness is
never derived from rendered HTML, from `index.json`, or from generated
taxonomy pages/feeds.

**Computation boundary.** A pure generator projection:
`signal_generators::related_entries(&SiteModel, &ContentEntry, limit,
date_format) -> Vec<EntrySummary>`. It takes no config object (the cap and the
date format are passed explicitly), performs no I/O, and mutates nothing. The
CLI render context (`entry_context`) calls it and exposes the result to
templates.

**Page-local, not an artifact.** Relatedness stays page-local derived data.
No `ArtifactKind`, no `related.json`, no `InputRef` variant, and no manifest
schema change. A global payload would be a second consumer-facing artifact
with its own versioned contract, would duplicate data the pages already need
at render time, and would serve no requirement Signal has (client-side
discovery is not one). Generated output must not become an internal API, so
neither pages nor a future consumer may read relatedness out of a generated
file.

**Dependency mechanism.** The existing `InputRef::Query{related:<route>}`
input on each `Page` artifact. No new mechanism is introduced. The query
digest is the digest of the exact consumed projection
(`related_entries(...)`), so it moves whenever any listed entry changes and
whenever the effective cap or date format changes.

**Determinism.** Ranking is fully specified and already implemented: candidate
membership is the model's tag index (no hash-map iteration); the score is the
integer count of shared tags; ties resolve through the shared
`compare_by_date_desc` rule (newest date first, then slug ascending, then
`ContentId`); the result is truncated to `limit`. The entry itself and section
roots are never candidates; an untagged entry, an empty model, or no overlap
yields an empty list (the template key is simply absent); a duplicate
candidate cannot appear because scoring is keyed by `ContentId`.

**Rendering contract.** `related` is a `Vec<EntrySummary>` — the same
summary projection listings, heroes, cards, and feeds already use (title,
optional description, raw and formatted date, encoded route, authored-order
tags, optional hero `image`/`image_alt`, reading time). Derived relationship
data is projected into the shared render model; internal generator structures
are never exposed. The key is absent when nothing overlaps, so templates gate
with `{% if related %}`. No change to the template contract is proposed.

## Candidate signals

| Signal | Existing canonical data? | Deterministic? | Useful? | New dependency? | Recommendation |
|---|---|---|---|---|---|
| Tags | Yes — `ContentEntry.tags` (sorted set) + `by_tag` index | Yes | Yes — the primary signal | No | **Use** (current) |
| Date | Yes — `date` (normalized) | Yes | Tie-break (recency) | No | **Use** as tie-break |
| Route / slug | Yes | Yes | Identity and tie-break only | No | Used as identity |
| Collection | Yes | Yes | Weak; candidates are site-wide today | No | Not used; scoping is a possible future option |
| Title | Yes | Yes | Not a similarity signal; part of the projection | No | Projection field only |
| Description | Yes | Yes | Not a similarity signal; part of the projection | No | Projection field only |
| Plain text (`RenderedBody.plain_text`) | Yes | Yes | Would need tokenization and scoring | Yes (algorithm + config) | **Reject** — an engine concern (ADR 0034) |
| Taxonomy membership | Yes — same `by_tag` index | Yes | Identical to tags | No | Reuse the tag index, not taxonomy output |
| Language | Yes (`language`) | Yes | No requirement | No | Not used |
| Semantic references | Field exists but ingestion never populates it | — | Editorial signal would be valuable | Would need front-matter syntax + validation | **Defer** |
| `last_modified`, `author`, `image`, `featured` | Yes | Yes | Not similarity | No | Not used |

## Alternatives considered

- **Metadata-based similarity** (tags + collection + language + recency as a
  weighted score). Adds configuration surface and non-obvious weights for no
  demonstrated requirement. The current shape already folds recency in as a
  deterministic tie-break, which is the useful part. Rejected.
- **Textual similarity** (TF-IDF, embeddings, vector search). Would put a
  ranking engine inside the build graph, require tokenization/stemming and a
  new dependency, and produce scores that are not explainable or cheaply
  reviewable in goldens. Signal owns publishing, not the search application
  (ADR 0034); the same boundary applies here. Rejected.
- **Explicit editorial references.** The model has `references`, but ingestion
  never populates it: there is no front-matter mechanism, and adding one is a
  new content feature, not an architecture choice. Deferred until a real
  requirement appears.
- **Dedicated generated artifact** (`RelatedContent` → `related.json`). Would
  add an artifact kind, a public schema to version, a manifest input family,
  and a second copy of data the pages already compute — with no consumer that
  needs it. It would also invite the exact coupling this ADR forbids (reading
  relationships out of generated output). Rejected.
- **Collection-scoped candidates.** Would change the ranking for a
  presentation preference rather than a requirement. Deferred.
- **`[related] limit = 0` as "disable".** The effective cap treats zero as
  "unset" (default 3), so relatedness cannot be turned off through config
  today. Documented as a known semantic; changing it would change published
  output and is not needed.

## Dependency model

```text
Page(route R)
 ├── Entry{R}                    # R's own digest (title, tags, date, body, …)
 ├── Query{related:R}            # ← the reverse-dependency carrier
 ├── TemplateSet
 ├── Config                     # whole-config, broader than related needs
 ├── Static{…}                  # referenced assets
 └── DerivedImage{…}            # referenced derivatives

Query{related:R}  ≡  digest(related_entries(model, entry_R, related_limit(config), date_format))
```

Mutation effects on the *projection* (`Query{related:R}`):

| Mutation | `related:R` | Reason |
|---|---|---|
| R's tags | changes | membership and score |
| A candidate's title / description / date / route / image / image_alt / reading time | changes | the projection embeds `EntrySummary` fields |
| Any entry's tags (candidate joins/leaves the set) | changes for affected viewers | membership and ranking |
| `[related] limit` | changes | truncation |
| `site.date_format` | changes | `date_formatted` |
| `base_url`, menus, minify, feeds, taxonomy config, `[images]`, `[social]`, site author | unchanged | not consumed by the projection |
| Templates, CSS/static assets, hero/unrelated images, `lastmod`, `featured` | unchanged | not consumed by the projection |
| An unrelated entry (no shared tag) | unchanged | not a candidate |

Note: at the *page* level a config change rebuilds the page anyway, because
`Page` consumes the whole-config digest for unrelated reasons (canonical URLs,
menus). The projection-level boundary above is exact; page-level reuse cannot
isolate relatedness's own config dependence, and the design does not pretend
otherwise.

## Incremental rebuild

Changing entry C propagates without any reverse edge:

1. C's own page rebuilds (`Entry{C}`).
2. Every viewer V whose related set contains C — or whose ranking changes
   because of C — has a different `Query{related:V}` digest and rebuilds. The
   planner recomputes each viewer's projection against the *current* model, so
   the reverse effect is automatic and precise.
3. Membership changes work the same way: creating C, retagging C, or changing
   C's date can move C into or out of a viewer's set, and the viewer still
   rebuilds because its projection is recomputed, not compared to a stored
   edge.
4. Viewers that share no tag with C are untouched (measured by
   `crates/signal-cli/tests/related.rs`).

This is the same "query projection" pattern as `summaries:*`, `home:*`, and
`tagged:*`: a per-artifact forward query whose digest covers exactly the value
the artifact consumes. The existing graph expresses the reverse dependency
correctly; **no extension is required**. The cost is bounded by tag
co-occurrence (each viewer's projection is recomputed once per build), with no
caching or traversal structure.

## Consequences

- Relatedness remains a pure function of the canonical model plus two
  explicitly-passed values (cap, date format); it is independent of the search
  artifact, of generated taxonomy pages/feeds, and of every other generated
  file.
- Because it is page-local, there is no public related-content schema to
  version and no new artifact to keep consistent.
- The projection's rebuild precision is as good as its query digest, which is
  exactly the consumed `Vec<EntrySummary>`.
- The signal is deliberately narrow: tag overlap. Sites that do not tag
  consistently get little or no related content, and very common tags can
  dominate a page's list. Both are accepted trade-offs of an explainable,
  deterministic, dependency-free ranking.

## Deferred

Textual or embedding-based similarity, a related-content artifact or
client-side payload, editorial (explicit) related-content references, shared-tag
score or overlap exposure to templates, collection-scoped candidates, weighted
(IDF-style) tag scoring, and a `[related] limit = 0` "disable" semantics. The
mechanism also cannot serve a *non-entry* artifact (a site-wide discovery page)
without a new query family — a small, existing-pattern extension that is not
needed now.
