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