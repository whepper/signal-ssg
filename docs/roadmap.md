# Roadmap

Post-audit roadmap (A9). This document reconciles the historical
milestone plan with what the repository actually implements, and records
only genuine remaining work. It is deliberately status-driven: milestone
numbers are not preserved for continuity, and an item is listed as
remaining only when repository evidence shows it is missing.

The repository is the source of truth. See `ARCHITECTURE.md` for the
current design, `docs/adr/` for decisions, and the website under
`website/` for user-facing documentation.

## Status legend

| Status | Meaning |
|---|---|
| DONE | Implemented, documented, and tested |
| HARDENING | Works, but invariants or documentation could be strengthened |
| DORMANT | Model or code exists with no ingestion path or consumer |
| MISSING | Not implemented; no requirement today |
| OBSOLETE | Superseded; should not be resurrected |

## Completed milestones

Every milestone A1–A9 is implemented and tested. Nothing below is pending.

| Milestone | Scope | Status | Record |
|---|---|---|---|
| A1 | First-class assets: source assets as declared build inputs | DONE | ADR 0028 |
| A2 | Image derivatives: deterministic WebP resizing | DONE | ADR 0029 |
| A3 | Responsive images: `srcset`/`sizes`/dimensions | DONE | ADR 0030 |
| A4 | AVIF derivatives and `<picture>` sources | DONE | ADR 0031 |
| A5 | Generated Open Graph/Twitter social images | DONE | ADR 0032 |
| A5.1 | Asset and publishing architecture review | DONE | ADR 0028–0032 |
| A6 | Asset and image diagnostics (`check`/`explain`) | DONE | ADR 0033 |
| A7 | Search architecture spike (document collection, no engine) | DONE | ADR 0034 |
| A7.1 | Search contract and `explain` observability | DONE | ADR 0034 |
| A7.2 | Search contract guardrails | DONE | ADR 0034 |
| A8 | Related-content architecture spike (page-local projection) | DONE | ADR 0035 |
| A9 | Capability, documentation, and website audit | DONE | this document |

Related content, the search index, diagnostics, and the full image
pipeline predate or were completed across these milestones; none of them
is outstanding.

## Remaining work

### HARDENING

- **Two user-documentation surfaces.** `docs/` and `website/content/docs/`
  both carry user documentation (getting started, concepts, content,
  configuration, templates, features, deployment) and have already
  drifted in places. Decide a single maintained source per audience and
  cross-link, or generate one from the other. Until then, changes must be
  applied deliberately to both.
- **`docs/` reference parity.** `docs/configuration.md` documents the
  options; the website additionally documents the no-configuration
  artifacts (sitemap, search index, static assets). Keep the two in step.

### DORMANT

These exist in the model or code but are never populated or consumed.
Either wire them when a real requirement appears, or remove them.

- **`ContentEntry.language`** — no front-matter path, no consumer.
- **`ContentEntry.translation_group`** — no front-matter path; the
  `translations_of` query is unused.
- **`ContentEntry.references`** — validated and queryable, but ingestion
  never populates it. This is the natural substrate for editorial
  related-content links (ADR 0035, Deferred).

Each is deliberately excluded from entry digests (no consumer can be
affected), and a build test pins that exclusion.

## Deliberately deferred (no requirement today)

These are documented boundaries, not backlog items. Implementing any of
them needs a concrete requirement; none should be built speculatively.

- **Search client** — UI, tokenization, ranking, highlighting. Signal
  publishes `index.json`; the client is the site's (ADR 0034).
- **Pagination** — listings render every member.
- **Aliases and redirects** — no route alias mechanism.
- **Plugins / extensions** — no extension point (ADR 0004).
- **CMS integration and remote content** — local sources only.
- **i18n** — no locale routing; `language`/`translation_group` are dormant.
- **Image cropping and art direction** — derivatives resize only.
- **Alternate social-card dimensions or themes** — one card layout.
- **Further image codecs beyond WebP/AVIF**, per-format encoder settings,
  and remote image services.
- **A graph traversal engine, per-artifact template-closure precision,
  field-level template tracking, and async builds** — see ADR 0020 and
  `ARCHITECTURE.md` §17.

## Notes on historical roadmap items

- A5.1 and A7.2 were documentation/hardening exercises, not features.
- A7, A8, and A9 were design, observability, guardrail, and audit
  milestones; the capabilities they examine (search index, related
  entries) were already implemented before them.
- No historical milestone turned out to be OBSOLETE or to require a
  redesign; no ADR has been superseded.
