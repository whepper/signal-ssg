# Signal Architecture

Signal is a deterministic static site **compiler**:

```text
config
  ↓
ingest
  ↓
validated specs (config gates, output paths, routes, templates)
  ↓
reference validation
  ↓
incremental BuildPlan
  ↓
execute
  ↓
prune
  ↓
manifest
```

One canonical pre-execution pipeline lives in `signal-cli::pipeline`
and is shared verbatim by every command. Commands stop at different
stages but never skip an upstream one:

| Command         | Ingest | Structural validation | Reference validation | Plan | Execute | Prune | Manifest |
| --------------- | ------ | --------------------- | -------------------- | ---- | ------- | ----- | -------- |
| `build`         | yes    | yes                   | yes                  | yes  | yes     | yes   | yes      |
| `serve`         | yes    | yes                   | yes                  | yes  | yes     | yes   | yes      |
| `check`         | yes    | yes                   | yes                  | no   | no      | no    | no       |
| `build --explain` | yes  | yes                   | yes                  | yes  | no      | no    | no       |
| `explain`       | yes    | yes                   | yes                  | yes  | no      | no    | no       |

`explain` without a target prints the whole plan exactly like
`build --explain`; `explain <asset>` prints one source asset's record
(source, type, byte size, referrers, planned derivatives, output, copy
action, reuse/rebuild decision) from the same plan (ADR 0028), and
`explain <asset> --width W [--format webp]` (or a bare derivative output
path) prints one derivative's record (source, input/output dimensions,
output, dependencies, generate action, decision) from the same plan
(ADR 0029). `explain social/<path>.png` prints one generated social
card's record (page, dimensions, inputs, output, generate action,
reuse/rebuild decision, or its non-planned state) from the same plan
(ADR 0032). `explain <output-path>` (A7.1) prints any planned artifact's
record by output path — kind, declared inputs, and reuse/rebuild decision,
plus the document count for the search index — so `explain index.json`
describes the search artifact; the same lookup also covers the sitemap,
feeds, `robots.txt`, `404.html`, and rendered pages (ADR 0034). `check`
additionally reports pure asset counts (discovered /
referenced / resolved / missing / unsafe, plus planned derivatives and
social images) derived from specs plus model, and advisory diagnostics
(A6, ADR 0033) over the same model — both read-only, with no filesystem
reads beyond validation and image-header measurement for diagnostics.

The invariant: a command must not call a site valid while skipping
validation a real build requires. `check` and `--explain` are read-only
(no writes, no pruning, no manifest mutation). The single documented
exception is the output-filesystem alias probe: it answers whether a
specific output filesystem can represent the plan distinctly, so it runs
in `build`/`--explain` (which take an output directory) but not in
`check` (which takes none). Every other build rejection — invalid config
routes, menu shape, logical output collisions, route collisions, template
failures, broken references — surfaces identically in all three commands
(pinned by `crates/signal-cli/tests/pipeline.rs`).

Signal is not a web framework. There is no request handling, no dynamic
runtime, and no server-side behavior. All decisions below serve deterministic
static output from normalized owned data.

## 1. Purpose

Build a modern, reusable, extensible static site generator. Real websites are
independent consumers: they own their content, templates, assets, and
configuration, and point the Signal binary at their site root. Markdown is an
input format; templates are a rendering detail; the site model is data.

## 2. Design principles

- Site independence: the engine is site-agnostic. The repository contains the
  engine, CLI, documentation, tests, minimal synthetic fixtures, and optional
  public examples — never a real website. No website is special to Signal;
  the architecture must never require access to a particular site.
- Content is data: normalize early into owned structures.
- Immutability after normalization: `&SiteModel` everywhere downstream.
- Boring identity: `(collection, source-relative-path)` by default.
- Determinism: ordered maps/sets, sorted discovery, sorted specs.
- Synchronous core: no async, no `Arc`-for-convenience, no lifetime-heavy or
  generic-heavy public APIs.
