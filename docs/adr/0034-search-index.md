# ADR 0034: Search index architecture (A7)

## Status

Accepted (A7 design spike: the static search index stays a **document
collection**; Signal ships no tokenizer, no postings, no ranking, and no
UI. No artifact, dependency, or manifest change.)

## Context

Signal already ships a search index (ADR 0012): `index.json`, planned
unconditionally by the `Search` projection, projected purely from the
normalized model, versioned as `{version, documents}`, and consumed by no
part of Signal itself. A7 was asked a narrower question than "add search":

> What is the smallest deterministic publishing artifact that makes
> high-quality client-side search possible for a Signal-generated site?

Two things make this a real decision rather than a restatement:

1. **The publishing half of search already exists and works.** The
   question is whether it is the *right* half, not whether to build it.
2. **A "search index" can mean two very different artifacts.** A corpus of
   documents (the client builds the index) and a precomputed inverted
   index (Signal builds the index) have opposite ownership boundaries,
   dependency shapes, and future costs.

The boundary Signal has held since ADR 0012 — Signal owns the publishing
model, not the application — makes the answer close to forced, but A7
required evidence rather than assertion. Measurements on the real Signal
website (13 documents) and on a Zipfian corpus model of natural language
(100–5000 documents) are in *Measurements* below; the rebuild matrix was
measured against Signal's own planner, not assumed.

## Decision

### 1. A document collection, not an engine

`index.json` stays a versioned JSON array of normalized documents. Signal
owns extraction, structural normalization, projection, serialization, and
versioning. The browser client owns tokenization, case/Unicode folding,
matching, ranking, filtering, highlighting, and presentation.

Signal ships no tokenizer, no stemming, no stop words, no postings, no
term frequencies, and no scores. This is the same boundary ADR 0012 drew
("this slice ships the index, not an engine") and the same one
`ARCHITECTURE.md` §17 records ("a search engine or UI" deferred).

### 2. Artifact identity and planning

- **Kind:** `ArtifactKind::SearchIndex` (existing; no new kind).
- **Path:** `index.json` — a site-wide singleton, no route.
- **Producer:** `signal_generators::search::search_index_json(&SiteModel)`.
- **Planning:** unconditional, like the sitemap; not config-gated like
  feeds, robots, images, or social cards.
- **Format:** pretty-printed JSON `{version, documents}`.

### 3. Dependency model — the narrowest possible

The only declared input is:

```text
InputRef::Query { key: "search_documents" }
```

No `InputRef::Config`, no `InputRef::TemplateSet`, no per-entry edges, no
rendered-HTML digests. The query digest hashes the exact projection the
artifact consumes (`search_documents(model)`), so the index rebuilds
**iff the searchable projection changes**. This is why the index is
config-independent: `url` is relative (no `base_url`), no field formats a
date, and no field depends on `[images]`, `[social]`, or `[output]`.

### 4. Document schema

Unchanged from ADR 0012. Each document is the smallest projection that
both matches and displays:

| Field | Indexed (searchable) | Stored in result | Reason |
| --- | --- | --- | --- |
| `id` | no | yes | Stable document key; the encoded route |
| `url` | no | yes | Result link, relative to the site root |
| `title` | yes (top weight) | yes | Primary match and display |
| `description` | yes | yes | Authored summary; excerpt source |
| `content` | yes | yes | Full-text match; snippet source |
| `tags` | yes | yes | Filter and match; sorted |
| `collection` | no | yes | Section filter and display |
| `date` | no | yes | Display and recency tie-break; omitted when absent |

Deliberately **not** indexed, with reasons that are dependency-shaped, not
aesthetic: adding a field widens the rebuild surface, so a field earns
its place only if it improves search or results.

| Excluded field | Why |
| --- | --- |
| `author` | The entry value is an override; the real fallback is the site author, which needs `Config` and would rebuild the index on every config edit. Not needed to match or display. |
| `last_modified` | Presentation metadata only. Including it would make a `lastmod` edit rebuild the index today it does not (measured). |
| `image`, `image_alt` | Presentation; duplicates model data; not a search signal. |
| `featured` | A home-page projection concern. |
| `language` | i18n is deferred; no multilingual site exists to serve. |
| `extra`, `tag_order`, `references`, `section_root` | Site-specific, presentation, or structural; not search. |
| `body.html`, `headings`, `code_blocks`, `links`, `images` | Rendered HTML and structure are not search text. |

