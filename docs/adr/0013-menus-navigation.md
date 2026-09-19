# ADR 0013: Configuration-driven menus without SiteModel

## Status

Accepted (slice 10: menus & navigation).

## Context

Templates need a `menus.main` navigation with current-page highlighting.
The tempting placements — deriving links from generated output, or storing
menu state in `SiteModel` — would each couple navigation to something it
does not need: menus are editorial configuration, not content.

## Decision

- Configuration (`[menus.main]` items with `label` + `url`) deserializes
  into input-only `MenuConfig` types. Resolution produces a separate
  render model (`MenuItem { label, url, active }`); raw config never
  reaches templates.
- Resolution is the pure function `ValidatedConfig + Route → Menu` in
  `signal-core::menu`, reusing existing route normalization. `SiteModel`
  is deliberately not involved: menus need no content lookup, and keeping
  them out prevents `SiteModel` from becoming a container for every piece
  of site configuration. No menu item depends on `ContentId`, and items
  are not validated against generated routes.
- Internal URLs normalize to canonical route form (`/posts` → `/posts/`);
  external URLs must be absolute `http(s)` with a host. Anything else —
  missing slashes, `//`, `?`/`#`, whitespace, empty labels, and
  `javascript:`/`data:`/other schemes — fails the build with a `Config`
  diagnostic before any output is planned.
- Active state is exact normalized-route equality only; external items are
  never active. No subtree inference.
- Templates receive `menus.main` (omitted entirely when unconfigured) and
  perform no resolution. Menus create no artifact.

## Consequences

- Navigation is testable from in-memory values: no filesystem, no HTML,
  no network, no model.
- Other menu names in `[menus.*]` are accepted and ignored; only `main`
  is rendered.

## Deferred

Nested menus, subtree-active semantics, existence checks against
generated routes, localization — none required so far.