- Boundaries over frameworks: third-party details (Comrak, MiniJinja) stay
  behind crate modules.
- Security by construction: templates get in-memory strings and explicit
  contexts only — no filesystem, no network, HTML auto-escaping on.

## 3. SiteModel

Immutable collection of ordinary indexed Rust structures:

```text
SiteModel
├── ContentStore (BTreeMap<ContentId, ContentEntry>)
├── collection indexes
├── slug indexes
├── relationship indexes (tags, references-reverse, translation groups)
└── route index
```

Built once via `SiteModelBuilder::build()`, which validates duplicate ids,
duplicate sources, route collisions, and dangling references, then freezes.
Downstream code receives `&SiteModel`. The model is `Send + Sync` without
interior mutability.

## 4. Semantic relationships

Relationships are indexes plus query functions, not a graph engine:

```text
entries_in_collection(...)
entries_tagged(...)
translations_of(...)
referencing(...)
lookup_by_route(...)
```

No traversal infrastructure, no path-finding, no graph database. A real future
requirement must justify anything more general.

## 5. Build manifest

Semantic references do not imply build dependencies. `Post A references Post B`
does not mean `Post A depends on Post B's HTML`. Dependencies arise from what
generators actually consume and are recorded in a disposable build manifest.
The manifest is cache/build state: deleting `.signal/` must never change
correctness, and no persistent mutable build DAG exists.

Since slice 12A every successful build writes `.signal/manifest.json`: a
schema-versioned, deterministic record of

```text
generation identity (engine_version + behavior_version)
entry / query / config / static-source digests
every loaded template by name
source-asset digests by static path (A1: `assets` map, ADR 0028)
one flat record per artifact: kind, route, InputRef inputs, output byte digest
```

Artifact records have no edges to traverse. A template-rendered artifact names
the complete loaded template set (`InputRef::TemplateSet`), and one build-wide
compatibility field gates reuse. A page (or section page rendering a
section-root body) additionally names the source assets its entry references
as `InputRef::Static` (A1 asset edges, ADR 0028); the reuse predicate compares
those against the manifest's source-asset digests, so editing an asset
rebuilds exactly its embedders. Generated image derivatives (A2, ADR 0029)
extend the same shape one level down: each `DerivedImage` artifact and each
embedding page names `InputRef::DerivedImage{source,width,format}` —
parameters ride the reference (parameter changes are `InputsChanged`),
byte comparison reuses the source-asset map (source changes are
`DerivativeChanged`), and no derivative is ever compared against page
HTML. Recording, reuse, and pruning are described
in `docs/adr/0015`, `0016`, `0017`, `0019`, and `0020`; there is no graph of
any kind.

Planning lives in `signal-cli::build_plan`: `artifact_inputs()` is the single
source of truth for what an artifact consumes (shared by reuse decisions and
manifest recording), `plan()` turns current state plus the previous manifest
into explicit per-artifact `Reuse`/`Rebuild{reason}` decisions plus the stale
inventory, and `build_site` executes those decisions without redefining them.
Build planning derives the canonical inputs and reuse decisions for each
artifact; execution consumes those decisions without independently redefining
the dependency contract. Resolution (`resolve_artifact`) remains a procedural
mirror of the declared inputs — see the contract table in `build_plan.rs`.

The plan can be surfaced read-only via `signal build --explain`
(`signal-cli::explain`): it constructs the same `BuildPlan` execution
would use (through the shared `signal-cli::pipeline` prefix, including
reference validation), renders reuse/rebuild reasons plus the stale list,
and exits without resolving, writing, pruning, or persisting the manifest.

## 6. Ingestion

