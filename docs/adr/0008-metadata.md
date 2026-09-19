# ADR 0008: Presentation metadata boundary

## Status

Accepted (slice 5: metadata + article presentation).

## Context

Article presentation needs human dates, canonical URLs, OpenGraph tags,
JSON-LD, authorship, and hero images. The naive move — preformatted strings
and HTML blobs in the model — would corrupt the normalized layer and smuggle
one site's conventions into the engine.

## Decision

- Stored vs presented: the model keeps machine-readable `YYYY-MM-DD`
  (validated at ingest, leap-day aware); `site.date_format` (strftime-style
  subset, fixed English months) formats at the projection boundary. Raw and
  formatted forms travel together (`date` + `date_formatted`).
- Canonical URLs derive from `base_url` + route in projections; templates
  never reconstruct URLs. Route and canonical stay distinct concepts.
- OpenGraph is explicit `og_*` context keys, each omitted when unavailable.
  No `og:site_name` (no upstream requirement).
- JSON-LD is a serialized `serde_json::Value` built in `signal-generators`,
  with `</` escaped so payloads cannot escape `<script>`. Only available
  fields are emitted — authorship resolves entry → site → omitted.
- Hero images: `image`/`image_alt` promoted to typed fields. Identity
  (site-root URL form, unsafe schemes rejected at ingest) is separated from
  delivery (static passthrough or external URL). No processing pipeline.

## Consequences

- Templates stay declarative (`<link rel="canonical" href=…>`,
  `{{ json_ld | safe }}`); all semantics live in tested Rust.
- `extra` keeps only genuinely untyped fields (`toc`, `math`, `repo`).

## Deferred

Feeds, sitemap, search, heading anchors/TOC, menus, image processing.
