---
title: Search
description: The static search index Signal generates, its schema, and how to query it in the browser.
---

Signal generates one static search index, `index.json`, from your content. It is a plain JSON document collection: Signal extracts and normalizes the text, and your site — not Signal — owns the search UI, tokenization, ranking, filtering, and highlighting.

There is no search runtime in Signal, no JavaScript, and no query endpoint. The index is an artifact like any other: planned, written, validated, recorded in the manifest, and reused when unchanged.

## What is indexed

One document per regular content entry, site-wide across every collection. Drafts never reach the model, section roots (`_index.md`) are excluded, and generated pages — listings, taxonomy pages, feeds, the sitemap, the 404 page — are not entries, so they are never indexed.

## Schema (version 1)

```json
{
  "version": 1,
  "documents": [
    {
      "id": "/posts/hello-world/",
      "title": "Hello, Signal",
      "url": "/posts/hello-world/",
      "description": "A short summary.",
      "content": "This page is generated from Markdown. …",
      "tags": ["Rust", "Static Sites"],
      "collection": "posts",
      "date": "2026-09-19"
    }
  ]
}
```

| Field | Type | Notes |
|---|---|---|
| `version` | number | Schema version; `1` today. Bumped only for a breaking change. |
| `documents` | array | Every document, ordered by route. |
| `id` | string | Stable document key: the entry's route, URL-encoded. |
| `title` | string | Entry title. |
| `url` | string | Public route, root-relative — resolve it against your site root. |
| `description` | string | Optional. Omitted when the entry has none, never `null`. |
| `content` | string | Normalized plain text (see below). |
| `tags` | string[] | Taxonomy terms, sorted. |
| `collection` | string | Owning collection, for section filtering. |
| `date` | string | Optional `YYYY-MM-DD`. Omitted when undated, never `null`. |

`id` and `url` currently carry the same value: treat `id` as the stable identity and `url` as the link target.

## What `content` contains

`content` is the entry's prose as plain text. Markdown is stripped and whitespace collapsed, but case, accents, punctuation, and non-Latin scripts are preserved exactly as authored, so snippets stay display-faithful.

Included: paragraph prose, headings, list items, table cell text, block-quote and alert text, link labels, image alt text, inline code, and math text.

Excluded: fenced code blocks, indented code blocks, Mermaid sources, raw HTML, the Markdown syntax itself, and every URL.

Signal performs no lexical normalization: it does not fold case, strip accents, stem, or tokenize. That belongs to the client, which keeps one index useful for any matching strategy.

## Using it

Fetch the file and query it however you like:

```js
const index = await fetch("/index.json").then((r) => r.json());
const terms = query.toLowerCase().split(/\s+/);
const hits = index.documents.filter((doc) =>
  terms.every((term) =>
    doc.title.toLowerCase().includes(term) ||
    doc.content.toLowerCase().includes(term)
  )
);
```

That is deliberately naive — ranking, highlighting, fuzzy matching, and the result UI are yours to design.

## Keeping it fresh

The index is a pure projection of the normalized model. It rebuilds exactly when the searchable projection changes — a title, description, body, tag, date, route, or collection edit — and is reused for template, configuration, asset, hero-image, `[images]`, `[social]`, `lastmod`, `featured`, `image_alt`, draft, section-root, and fenced-code-only changes.

`signal explain index.json` reports the artifact's kind, declared query input, document count, and reuse/rebuild decision:

```text
Artifact
========
  path: index.json

Kind:
  SearchIndex

Inputs:
  Query(search_documents)

Documents:
  13

Decision:
  reuse
```

## Deliberate limits

- No built-in search UI, no JavaScript, and no query endpoint.
- No tokenizer, stemming, stop words, or ranking — those stay in the client.
- No partitioning: the whole index is one file.
- No configuration: the index is always generated.

Related entries are a separate, page-local projection over the same canonical model; they render into entry pages at build time and are not part of `index.json`. See [Templates](../templates/).
