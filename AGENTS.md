# Agent Instructions

## Repository identity

This repository is whepper/signal-ssg.

It is the Signal static site generator. The separate whepper/blog
repository is a real-world compatibility target, not part of this
repository.

...

## Repository boundaries

- Never modify whepper/blog while working in signal-ssg unless the
  user explicitly requests a cross-repository task.
- Never copy the blog into Signal.
- Never add jeroen.steenvoor.de-specific logic.
- Never solve a blog compatibility problem by changing the blog.
- Requirements discovered from the blog must become generic Signal
  capabilities.

## Architecture

- Read ARCHITECTURE.md before making architectural changes.
- Preserve crate boundaries.
- signal-core remains free of filesystem/network/template I/O.
- ...
  
## Capability discovery before milestone work

Establish what the repository already does before designing or
implementing anything. Never infer that a capability is absent from a
roadmap item, milestone number, ADR title, TODO/FIXME, "deferred"/"future"
documentation, issue title, or task description — those state intent, not
repository state.

Inspect the actual repository instead, as relevant: production code,
configuration, CLI paths, generators, render context, the dependency and
build graph, artifacts, tests, fixtures, generated output, ADRs and
architecture docs, and compatibility/legacy paths. Look for complete,
partial, dormant, and pre-milestone implementations, and for existing
dependency mechanisms and tests that already establish the behavior.

Classify the requested capability before proposing work:

1. **Absent** — nothing meaningful exists; design and implement.
2. **Partial** — some implementation exists; name the precise gap.
3. **Implemented but undocumented** — works, but its architecture/contract
   is under-documented; prefer docs/ADR plus targeted guardrails.
4. **Implemented but insufficiently guarded** — sound, but key invariants
   are unprotected; prefer focused regression tests or diagnostics.
5. **Implemented but architecturally incorrect** — conflicts with current
   architecture; state the concrete problem before proposing refactoring.
6. **Implemented and architecturally sound** — do not reimplement; close
   with the smallest useful documentation or verification, or move on.
7. **Already fully covered** — adds no meaningful value; recommend skipping.

The milestone name never determines the classification. Before implementing
new functionality, state what already exists and where, what tests cover it,
what contract it follows, what is actually missing, and why existing
mechanisms cannot satisfy the requirement. If it already exists, stop the
greenfield path and reframe the task.

This generalizes the compatibility rule below ("determine whether Signal
already supports it") to every milestone, greenfield or incremental.

Milestone workflow:

```text
1. Inspect
2. Establish current capability state
3. Identify the actual gap
4. Classify the work
5. Define architectural boundary
6. Design
7. Implement only the demonstrated gap
8. Verify observable behavior
9. Document the resulting architecture
```

Existing capabilities may terminate earlier:

```text
Inspect → already implemented + sound → document/guard if valuable → stop
```

Keep discovery targeted: evidence-based, time-bounded, and limited to the
requested capability. This is not speculative archaeology — do not refactor
unrelated code, invent requirements, or add features because related code
exists. Example:

```text
A roadmap says "Feature X".
Do not assume X is absent. Establish:
    Does X already exist? Is it partial? Undocumented?
    Insufficiently tested? Is its architecture still valid?
Implement only what the evidence shows is missing.
```

## Compatibility work

The blog is a behavioral compatibility target.

"Compatible" means equivalent:
- URLs
- visible content
- metadata
- feeds
- assets
- functionality
- responsive presentation

It does not mean reproducing Hugo internals or byte-identical HTML.

When a capability is required by the blog:
1. verify that the requirement is actually exercised;
2. determine whether Signal already supports it;
3. implement the smallest generic capability;
4. add tests;
5. update compatibility documentation.

Do not implement unused Hugo features speculatively.

## Development

- Keep deterministic output.
- Preserve security invariants.
- `forbid(unsafe_code)` remains mandatory.
- Run `cargo fmt --all`.
- Run `cargo test --workspace`.
- Update documentation when behavior/architecture changes.