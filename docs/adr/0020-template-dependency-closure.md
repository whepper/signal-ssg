# ADR 0020: Template dependency closure via a conservative template-set digest

## Status

Accepted (slice 14C: sound template-driven invalidation; no DAG, no manifest
schema break).

## Context

Before this slice, a template-rendered artifact recorded only the template the
generator directly selected (e.g. `post.html`). Changing a template reached
through `{% extends %}` or `{% include %}` — including nested includes — did
not change any recorded input digest, so incremental reuse could keep stale
output. Slice 13 demonstrated this with `post.html` extending `base.html`:
editing `base.html` rebuilt nothing.

MiniJinja 2.x provides no per-render dependency API. Its loader is invoked
once per template name and compiled templates are cached (`MemoMap`), so a
custom loader cannot observe the templates consumed by one render without
clearing and recompiling the store for every artifact. Reuse decisions also
happen before rendering, so exact closure would additionally have to be
trusted from recorded names rather than recomputed.

## Decision

- Template-rendered artifacts (`Page`, `CollectionIndex`, `Home`, `Taxonomy`)
  depend on the **complete loaded template set** via a new input reference,
  `InputRef::TemplateSet`. Its digest is the deterministic digest of the
  manifest's `templates` map (`name → content digest`).
- All templates under the configured template root are loaded up front, so the
  set is complete and deterministic. Changing any loaded template changes the
  set digest and invalidates every template-rendered artifact; feeds, sitemap,
  search, and static artifacts record no template input and stay reusable.
- The manifest continues to store the full `templates` map; no schema change
  is needed. `InputRef::Template { name }` is retained only so manifests
  written before this slice still deserialize; current builds never emit it.
- No template-source parser is introduced. The dependency is computed from the
  engine's loaded template set, not by scanning `{% extends %}` / `{% include %}`
  syntax.

## Why this is sound

Reuse requires every recorded input digest to match. Any template that can
influence output is in the loaded set, so any change to it changes the set
digest, forcing a rebuild. Dynamic/conditional includes are covered for the
same reason: the template set is the union of all loaded templates, so a
change to a potentially-included template invalidates regardless of whether it
was actually evaluated. A missing template still fails rendering normally, and
the manifest is written only after a fully successful build.

## Precision trade-off (deliberate)

Exact per-artifact closure would rebuild fewer artifacts when an unrelated
template changes. The conservative set rebuilds all template-rendered
artifacts on any template change. That is accepted: correct reuse is more
important than maximal reuse, and the alternative (per-artifact recompilation
plus render-time closure capture, then trusting recorded closures on the reuse
path) is more invasive with no correctness benefit. `template_change_rebuilds_all_template_consumers`
and `unused_template_change_also_rebuilds_template_consumers_by_design`
pin the behaviour.

## Consequences

- `{% extends %}`, `{% include %}`, nested includes, and inheritance+include
  combinations are all soundly invalidated.
- Generation compatibility (ADR 0019) remains a separate whole-build gate and
  is not part of artifact inputs.
- No build DAG or reverse dependency graph exists; template dependencies live
  in the existing per-artifact input list.
