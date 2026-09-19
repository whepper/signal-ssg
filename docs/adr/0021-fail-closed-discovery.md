# ADR 0021: Fail-closed source and template discovery

## Status

Accepted (slice 15A; remediation of the fresh-review finding R14-1).
Extended in slice 15E: entry classification is fail-closed too, and the
source symlink policy is stated explicitly (no policy change).

## Context

Source discovery walked the site tree with silent error suppression:

- `signal-cli::discover::collect_markdown` returned `Ok(())` (an empty
  subtree) on any `read_dir` failure;
- `signal-cli::build::collect_templates` collapsed `read_dir` failures to
  `.unwrap_or_default()` (an empty template set).

On its own that looks harmless. Combined with manifest-driven stale
pruning it is not:

```text
source read failure
    ↓
source appears absent
    ↓
current artifact disappears from the plan
    ↓
previous artifact appears stale
    ↓
Signal deletes the published artifact
    ↓
new manifest records the truncated inventory
```

A transient condition — a permission change, an NFS hiccup, a partial
checkout — could therefore delete published output and encode the truncated
inventory as truth, with no error and no warning.

## Decision

- **Discovery is fail-closed.** `discover_markdown_sources` and
  `collect_templates` propagate `read_dir` and per-directory-entry failures
  as `BuildError::Read` naming the offending directory, through the shared
  `errors::discovery_error` helper. No new error type or abstraction is
  introduced; the existing `collect_static_files` pattern is the precedent.
- **Absence is not failure.** A missing directory (`NotFound`) remains an
  allowed empty result: configured collection sources, the default
  `content/`, and `templates/` are all optional. Empty directories remain
  allowed. Only genuine read failures abort.
- **Ordering already provides the pruning guard.** Markdown discovery runs
  during ingestion, before planning. Template discovery runs after planning
  but before any artifact is written, before stale pruning, and before the
  manifest is replaced. A discovery failure therefore aborts the build with
  the previous manifest and published output untouched; a later successful
  build reconciles normally. No additional pruning guard was added.
- **The same rule protects the template set.** Because template discovery is
  part of the `InputRef::TemplateSet` digest (ADR 0020), a partial walk must
  never masquerade as a legitimately smaller set.

## Consequences

- An unreadable content or template subdirectory fails the build with a
  diagnostic naming the path; no artifact is pruned, no page is lost, and the
  previous manifest remains intact.
- A symlink cycle that previously terminated only because a `read_dir`
  failure was swallowed now surfaces as an explicit discovery error. Source
  symlink policy itself is unchanged; R14-4 remains open.
- Successful builds are unaffected: complete, empty, and missing directories
  behave exactly as before, and incremental reuse and pruning are unchanged.

## Follow-up (slice 15E)

Slice 15A closed `read_dir` and directory-entry *iteration* errors, but both
walkers still classified entries with `Path::is_dir()`, which reports
`false` when metadata cannot be obtained (unreadable traversal, symlink
depth exhaustion, I/O faults). A metadata failure therefore skipped the
entry silently — the same truncation 15A was built to prevent, reproducible
with a symlink through an unreadable parent (successful build, silently
dropped page, pruned output, rewritten manifest).

Both walkers now obtain metadata explicitly and propagate its failure
through the same `discovery_error` helper. The follow/skip policy is
unchanged, now stated explicitly:

- Markdown discovery follows symlinks: a link to a directory is traversed,
  a link to a `.md` file is collected.
- Template discovery follows symlinks under `templates/` the same way.
- Static discovery skips symlinks entirely (`file_type` never follows).
- A symlink that cannot be resolved is fatal in all three walkers, never a
  silent skip. Symlink cycles surface as explicit discovery errors once the
  OS reports the traversal failure.

An R14-4 redesign (whether these policies should be unified) is still
deferred; 15E only guarantees that policy and failure are never confused.

## Out of scope (unchanged)

`collect_static_files` entry-level filtering, source-side symlink policy
(R14-4), current-vs-current filesystem alias detection (R14-2), URL
percent-encoding (R14-3), and manifest trust wording (R14-5).