### 5. Normalization boundary — structural, not lexical

Searchable text is `RenderedBody.plain_text`, extracted from the
transformed Comrak AST. Signal normalizes **structure** only:

- Markdown syntax is removed; prose, headings, inline code, link text,
  image alt text, alert bodies, block-quote text, table cell text, and
  math literals survive, in document order.
- Fenced and indented code blocks and Mermaid sources are excluded.
- Author raw HTML is detached at parse; unsafe-URL nodes are unwrapped so
  their text survives.
- Block and line boundaries become single spaces; whitespace is collapsed
  (`split_whitespace().join(" ")`).

Signal performs **no lexical normalization**: no case folding, no
accent/diacritic folding, no stemming, no stop words, no punctuation
removal, no NFC/NFKC. Authored case and Unicode pass through unchanged so
that snippets remain display-faithful and serialization stays
deterministic. Lexical folding is the client's job, applied symmetrically
to the query and to the stored text it already holds.

### 6. Ranking is the client's

Signal emits no scores and no term statistics. It emits **field-separated**
text so the client can weight fields (a reference weighting such as
title ≫ tags > description > content is guidance, not a contract), and
`date` so the client can break ties by recency. Signal will not own
BM25/TF-IDF: that would make the artifact an engine and couple it to one
client.

### 7. One site-wide artifact

`index.json` is not partitioned. Evidence: the real website is 19 KiB
gzip for 13 documents, and even a 1000-document corpus is ~1.4 MiB gzip.
Partitioning would add client complexity (section enumeration,
cross-section queries needing every partition) for no benefit at Signal's
realistic scale. Partitioning remains a future extension the artifact
architecture already admits (one spec per partition with a
collection-scoped query key), to be reconsidered only past a documented
size trigger (see *Consequences*).

### 8. Serialization

Pretty JSON, as today. Measured: pretty vs compact differs by ~1% raw and
~0.01% gzip, so inspectability and golden reviewability win. No custom
binary, no pre-compression (HTTP handles transfer compression), no new
dependency (`serde_json` is already used).

### 9. Client contract

A future browser client consumes exactly `index.json`:

```text
{ "version": 1,
  "documents": [
    { "id": "/posts/alpha/",
      "title": "Alpha",
      "url": "/posts/alpha/",
      "description": "…",        // optional
      "content": "…",            // structural plain text
      "tags": ["Rust"],          // sorted
      "collection": "posts",
      "date": "2026-02-01" } ] } // optional
```

Documents are in route order; optional fields are omitted when absent.
The contract is framework-agnostic: MiniSearch, FlexSearch, Lunr, or a
custom client can all consume it. A client that needs snippets has the
full `content`; a client that needs speed can build its own index once.

### 10. Exclusions reuse the publication model

Only regular entries are indexed: drafts never reach the model, section
roots are filtered, and generated pages (listings, taxonomy, feeds,
sitemap, robots, 404), generated images, social cards, and static assets
are not entries. No second visibility system is introduced.

### 11. Determinism

Documents sort by route; ids are route-derived and encoded; `tags` are a
`BTreeSet` (sorted); serialization is `serde_json` over a struct with
fixed field order; routes are unique by validation, so ids are unique.
There are no timestamps, random values, hash-map iteration, or
environment-dependent inputs. Identical source trees produce
byte-identical `index.json`.

## Measurements

Rebuild decisions measured against Signal's planner on the sample fixture
(`build --explain`, one change per run):

