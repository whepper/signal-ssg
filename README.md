# Signal SSG

> **Beta (0.1.0-beta.1).** Signal is usable and actively evolving; public
> feedback is welcome. Behavior and compatibility may still change between
> beta releases, and not all Hugo features are supported — see
> [docs/migration/hugo.md](docs/migration/hugo.md) for the tracked gap
> list. Signal is not yet a production-ready 1.0.

Signal is a deterministic, Rust-based **static site compiler**.

```text
sources -> ingestion -> validation -> immutable SiteModel
  -> queries/generators -> ArtifactSpec -> rendering/resolution -> output
```

Content is data: Markdown is one input format, and the canonical
representation is normalized owned Rust data. Signal is not a web framework
and not a Hugo compatibility layer.

## Usage model

A site is an independent consumer of Signal:

```text
my-site/
├── content/
├── templates/
├── static/
└── signal.toml

signal build
```

Point `signal` at a site root and it writes a static site to an output
directory. The Signal repository itself contains only the engine, its
documentation, and synthetic test fixtures — never your site.

```sh
signal build --root my-site --out dist   # defaults: --root . --out dist
signal check --root my-site              # validate signal.toml, list collections
```

`signal build` exits `0` on success and non-zero with a diagnostic on
failure; a failed build leaves the previous output and manifest in place.

## Core architecture

- **Compiler, not framework.** Fixed pipeline: ingest, validate/normalize,
  freeze `SiteModel`, generate `ArtifactSpec`s, validate routes,
  render/resolve, write output.
- **Site independence.** The engine is site-agnostic: collections, routes,
  templates, taxonomy, and home behavior are all configuration plus site
  templates. No website is special to Signal. See `ARCHITECTURE.md`.
- **`SiteModel`, not `SiteGraph`.** Immutable collection of ordinary indexed
  structures (`BTreeMap`/`BTreeSet`) with query functions
  (`entries_in_collection`, `entries_tagged`, `translations_of`,
  `referencing`). No generic graph engine.
- **Semantic relationships are not build dependencies.** References are query
  substrate; incremental builds derive dependencies from generation into a
  disposable `.signal/` manifest.
- **Boring identity.** Default identity is `(collection, source-relative-path)`;
  `ContentId`, source path, slug, and route are distinct types.
- **Owned Markdown derivatives.** Comrak stays behind `signal-markdown`;
  core sees only owned `RenderedBody { html, headings, links, images,
  code_blocks, plain_text, word_count }`, where headings carry deterministic
  fragment anchors and project to an explicit `Toc` — templates never parse
  HTML for structure. Front matter accepts YAML (`---`) and TOML (`+++`).
- **Sandboxed rendering.** `Renderer` trait with an in-memory MiniJinja
  implementation, HTML auto-escaping, no filesystem or network access.
- **Lightweight artifacts.** Generators emit `ArtifactSpec { path, kind }`;
  content is resolved/written one artifact at a time.

See [ARCHITECTURE.md](ARCHITECTURE.md) and [docs/adr/](docs/adr/).

## Current capabilities

Implemented and tested:

- Markdown ingestion with YAML/TOML front matter, drafts, date-prefixed or
  plain slugs, `_index.md` section roots, topics/tags indexes
- Validated machine-readable dates with locale-free presentation formatting
- Canonical URLs, OpenGraph tags, and safely-serialized JSON-LD (`Article`
  and `WebPage`), all omitted — never fabricated — when inputs are missing
- Authorship (entry override → site default → omitted) and hero images
  (identity/URL separation, unsafe schemes rejected, no processing pipeline)
- Entry pages, collection section pages, home page (featured hero + recent),
  taxonomy index and term pages — all with deterministic newest-first ordering
- RSS main + per-term feeds and sitemap, serialized from normalized data
  (no rendered HTML is ever read back)
- Versioned static search index (`index.json`) projected from normalized
  plain text — no engine or UI in Signal; any client is external
- Markdown presentation at the AST level: deterministic heading anchors +
  TOC projection, syntect class-span code highlighting with copyable
  wrappers, Mermaid source preserved for client renderers, semantic alert
  asides; author raw HTML is detached at parse (no passthrough) and
  author link/image URL schemes are checked before rendering — no
  `javascript:`/`data:`/`vbscript:` destination can reach `href`/`src`
- MiniJinja templates with inheritance, explicit contexts, auto-escaping
- Configuration-driven `menus.main` navigation with validated internal /
  external URLs and exact-match active state, resolved per page
- A complete artifact plan: static assets ride the same plan → resolve →
  write pipeline as generated output, every spec resolves independently,
  and output paths are validated/contained within the output directory
- Deterministic build manifests (`.signal/manifest.json`): schema-versioned
  record of a generation-behavior identity plus entry/query/template/
  config/static digests and per-artifact inputs and output digests
- Hash-based incremental reuse: unchanged artifacts skip resolve and write
  only when the previous manifest was produced by compatible generation
  behavior and every recorded input digest matches; template-rendered
  artifacts depend on the complete loaded template set, so
  `{% extends %}` / `{% include %}` changes invalidate soundly; no DAG
- Stale-output reconciliation: `previous manifest − current plan` pruned
  through the contained write boundary (files and links only, never
  directories). Only paths recorded as artifacts in the previous manifest
  are ever pruned — the manifest is the inventory authority, so treat it as
  trusted build state alongside the site sources
- Fail-closed source discovery: an unreadable content, template, or static
  directory — or a source entry whose metadata cannot be obtained — fails
  the build instead of being mistaken for an empty tree or a deleted file,
  so an incomplete inventory can never make published artifacts look stale
- Verbatim `static/` passthrough
- Deterministic golden-tree integration tests

Not yet implemented: image
processing, pagination, aliases, plugins.
See [docs/migration/hugo.md](docs/migration/hugo.md) for the tracked gap list.

## Incremental builds

Every successful build writes `.signal/manifest.json` and the next build
consults it. An artifact is reused only when the previous build was
generation-compatible and every recorded input digest still matches; stale
outputs are then removed as `previous manifest − current plan`. Reuse never
consults mtimes, a missing/corrupt/wrong-schema manifest disables reuse, and a
failed build leaves the previous manifest in place. Discovery is fail-closed:
an unreadable content, template, or static directory — or a source entry
whose metadata cannot be obtained — fails the build before any pruning
rather than being read as deletion, and missing/empty directories stay valid.
The build is not whole-tree transactional. See
[ARCHITECTURE.md §13](ARCHITECTURE.md) for the exact predicate, failure
semantics, and limitations.

Signal is designed for **deterministic static output** (ordered maps/sets,
sorted discovery, sorted specs). No performance characteristics are claimed;
nothing has been measured.

## Workspace

```text
crates/
├── signal-core/        # SiteModel, identities, queries, ArtifactSpec, config types
├── signal-markdown/    # Comrak + front-matter boundary, owned derivatives
├── signal-render/      # Renderer trait, MiniJinja implementation
├── signal-generators/  # Generator trait, page/section/home/taxonomy projections
└── signal-cli/         # CLI, config loading, discovery, build orchestration
```

## Development

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Dogfooding against a real site happens in that site's own (private)
repository: build Signal here, then run the resulting binary against the
external site checkout. Real-site verification is a local workflow, never a
repository dependency — the public test suite runs from a clean checkout
using only the synthetic fixtures under `fixtures/`.

## License

Apache-2.0. See [LICENSE](LICENSE).
