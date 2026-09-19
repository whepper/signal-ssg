# ADR 0022: Current/current filesystem-alias collision detection

## Status

Accepted (slice 15B; remediation of the fresh-review finding R14-2).

## Context

Output-collision validation compared logical output-path strings only. On a
case-insensitive or Unicode-normalizing filesystem two distinct logical
paths can resolve to one object (`posts/Foo/index.html` vs
`posts/foo/index.html`; NFC vs NFD `café`; `static/Index.json` vs the
generated `index.json`). Signal accepted both artifacts, wrote both, let one
silently overwrite the other, and recorded two manifest records — one with a
digest the filesystem contradicted. Slice 14D had protected the
stale-vs-current edge with filesystem identity; the current-vs-current edge
was explicitly deferred and has now been demonstrated end to end.

## Decision

- **Validation, not route redesign.** Logical route and slug rules are
  unchanged: `Foo` and `foo` remain distinct Signal routes. What is new is a
  pre-write check that the configured output filesystem can represent the
  whole plan distinctly, and a build failure when it cannot.
- **The filesystem is the oracle.** `BuildPlan::validate_output_paths`
  keeps its exact-duplicate check, then replicates the planned tree — in
  sorted order — into a transient probe directory inside the output root
  (hence the same filesystem) and reports the first pair of distinct logical
  paths that resolve to one object, as `BuildError::OutputCollision` naming
  both paths. No case-folding or Unicode-normalization tables are
  introduced; whatever the volume folds — case, normalization, or both — is
  observed directly.
- **Identity vs pathname (§17 question).** `(device, inode)` alone cannot
  validate non-existent targets, which is the common clean-build case, so
  identity is *not* the decision procedure here. It is used only inside the
  probe to attribute an already-proven collision to the earlier planned path.
  Because the probe directory starts empty, pre-existing hard links in the
  output tree can never be mistaken for aliases: two hard-linked names are
  representable and still build.
- **Ordering.** Plan → logical validation → filesystem-alias validation →
  resolve/reuse/write → prune → manifest. A rejected plan writes nothing,
  prunes nothing, and leaves the previous manifest (and its outputs)
  untouched. Reuse cannot bypass the check: validation precedes every reuse
  decision.
- **Failure modes stay fail-closed.** An unreadable or unwritable output
  root fails probe setup with a `Write` error; such a build could not write
  outputs anyway. Absolute spec paths never enter the probe (they would
  escape it) and fail later at the contained write boundary, as before.

## Consequences

- An aliased plan fails with a deterministic diagnostic naming both logical
  paths (`output path collision: "posts/Foo/index.html" and
  "posts/foo/index.html" resolve to the same output`); the first pair in
  sorted plan order is reported.
- Case-sensitive and non-normalizing filesystems are unaffected beyond the
  probe's file operations: distinct names replicate distinctly, so existing
  valid builds keep working.
- Probe cost is proportional to plan size (directory creation plus one empty
  file per artifact, all removed via `Drop` on every exit path). Probe names
  never appear in diagnostics, outputs, or the manifest.
- The alias decision itself has no check-then-use gap: it depends only on
  the fixed plan and volume semantics, not on mutable output-tree state.

## Out of scope (unchanged)

Route/slug normalization policy (none introduced), the 14D stale-vs-current
identity protection (kept, distinct responsibility), source-side symlink
policy (R14-4), URL percent-encoding (R14-3), manifest trust wording (R14-5).