Filesystem discovery and Markdown parsing live outside `signal-core`
(`signal-cli` and `signal-markdown` respectively). Discovery is fail-closed
(ADR 0021): a content, template, or static directory that cannot be read —
or a source entry whose metadata cannot be obtained — fails the build
rather than being treated as empty or skipped, because an incomplete
inventory would otherwise make published artifacts look stale and trigger
pruning. Missing and empty directories remain valid — sources are optional.
Symlink policy is explicit: Markdown and template discovery follow symlinks
(a link to a directory is traversed, a link to a source file is collected),
static discovery skips symlinks entirely, and an unresolvable link is fatal
in all three walkers. Ingestion normalizes into `ContentEntry` values: identities, slug, route, title,
description, dates (validated `YYYY-MM-DD`), authorship, image references
(safety-checked), tags,
translation grouping, and semantic references. Front matter accepts both YAML (`---`) and
TOML (`+++`) blocks; unknown fields are preserved verbatim and surfaced to
templates through the `extra` context key. `_index.md`/`index.md` address the
collection root. Drafts are skipped at
ingest. Generators stay pure; resolution and writing happen in `signal-cli`
one artifact at a time — static assets included, as planned `Static`
specs — and every output path is validated/contained within the output
directory. Source assets (files under `static/`) are the asset input
boundary; references from content (Markdown body images, front-matter
heroes) resolve to source paths through the shared pure rules in
`signal-core::asset`, and embedding pages name them as `Static` build
inputs (ADR 0028). Front-matter ID as a move-surviving escape hatch is reserved,
not implemented. Git-derived `last_modified` is opt-in
(`[git] last_modified`), resolved by `signal-cli` (the process/filesystem
boundary) at ingest, advisory on failure, and always outranked by explicit
front-matter `lastmod` (ADR 0024).

Markdown renders to owned `RenderedBody` data in a single Comrak parse with
AST-level transforms inside `signal-markdown`: headings gain fragment ids
in the same pass that records them, fenced code becomes highlighted
copyable blocks (Mermaid preserved as text for client renderers), and
`[!KIND]` blockquotes become semantic asides. Author raw HTML is detached
at parse — Signal's content model carries no raw-HTML passthrough.

Identity concepts stay separate: `ContentId` (in-memory handle),
`SourceRef` (stable `(collection, path)`), `Slug`, `Route`. No persistent
hash-based identity.

## 7. Rendering

`signal-render::Renderer` is the only rendering surface:

```rust
trait Renderer {
    fn render(&self, template_name: &str, ctx: &RenderContext) -> Result<String, RenderError>;
    fn template_names(&self) -> Vec<String>;
}
```

`MiniJinjaRenderer` loads templates from in-memory strings only, enables HTML
auto-escaping for `.html` templates, and exposes the loaded set for template
dependency recording. Template-rendered artifacts depend on the complete
loaded template set (`InputRef::TemplateSet`, ADR 0020) rather than a bare
selected name, so `{% extends %}` / `{% include %}` changes invalidate
soundly. No field-level access tracking is performed; the set is deliberately
conservative. Rendering contexts are explicit
`RenderContext` value bags carrying ready-to-render values: formatted dates,
canonical URLs, OpenGraph fields, and serialized JSON-LD alongside content.
Body HTML for entry and section-root pages passes through responsive
rewriting first (A3, ADR 0030; `<picture>` since A4, ADR 0031): Comrak
`<img>` tags whose sources have planned derivatives gain `srcset` (actual
widths, deduplicated), `sizes="100vw"`, and intrinsic dimensions for a
single planned format, or become `<picture>` with one AVIF-first
`<source type=…>` per planned format and a WebP fallback `<img>` for
several, while every other tag passes through byte-identical; entry
heroes additionally gain a `responsive_image` context value (flat
fallback fields plus per-format `sources` and a `has_picture` flag) for
templates that opt in. On `[social]`-enabled sites, participating entry
pages additionally gain a generated Open Graph/Twitter card reference
(A5, ADR 0032): `og_image`/`twitter_image` point at the page's
`social/<route-path>.png` card (superseding the hero), `twitter_card`
and `twitter_*` keys are supplied, and `social_image` carries
`{ url, absolute_url, width, height }` for templates. The card itself is
an `ArtifactKind::SocialImage` artifact planned from page identity,
configuration, and the optional hero source — never from the page's HTML.
The planner decides which representations exist (their inputs are already
declared); rendering only expresses them.
Every route-shaped string entering a template or serializer is the URL-path
form (`signal_core::encode_route_path`, ADR 0023): logical routes may
contain URL-significant characters, so `?`, `#`, `%`, and non-ASCII bytes
are percent-encoded at this boundary while `/` separators, trailing
slashes, and ordinary slugs pass through unchanged. The same boundary
covers front-matter image `src` values and internal menu URLs (emitted
encoded; active matching still compares raw logical routes), while Markdown
author URLs and external menu URLs pass through untouched. URL-path encoding
precedes HTML/JSON/XML escaping and replaces none of it; typed `Route`
values (menu matching, model lookups, manifest keys, filesystem paths)
keep the raw logical form.

