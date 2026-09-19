# ADR 0017: Stale-output reconciliation without a dependency graph

## Status

Accepted (slice 12C: safe pruning; no skipping changes, no invalidation changes).
Hardened in slice 14D: filesystem-aware containment and alias-safe pruning
(no change to the set-difference model).

## Context

Slices 12A/12B gave us a complete artifact inventory per build and reuse
of unchanged artifacts. Deleted content still left its output behind:
the previous manifest knew about artifacts the current plan no longer
contains. The remaining lifecycle piece is removing exactly those files —
without turning the manifest into a dependency graph and without ever
treating the filesystem as the source of truth.

## Decision

- Stale is pure inventory subtraction: `previous.artifacts − current.plan`,
  matched by normalized artifact path, sorted. No dependency analysis, no
  traversal, no graph of any kind.
- Deletion reuses the Slice 11 path-safety primitives, not a parallel
  implementation: `is_safe_artifact_path` pre-screens untrusted manifest
  paths (invalid records are skipped, and the build still succeeds), then
  `remove_artifact` enforces component containment again and removes only
  regular files or the symlink itself (never followed, never recursive).
  Directories at stale paths fail the build rather than being recursed
  into; missing files are already reconciled, not errors.
- Filesystem-aware hardening (slice 14D). Containment is measured against
  the *resolved* output root: the root is canonicalized once, so a
  configured output root that is itself a symlink is followed once and
  everything stays contained within its real target, while symlinks in the
  root's own ancestry are not mistaken for artifact ancestors. For both
  writes and removals, pre-existing symlinked *ancestor* directories between
  the root and the artifact are refused rather than traversed; a write also
  refuses an existing final-component symlink instead of overwriting its
  target. Only pre-existing components are inspected, so ordinary nested
  directory creation and missing destinations still work.
- Alias-safe pruning. After the current artifacts are written, pruning
  builds the set of their filesystem identities (`(device, inode)` on Unix;
  canonical path elsewhere) and refuses to remove any stale path whose
  identity matches one of them. On case-insensitive or Unicode-normalizing
  filesystems two distinct logical paths can name one object; such a stale
  path is skipped, never deleted. Ambiguous identity (missing or
  uninspectable) is skipped rather than risked; stale paths remain in the
  new manifest's absence and are preserved as unknown files.
- Ordering: resolve/write all current artifacts → prune stale → write the
  new manifest atomically. Pruning runs only against a usable previous
  manifest (corrupt manifests mean full build, no pruning), and only after
  current writes succeed. Unknown output files — never in any manifest —
  are never touched.
- Observability is runtime-only (`pruned` count + sorted `pruned_paths`
  on `BuildSummary`, printed by the CLI); nothing about pruning is
  persisted. Empty parent directories are left alone.

## Consequences

- Deleting an article, a tag's last member, a static asset, or renaming a
  slug removes exactly the disappeared outputs and nothing else; clean and
  incremental builds stay byte-identical.
- A malicious manifest with invalid or unsafe paths can at worst leave a
  stale file behind (skipped record). Neither manifest paths, ancestor
  symlinks, final-component symlinks, nor filesystem case/Unicode aliases
  can redirect a write or a deletion outside the resolved output root, and
  the self-healing rewrite drops invalid records from the new manifest.

## Trust boundary (slice 15E)

The manifest is the inventory authority for Signal-managed output: pruning
deletes exactly the well-formed artifact records in the previous manifest
that are absent from the current plan, and never enumerates the output
directory. Files absent from that inventory are not proactively deleted —
but the guarantee is conditional on the manifest itself. A well-formed
forged record (correct kind, inputs, and digest for a path Signal never
created) is indistinguishable from a legitimate one and its path will be
pruned. Someone able to modify the manifest or the output tree already has
the write access to delete such files directly, so this is a trust
assumption, not a privilege escalation: treat `.signal/manifest.json` as
part of the trusted build state, alongside the site sources.

## Deferred

Finer config/template scoping, template `{% extends %}` closures, remote
caches — all independent of this mechanism.