| Change | Search index | Page HTML |
| --- | ---: | ---: |
| title | rebuild | rebuild |
| body prose | rebuild | rebuild |
| body fenced code / Mermaid | **reuse** | rebuild |
| description | rebuild | rebuild |
| tag | rebuild | rebuild |
| date | rebuild | rebuild |
| route / slug | rebuild (+stale) | rebuild (+stale) |
| section-root body | reuse | rebuild (section page) |
| draft edit / new draft | reuse | reuse |
| author / `lastmod` / `featured` | reuse | rebuild |
| front-matter hero image | reuse | rebuild |
| template | reuse | rebuild |
| static asset | reuse | reuse |
| config (`minify_html`, `base_url`, site author, `date_format`, menus, `[feed]`, taxonomy) | **reuse** | rebuild |

Size, gzip, on the real website and a Zipfian corpus (avg 4.6 KB text/doc):

| Corpus | Documents | A: documents (current) | B: inverted index | C: hybrid |
| --- | ---: | ---: | ---: | ---: |
| real website | 13 | 19 KiB | 8 KiB | 8 KiB |
| model | 100 | 137 KiB | 72 KiB | 79 KiB |
| model | 500 | 680 KiB | 301 KiB | 333 KiB |
| model | 1000 | 1.36 MiB | 582 KiB | 645 KiB |
| model | 5000 | 6.8 MiB | 3.35 MiB | 3.66 MiB |

The inverted index is ~45% smaller at scale — real, but not decisive, and
it buys that saving by discarding text (no snippets, no reuse by A8) and
by making Signal own tokenization. The measured saving does not justify
that ownership.

## Consequences

- **No engine, no dependency, no artifact change.** A7 is a contract
  freeze: the ADR, the architecture note, and (in the follow-up
  implementation) observability and determinism guards. Nothing in the
  build changes.
- **Observability (A7.1).** `signal explain <output-path>` describes any
  planned artifact — kind, declared inputs, reuse/rebuild decision — and
  reports the document count for the search index. The record is derived
  from the spec list and `artifact_inputs`, the same values the build
  plans from, so it cannot disagree with the plan.
- **Mechanically guarded (A7.2).** The schema field set and version, the
  route ordering, the structural normalization boundary, the exact input
  set (`Query{search_documents}` and nothing else, read from the recorded
  build graph), the full rebuild matrix, and `build`/`check`/`explain`
  agreement are pinned by `crates/signal-cli/tests/search.rs`. A schema
  field cannot land silently under version 1.
- **The index is cheap to keep narrow.** It already rebuilds only on
  searchable-projection changes and is reused across template, config,
  asset, fenced-code, draft, and section-root changes. Any future field
  addition must be argued against that baseline.
- **Large-site trigger.** If a real site exceeds roughly 1000 documents
  or ~2 MiB gzip, revisit partitioning or an opt-out — with measurements,
  not speculation. Until then, one artifact.
- **A8 stays possible.** Related content can reuse the model's
  `plain_text`, tags, collection, and route without touching the search
  artifact.

## Alternatives rejected

- **Option B — precomputed inverted index.** Forces Signal to own
  tokenization (Unicode, CJK, stemming, stop words), couples the artifact
  to one client algorithm, discards text needed for snippets and A8, and
  makes the client contract bespoke. Measured ~45% smaller at 1000
  documents.
- **Option C — hybrid (thin documents + postings).** Keeps B's ownership
  problem and adds two structures to keep consistent; measured 5–10%
  larger than B and still without snippets.
- **Config-gated or opt-out search.** Would add `InputRef::Config` to the
  artifact, so every config edit would rebuild the index (today none do).
  ADR 0012 deliberately avoided it and no need has appeared.
- **Content truncation / excerpt capping.** Halves size (measured) but
  costs recall and snippet fidelity for a search-first artifact.
- **Partitioned index.** No benefit at realistic Signal scale; adds
  client complexity.
- **Binary or pre-compressed format.** Not justified; harms
  inspectability and golden reviewability.
- **Signal-owned ranking (BM25/TF-IDF).** An engine concern; the client's.

## Deferred

The client itself (UI, runtime, query execution, ranking, highlighting),
partitioning, an opt-out, schema additions, and any search dependency.
A8 (related content) is not designed here; it may reuse the normalized
model, not the search artifact. That design is now recorded in ADR 0035:
relatedness is a page-local shared-tag projection over the canonical
model, independent of this artifact.