## 8. Generators

Generators are pure projections: `&SiteModel -> Vec<ArtifactSpec>`. They do
not mutate the model and do not consume each other's rendered output.
`EntryPages` (one page spec per regular entry), `SectionIndex` (one listing
per collection), `Home` (featured hero plus recent list), `TopicsIndex` /
`TopicTerms` (taxonomy index plus one page per term), `MainFeed` /
`SectionFeeds` / `TaxonomyFeeds` (RSS from normalized data: main, one per
collection, and the taxonomy label index plus one per term), `Sitemap`,
`Robots` (a fixed allow-all policy referencing the sitemap), and `Search`
(versioned JSON index from normalized plain text) are implemented. The
themed not-found page (`404.html`) is planned directly from
`site.not_found_template` (it consumes no content). RSS beyond these
families (e.g. author feeds) remains a future projection behind the
`Generator` trait.

Entry pages additionally carry a **related-entry projection**
(`signal_generators::related_entries`): the strongest shared-tag overlaps
over the canonical tag index, capped by `[related] limit`, ranked by
shared-tag count then the standard newest-first order. It is a context
projection, not an artifact — no spec, no file. Each page declares
`InputRef::Query{related:<route>}`, so a change to any entry it lists
rebuilds the page without any reverse edge (ADR 0035).

### Search index (A7, ADR 0034)

`Search` is the one projection written for an external consumer rather
than a rendered page. ADR 0034 fixes its architecture:

- **Document collection, not an engine.** `index.json` is a versioned,
  pretty-printed JSON array of normalized documents. Signal owns
  extraction, structural normalization, projection, and serialization;
  tokenization, matching, ranking, filtering, and presentation belong to
  the browser client. Signal ships no tokenizer, no postings, and no
  scores.
- **One site-wide artifact.** `ArtifactKind::SearchIndex` at `index.json`,
  planned unconditionally (like the sitemap), carrying no route.
- **Narrowest possible input.** The only declared input is
  `InputRef::Query{search_documents}` — the exact projection consumed. No
  `Config`, no `TemplateSet`, no per-entry edges, and no HTML digests. The
  index therefore rebuilds exactly when the searchable projection changes
  (title, body prose, description, tag, date, route) and is reused for
  template, config, asset, fenced-code, draft, and section-root changes.
- **Structural normalization only.** `RenderedBody.plain_text` strips
  Markdown and collapses whitespace while preserving authored case and
  Unicode, so snippets stay display-faithful. Lexical folding (case,
  accents, stemming) is the client's.
- **Exclusions reuse the publication model.** Regular entries only:
  drafts never reach the model, section roots are filtered, and generated
  pages (listings, taxonomy, feeds, sitemap, 404) are not entries.

`signal explain index.json` reports the artifact's kind, declared input,
document count, and reuse/rebuild decision, derived from the same spec
list and `artifact_inputs` the build plans from (A7.1). The contract, its
structural normalization, its exact input set, and the rebuild matrix are
pinned by `crates/signal-cli/tests/search.rs` (A7.2).

## 9. ArtifactSpec

Generators plan; the CLI resolves and writes — one artifact at a time:

