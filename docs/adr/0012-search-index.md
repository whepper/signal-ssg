# ADR 0012: Search index as a normalized-data projection

## Status

Accepted (slice 9: search index).

## Context

Client-side search needs a static JSON index. The naive source — rendered
HTML — would tangle the index in presentation markup, duplicate the
heading/code/alert work of earlier slices, and create a build dependency
from search onto rendered artifacts (poison for future incremental
manifests).

## Decision

- New `RenderedBody.plain_text`, extracted from the transformed Comrak AST
  in the same parse: prose, headings, and inline code in document order;
  fenced code/Mermaid excluded by rule; alert bodies included as normal
  prose; whitespace-normalized. Never derived from HTML.
- New `SearchDocument`/`SearchIndex` schema (`{version: 1, documents}`),
  projected from `&SiteModel`: regular entries only (section roots out,
  matching the reference's regular-pages behavior), route-ordered, routes
  as both id and url (relative — no `base_url` requirement), optional
  description/date omitted when absent. No ranking/tokenizer/stemming
  config: this slice ships the index, not an engine.
- New `ArtifactKind::SearchIndex` at `index.json`, always generated, no
  opt-in table. No client UI in Signal; any client library is an external
  integration contract against the versioned schema.

## Consequences

- Search input boundary is explicit (`title`, `description`, `plain_text`,
  `tags`, `date`, `route`, `collection`) — future query digests hash data,
  never HTML.
- Pretty-printed JSON keeps goldens reviewable at negligible size cost.

## Deferred

Client search UI, ranking, section-list feeds, menus, image processing.