```rust
struct ArtifactSpec {
    path: String,       // relative to output root
    kind: ArtifactKind, // Page | CollectionIndex | Home | Taxonomy | Rss | Sitemap | SearchIndex | Static | DerivedImage | SocialImage | Robots | NotFound
    route: Option<Route>,
}
```

No giant in-memory `Artifact { content: Vec<u8> }` map. Dependencies live in
the build manifest, not on the spec. Future content representations
(`Bytes` / `Source` / `Staged`) would let large assets bypass memory.

Three invariants (ADR 0014):

- **Plan completeness.** Every output file — HTML, feeds, sitemap, search
  index, static assets, and generated image derivatives and social images —
  corresponds to exactly one `ArtifactSpec`.
  Static files are enumerated during planning (deterministically,
  symlinks skipped) and resolved through the same pipeline; derivatives
  are planned purely from content references plus `[images]`
  configuration (A2, ADR 0029), and social images purely from page
  participation plus `[social]` configuration (A5, ADR 0032); generated vs
  static path collisions fail validation before anything is written. Two
  distinct planned paths that resolve to the same filesystem object on the
  output volume (case-insensitive or Unicode-normalizing alias) are likewise
  rejected before any write (ADR 0022) — logical route identity is unchanged,
  the build simply refuses a plan the filesystem cannot represent distinctly.
- **Independent resolution.** `resolve_artifact(spec, config, root, model,
  renderer) -> bytes` is the single dispatch point per kind. One artifact
  can be regenerated without the full build loop; cheap projections are
  recomputed inside rather than threaded through loop state.
- **Path containment.** Route segments are validated at their birth
  (`validate_route_segment`/`validate_route`: no `.`, `..`, empty,
  separators, backslash, whitespace, or control characters — slugs at
  ingest, collection/taxonomy prefixes before planning), and the write
  boundary enforces component-based containment, so no artifact can escape
  the output directory lexically. Containment is also filesystem-aware
  (ADR 0017): the output root is resolved once, pre-existing symlinked
  ancestor directories are refused for both writes and removals, and a
  write refuses an existing final-component symlink rather than following
  it. Stale pruning compares filesystem identities, so a stale logical path
  can never delete a current artifact through a case-insensitive or
  Unicode-normalizing filesystem alias. Traversal input fails the build; it
  is never silently rewritten. Containment is check-then-use: fully closing
  the race between the symlink check and the write/removal would require
  no-follow / open-by-directory-descriptor primitives beyond the portable
  standard library, which Signal does not use; that race is not claimed to be
  closed.

## 10. Crate boundaries

| Crate | Owns | Forbids |
|---|---|---|
| `signal-core` | `SiteModel`, identities, queries, `ArtifactSpec`, config types, diagnostics | I/O, Comrak, templates, CLI |
| `signal-markdown` | Comrak integration, `RenderedBody`, extraction | Leaking arena AST |
| `signal-render` | `Renderer`, contexts, MiniJinja, recorded template set, post-render HTML minification | Leaking engine types, FS/network loaders |
| `signal-generators` | `Generator`, page/section/home/taxonomy projections | Mutating model, chaining rendered output |
| `signal-cli` | CLI, config file loading, discovery, artifact writing, orchestration, manifest | — |

`signal-core` has no `minijinja`/`comrak`/`clap`/`miette` dependencies and no
`std::fs`/`std::net` usage; invariant tests pin this via manifest/source
inspection.

## 11. Determinism

`BTreeMap`/`BTreeSet` for model indexes, contexts, and manifest maps;
`ContentId`-ordered iteration; sorted source discovery; sorted artifact specs;
sorted template names; canonical digest hashing (struct field order, ordered
`Vec`s). The manifest serializer is deterministic, and the generation identity
contains no timestamps, runtime IDs, environment values, or machine paths.

Golden-tree tests assert byte-identical public output and manifest bytes
across independent builds, and clean-vs-incremental equivalence for identical
inputs. Date formatting is locale- and timezone-independent by construction
(`signal_core::format_date` implements a fixed strftime subset over
`YYYY-MM-DD` strings with an internal month table — no libc, no `TZ`
dependence). Generation identity contains no timestamps, runtime IDs,
environment values, or machine paths.

Platform qualifications (deliberate, not gaps): behavior that depends on
filesystem semantics (case/Unicode aliasing, symlink handling, file-identity
comparison) can differ by platform and is tested where the platform exhibits
it. CI runs the suite on Ubuntu and macOS; universal cross-platform byte
identity is not claimed beyond that matrix.

## 12. Security boundaries

- Templates: in-memory strings only, explicit contexts, HTML auto-escaping,
  no filesystem or network access.
- Markdown: author raw HTML is detached at parse (no passthrough), and
  author-controlled link/image destinations are scheme-checked at the AST
  boundary before HTML generation (`signal_core::is_safe_author_url`).
  Relative, root-relative, and protocol-relative destinations plus
  `http(s)` pass; `mailto:` passes for links only; `javascript:`, `data:`,
  `vbscript:`, and every other scheme are neutralized, so no executable
  `href`/`src` can be produced. Front-matter image references share the
  same policy (`resolve_image_url`).
- Config: TOML + Serde; file reads confined to `signal-cli`.
- Secrets: never read `.env`/keys/credentials; production logs/config treated
  as sensitive.

## 13. Incremental builds

Every successful build writes `.signal/manifest.json`; the next build consults
it. Reuse is a total per-artifact predicate — an artifact is reused only when
all of the following hold (any uncertainty rebuilds):

- generation compatibility: the recorded engine/behavior identity exactly
  matches the running binary (a build-wide gate, never an artifact input);
- the manifest record exists under the same path with the same artifact kind;
- the canonical input reference lists are equal;
- every current input digest matches the recorded one (entries; queries via
  their consumed projections; config; the complete loaded template set via
  `InputRef::TemplateSet`; and source bytes via the `assets` map, for
  `Static` inputs and for generated inputs that consume a source —
  `DerivedImage{source}`, a social image's hero);
- the existing output is a regular file whose bytes hash to the recorded
  output digest.

mtime is never consulted. A missing, corrupt, or wrong-schema manifest disables
reuse (`PreviousManifest::Unusable` → full build). A legacy manifest without a
generation identity is readable but never compatible, so it also forces a full
build. A generation mismatch is an invalidation condition, not an error.

Ordering: plan current artifacts → validate output paths (exact logical
duplicates plus current/current filesystem aliases, ADR 0022) →
resolve/reuse and write current artifacts →
prune `previous manifest − current plan` → atomically replace the manifest.
Pruning runs only against a usable previous manifest, never from a corrupt one,
and only after current writes succeed. Stale paths come only from the previous
manifest inventory — the output directory is never enumerated — and the set
difference is over normalized paths. Current-vs-stale filesystem identity
collisions are protected (ADR 0017): missing stale outputs are ignored, stale
directories fail safely, and unsafe or aliasing stale paths are skipped. Parent
directories are never removed. The manifest is the inventory authority for
Signal-managed output: only paths it records as artifacts are ever pruned,
so treat it as trusted build state alongside the site sources — a
well-formed forged record is indistinguishable from a legitimate one.

Failure semantics: a failure in resolution, writing, or pruning returns an
error and leaves the previous manifest in place, so the next build reconciles
from it (missing or modified outputs rebuild; the stale set is recomputed).
Source, template, and static discovery are fail-closed (ADR 0021): an
unreadable directory — or a source entry whose metadata cannot be obtained —
aborts the build before any write, prune, or manifest replacement,
so an incomplete inventory can never be mistaken for intentional deletion.
The manifest is replaced atomically via a temp file plus rename, so a failure
before the rename leaves the old manifest intact (an abandoned `.signal/` temp
file is harmless). The build is **not** whole-tree transactional: artifacts
written before a mid-build failure remain on disk until a later build rewrites
or prunes them, and stale files deleted before a manifest-write failure may
already be gone. Recovery is via the next manifest-driven build.

Template invalidation is deliberately conservative: any change to the loaded
template set invalidates every template-rendered artifact (ADR 0020). There is
no per-artifact `{% extends %}` / `{% include %}` closure and no template-source
parser. Finer config scoping remains a possible future refinement.

## 14. Serve

`signal serve` (`signal-cli::serve`) is a thin synchronous loop over the
production pipeline: initial `build_site_from_disk`, then `notify`-driven
re-invocation of the same function on coalesced source changes
(`signal.toml`, collection sources, `templates/`, `static/`, plus `.git`
only with opt-in git dates), serving the output directory with a blocking
HTTP server. The configured output tree is never watched, so Signal's own
writes cannot retrigger builds. Failures are reported while last-known-good
output keeps serving; recovery is via the next manifest-driven build, as
with `signal build`. No live reload, no async runtime, no second renderer.

## 15. Reference validation

`signal-cli::link_check` validates structured internal references
(`RenderedBody.links/images`, front-matter images, internal menu targets)
against the in-memory inventory of planned routes, files, static assets,
and entry heading ids. It runs after `validated_plan()` and before any
write/prune/manifest step, so a broken reference fails closed like invalid
content; `signal check` runs the same validation without building.
Image derivative sources (A2, ADR 0029) are probed in the same position —
existence plus PNG/JPEG decodability, on both the build and check paths —
so malformed sources fail identically everywhere before any write.
Composited social-card hero sources (A5, ADR 0032) are probed there too,
by the same read-then-decode rule.
External destinations are classified and skipped (never fetched).
Reference-style/autolink Markdown forms, template-literal URLs, and raw
HTML are outside the structured model and intentionally unchecked.

## 16. Asset and publishing pipeline (A1–A6)

A1–A5 built three generated artifact families around the original source
asset. They share one pipeline and one rule per family: **identity decides
reuse, and the planner decides what exists.**

```text
                       content (Markdown bodies, front matter)
                                     │
                                     ▼
                          asset references (pure strings)
                                     │
         ┌───────────────────────────┼───────────────────────────┐
         ▼                           ▼                           ▼
   Static artifact          DerivedImage artifact         SocialImage artifact
   source bytes             source + width + format      page metadata + hero
         │                           │                           │
         └───────────────────────────┼───────────────────────────┘
                                     ▼
                            ArtifactSpec (path + kind)
                                     │
                     ┌───────────────┴────────────────┐
                     ▼                                ▼
              artifact_inputs                   resolve_artifact
              (pure: config + model)            (bytes at the I/O boundary)
                     │                                │
                     ▼                                ▼
             BuildPlan / manifest              HTML, images, feeds, …
                     │
                     └──────────────► check / explain / build agree
```

| Family | Kind | Output identity | Declared inputs | Producer |
|---|---|---|---|---|
| source asset | `Static` | source path, verbatim | `Static{path}` | copy (`build::resolve_static_artifact`) |
| image derivative | `DerivedImage` | `(source, width, format)` → `{stem}-{width}.{ext}` | `DerivedImage{source,width,format}` | `signal-cli::images` (decode → clamp → resize → encode) |
| social image | `SocialImage` | route → `social/<route-path>.png` | `Entry{route}`, `Config`, `Static{hero}` when composited | `signal-cli::social` (metadata + bundled font → PNG) |

Boundaries, unchanged from A1's shape:

- **Pure core.** `signal-core::asset` (references, MIME, derivative
  identity, responsive selection) and `signal-core::social` (naming, the
  path → page inversion, participation, URLs) do no I/O: planning and
  `artifact_inputs` stay byte-free, and every identity rule lives in one
  pure function that planning, rendering, `check`, and `explain` share.
- **I/O boundary.** `signal-cli::images`, `signal-cli::social`, and
  `signal-cli::responsive` own pixels and files. Codec details never leave
  them: the planner sees specs, never `image`/`ravif`/`ab_glyph` types.
- **No artifact → artifact edges.** Every declared input is an entry, a
  query projection, the template set, the configuration, or a *source*
  file's bytes. Derivative and social inputs digest the source through the
  manifest's `assets` map; nothing digests another artifact's output. The
  dependency graph is therefore depth ≤ 2 and **cannot contain a cycle**,
  and reuse needs no traversal.
- **Rendering decides only how.** Body rewriting and hero contexts consume
  the plan's derivative views (via the shared `responsive_image` selector);
  the social generator never renders HTML and the renderer never plans.
  The one deliberate filesystem read in the rendering path is dimension
  *measurement* for `srcset`/`width`/`height` (`signal-cli::images`
  `probe_dimensions`, post-validation, hard error on failure): planning
  deliberately stays byte-free (ADR 0029), so dimensions are measured
  facts, not plan inputs.
- **Manifest.** `generation identity` (engine + behavior version) is a
  build-wide gate; `config_digest`, `entries`, `queries`, `templates`, and
  `assets` are the digest tables that declared inputs resolve against.
  Generated families need no schema of their own.

Behavior versions record output-generating changes that semantic inputs
cannot express (rendering, encoders, layout). The inventory and the bump
rule live in [Incremental builds](website/content/docs/builds.md); the
authoritative history is the doc comment on
`signal_cli::manifest::GENERATION_BEHAVIOR_VERSION`.

### Diagnostics (A6)

A6 adds no artifact, no input variant, no manifest field, and no new
dependency edge. It adds one **pure analysis over the publishing model**:
`signal-cli::diagnostics::analyze` reads the planned specs, the frozen
model, the configuration, and image headers, and returns a small closed
set of advisory `Diagnostic` values.

```text
        planned specs + frozen model + config + source headers
                              │
                              ▼
                         analyze()
                              │
                 ┌────────────┴────────────┐
                 ▼                         ▼
          check presentation        explain presentation
```

Rules that keep diagnostics from becoming a second publishing system:

- **Not artifacts.** Diagnostics are never planned, never written, never
  pruned, and never recorded in the manifest. They do not appear in
  `ArtifactKind`, `InputRef`, or the artifact graph, and computing them
  cannot change a single output byte or reuse decision.
- **Advisory only.** Hard failures remain `BuildError` and are unchanged:
  a missing or unsafe reference, a malformed image, an output collision,
  or an invalid configuration fails before diagnostics run. Severity is
  therefore two-valued — `warning` and `info` — with no `error` level.
- **One analysis, two presentations.** `check` and `explain` both call
  `analyze`; neither re-derives a condition. `explain` filters the same
  result by subject, so the two can never disagree.
- **Evidence, not advice.** Each diagnostic states a measured fact
  ("source is 4000×3000; the largest generated representation is 1280px
  wide") and carries a stable `code`. Conditions Signal cannot establish
  are not reported: byte-level "derivative larger than its source" needs
  generated output, which `check` does not have, so it is deliberately
  absent (see `docs/adr/0033`).
- **Deterministic.** Diagnostics sort by `(severity, subject, code)`;
  every input is a `BTreeMap`/`BTreeSet` or a sorted walk.

## 17. Explicitly deferred

Plugins (WASM/native/dynamic), remote content/caching, further image
codecs beyond AVIF/WebP, per-format encoder settings, cropping and other
transformations, alternate social-card dimensions or themes, recursive
processing of generated social images, remote image services, i18n,
a search engine or UI (the static index is a document collection;
tokenization, ranking, and presentation stay the client's — ADR 0034),
textual/embedding similarity, a generated related-content payload, and
editorial related-content references (relatedness is page-local derived
data over the canonical tag index — ADR 0035), CMS integration, dynamic
runtime behavior, redirects/aliases, sophisticated pagination, a custom
template language, a custom Markdown parser, a graph traversal engine,
per-artifact template-closure precision and field-level template tracking, and
async build architecture. Boundaries admit these later; none are built now.
