# Dogfooding log — Hugo site migration

Signal is developed against synthetic fixtures in this repository. In
parallel, it is dogfooded against a real private Hugo site kept in its own
repository — never in this one. This log records what each slice proved,
how it was verified, and which engine differences were found. Concrete
site content (titles, URLs, terms, dates) stays private; what is recorded
here is the verification method and the architectural lesson.

Method throughout: rebuild the private Hugo site fresh, build the same
sources with Signal, and compare observable behavior — page existence, URLs,
titles, ordering, listed entries, links, headings, body text — with small
throwaway parse scripts. Byte-for-byte HTML equality is never expected;
the engines differ by design.

## Slice 1 — content + article pages

Proved the full path: Markdown + front matter → frozen `SiteModel` →
routes → rendered pages.

- Front matter must accept both YAML (`---`) and TOML (`+++`): the live
  site uses both, including bare TOML dates. Unknown fields are preserved
  verbatim for later slices.
- Filename stems map to slugs verbatim (including date prefixes); `_index.md`
  addresses the collection root; drafts are skipped.
- Heading structure and code-fence counts matched exactly; body text matched
  apart from template chrome and render-hook output.
- Engine differences found: Goldmark smart quotes vs Comrak plain entities
  (accepted); source-word vs rendered-word reading-time counts (differ by a
  minute on long pages; alignment deferred); MiniJinja escapes `/` as
  `&#x2f;` in `href` attributes (functionally equivalent).

## Slice 2 — home + section lists

Proved listings from the frozen model: `entries_in_collection_by_date`
(dated newest-first, undated last, slug-then-id tie-breaks) mirrors Hugo's
`.ByDate.Reverse`, including a cross-item date tie.

- Section titles resolve `_index` entry → collection config → fallback;
  sections without members render content with an empty listing.
- Home renders the newest featured entry plus a capped recent list; the home
  title omits the per-page title key (Hugo `IsHome` behavior).
- Verified identical: page titles, headings, entry order, links, listed
  entries across home and all section pages.
- No pagination exists on the dogfooding site (`first-N` limits only), so no
  pagination framework was built.

## Slice 3 — topics taxonomy

Proved taxonomy from the existing `tags` index: display labels keep original
case while slugs fold (`Hello World` → `hello-world`, verified per live
term); terms index sorts case-insensitively; term pages list members
newest-first across collections.

- Verified identical: all live term URLs, labels, order, per-term counts,
  titles, and member sets including a cross-collection term with a date tie.
- Terms derive from entries only, so empty terms are unrepresentable —
  matching Hugo. Draft-only topics produce no pages (tested).
- Slug collisions are a build error, never silent (tested).
- Incidental finding: the committed `public/` output contained a stale term
  page with no corresponding source. Only live sources count; stale output
  is ignored, never migrated.

## Standing gaps (tracked in `hugo.md`)

Search, code/diagram/alert components, menus, image processing.

---

# Slice 6 — RSS + sitemap

Proved feeds and sitemap as pure model projections: generators plan
`Rss`/`Sitemap` specs from `&SiteModel`; `quick-xml` event writers
serialize bytes at resolve time, one artifact at a time. No rendered HTML
is ever read back; MiniJinja is bypassed for fixed XML schemas by explicit
design (string-templated XML is where escaping bugs hide).

- Main feed (`index.xml`): all regular entries newest-first, capped by
  `[feed] limit` (default 20). Per-term feeds (`<root>/<slug>/index.xml`):
  tag members newest-first through the same taxonomy index as term pages.
  Item selection mirrors the reference behavior (entries across collections,
  no section roots); descriptions prefer the author's summary, else a
  70-word plain-text body excerpt — the reference summary-length behavior.
- Dates are RFC 2822 at fixed UTC midnight with calendrically computed
  weekday names (no locale, no timezone); `lastBuildDate` derives from the
  newest item, never build time. Undated entries omit `<pubDate>`.
- The label-list index feed and section-list feeds are deliberately out:
  the former's rows are labels without content dates (stamping them would
  need non-model sources), the latter add no selection semantics beyond the
  main feed. Both recorded as deferred, not silently dropped.
- Sitemap derives from the explicit route inventory (entries, collection
  prefixes, home when generated, taxonomy routes) — never from output
  files. `<lastmod>` prefers `last_modified`, falls back to `date`, omits
  otherwise; list pages carry none. Feeds, assets, and drafts are excluded
  by construction. No `changefreq`/`priority` (no requirement).

---

# Slice 5 — metadata + article presentation

Proved the stored-vs-presentation split: the model keeps machine-readable
`YYYY-MM-DD` dates (validated at ingest, including leap days); display
formatting happens at the projection boundary via an explicit
`site.date_format` (strftime-style subset, fixed English months — no locale,
no timezone, deterministic).

- Canonical URLs derive from `base_url` + route; routes stay distinct and
  relative. OpenGraph ships `og:title/description/url/type/image` as explicit
  context keys, each omitted when its input is missing (`og:site_name` has no
  upstream requirement and stays out).
- JSON-LD `Article` (entries) and `WebPage` (sections, home, taxonomy) is
  built with `serde_json` — never string concatenation — with `</` escaped so
  payloads cannot break out of `<script>` (tested with a hostile title).
  Only available fields are emitted: no fabricated authors, dates, or images.
- Hero images promoted `image`/`image_alt` to typed fields. Identity
  (site-root URL form) is separated from delivery (static passthrough or
  external URL); unsafe schemes fail the build at ingest. No processing
  pipeline: resizing/transcoding stay out until a requirement proves need.
- Authorship resolves entry override → site default → omitted everywhere.
- Sample fixture covers: full metadata, sparse metadata (omission paths),
  author override vs default, hero figure, hostile-title escaping, formatted
  row dates, and a real static SVG proving the front-matter→static→URL chain.

---

# Slice 7 — heading anchors + table of contents

Proved anchors as parse-time data: a Comrak `HeadingAdapter` assigns one
deterministic anchor per heading while rendering, recording the normalized
`Heading` (level, plain text, id) in the same pass — model and HTML cannot
disagree. No second parser, no HTML post-processing, no Comrak types outside
`signal-markdown`.

- Anchor rule: Unicode-lowercase, keep alphanumerics/`-`/`_`, whitespace
  runs to `-`, drop the rest, trim edges, empty becomes `section`; repeats
  gain `-1`, `-2`, … — the GitHub convention, matching observable Hugo
  output for all live heading shapes.
- `Toc::build` nests H2–H6 by level order (H1 excluded — the title owns
  it); templates receive `toc` only when non-empty, so heading-free pages
  render no empty markup. Fragment links reuse the same ids by construction.
- Ids are attribute-escaped at write time (structurally safe despite the
  restricted charset); inline markup strips to plain text for both anchors
  and TOC labels; Unicode letters survive folding.

---

# Slice 8 — code blocks, Mermaid, alerts

Proved Markdown presentation as AST-level transforms inside the single
Comrak parse: author raw HTML detached first (preserving the no-passthrough
policy while allowing Signal-generated structural markup), alert
blockquotes spliced into semantic asides with real children, fenced code
value-swapped for final HTML — then one render pass produces HTML,
headings, and code metadata together.

- Code: syntect class-span highlighting composed from Comrak's own
  adapter (no new dependency), `div.code-block` wrapper with
  `data-language` hook and server-rendered copy button (behavior stays in
  site assets), unknown languages deterministic, per-block filename/
  linenos/hl-lines deliberately out (no live usage).
- Mermaid: source preserved as escaped text in `pre.mermaid`; no
  build-time browser; `has_mermaid` gates client-loader inclusion from
  normalized metadata.
- Alerts: NOTE/TIP/IMPORTANT/WARNING/CAUTION with fixed labels and
  accessible `<aside>` structure; multiline, lists, links, and inline
  Markdown render normally inside; unknown markers stay blockquotes.
- Security: hostile code/alert content verified escaped or dropped;
  attribute injection structurally impossible from the restricted id
  charset plus attribute escaping.

---

# Slice 9 — search index

Proved search as a pure model projection: `RenderedBody.plain_text` is
extracted from the transformed Comrak AST in the same parse (prose,
headings, and inline code in document order; fenced code and Mermaid
excluded by rule; alert bodies included as normal prose), and the
versioned `{version: 1, documents}` index serializes straight from
`&SiteModel`. No HTML parsed, no filesystem scanned, no client framework.

- Eligibility mirrors the reference (regular pages only): section roots
  out, drafts never in the model, feeds/listings not entries. Documents
  sort by route; routes double as stable ids and relative urls (no
  `base_url` requirement, unlike feeds).
- Headings flow inline (no duplication), code stays out (no documented
  code-search requirement), Unicode preserved, missing description/date
  omitted rather than fabricated.
- Always generated at `index.json` with no opt-in table; pretty-printed
  for reviewable goldens. In-memory-only tests prove the projection needs
  no build output, no files, no network.

---

# Slice 10 — menus & navigation

Proved navigation as configuration data resolved per page: `[menus.main]`
deserializes into input-only types, and the pure function
`ValidatedConfig + Route → Menu` validates URLs, normalizes internal
routes, and marks exact-match active items — with no `SiteModel`
involvement, no filesystem reads, and no generated-HTML inspection.

- Internal URLs normalize to canonical route form; absolute `http(s)`
  URLs pass through with host checks; `javascript:`/`data:`/other schemes,
  relative paths, and malformed values fail the build with a `Config`
  diagnostic before anything is planned.
- Templates receive `menus.main` (`{label, url, active}`) and iterate it;
  the key is absent when unconfigured. No menu artifact is generated.
- The synthetic fixture demonstrates home/collection/taxonomy/external
  items with per-page active states, verified end to end.

---

# Slice 12A — manifest recording & deterministic input digests

Recorded the first manifest: `.signal/manifest.json` after every
successful build, describing semantic inputs and exact outputs without
influencing the build. No skipping, no pruning, no DAG.

- One digest representation (lowercase hex SHA-256 over canonical JSON);
  entries hash an explicit semantic subset; queries hash consumed
  projections (never bare IDs, with `date_format` folded into listings);
  config is whole-canonical (coarse by design, values never stored);
  templates record the loaded set; statics hash content bytes; outputs
  hash exact written bytes collected during resolve/write.
- Artifact inputs (`Entry`/`Query`/`Template`/`Config`/`Static`) derive
  from the same resolution code that produces the bytes. Menus ride
  inside the config digest — no separate variant needed.
- Manifest is byte-identical across independent builds (frozen golden
  for the sample fixture); failed builds never replace it (atomic
  temp+rename, written only after all artifacts succeed).

---

# Slice 12B — manifest consumption & incremental artifact reuse

First real incremental behavior: unchanged artifacts skip resolve and
write entirely, driven solely by manifest input references plus current
digests — no DAG, no graph, no hard-coded edges, no pruning yet.

- Reuse predicate is total: record present, kind equal, canonical inputs
  equal, every input digest equal, existing output hashing to the
  recorded digest. Any doubt rebuilds. Previous manifests are untrusted:
  absent/corrupt/wrong-schema all fall back to full builds, and manifest
  paths are lookup keys only (spec paths drive all I/O).
- Invalidation precision verified end to end: title edits skip the
  sitemap; body edits reach only page/search/excerpt-feeds; tag edits
  skip the main feed but rebuild the sitemap on the new term route; date
  edits reach the sitemap via the lastmod fallback; template edits touch
  only their pages; config edits spare search and statics.
- Clean and incremental builds are byte-identical (public output and
  manifest); identical second builds resolve nothing (proven by
  read-only-output builds succeeding); failed builds still replace
  nothing.

---

# Slice 12C — stale-output reconciliation & safe pruning

Closed the incremental lifecycle: artifacts in the previous manifest but
absent from the current plan are deleted through the contained write
boundary, after current writes and before the atomic manifest rewrite.
No DAG, no graph, no filesystem-wide cleanup — pure inventory subtraction.

- Detection is `previous.artifacts − current.plan` on normalized paths,
  sorted; unknown files (never in any manifest) are never touched, and
  empty parent directories are left alone.
- Safety reuses Slice 11 primitives: invalid stale paths are skipped
  (self-healing, since the rewritten manifest drops them), directories
  fail the build instead of being recursed into, symlinks are removed
  themselves and never followed, and pruning runs only against usable
  manifests after current artifacts succeed.
- Verified end to end: article/static/term disappearance, renames
  (new artifact + stale artifact, never a "move"), multi-file pruning,
  nested statics, tampered-manifest sentinels intact, and clean ≡
  incremental byte equality with stale outputs absent from both.

---

# Slices 14A–14B — security fix and reuse compatibility

Two Slice 13 findings closed without touching the dependency model.

## 14A — Markdown URL safety

Author Markdown destinations were reaching generated HTML as live
`href`/`src` (`javascript:`, `data:`, `vbscript:`), contradicting the
front-matter image policy and the raw-HTML detachment boundary. The fix is
an AST-level allow-list, not post-render filtering: one shared predicate
(`signal_core::is_safe_author_url`, also used by `resolve_image_url`)
permits relative/root-relative/protocol-relative destinations, `http(s)`,
and `mailto:` (links only); rejected link/image nodes are unwrapped to their
inline text so no unsafe attribute can be rendered. Comrak's entity decoding
was verified (`javascript&#58;`, `&#106;avascript:`, `data&colon;`), and the
generated page is asserted clean end to end.

## 14B — generation-behavior identity

Incremental reuse compared semantic inputs only, so a change to Signal's
output-generating code could preserve stale artifacts across an in-place
binary upgrade. The manifest now records a generation identity
(`engine_version` = package version, `behavior_version` = maintained
constant), kept separate from `MANIFEST_SCHEMA_VERSION`. Reuse requires an
exact identity match; an absent identity (pre-field manifest) is
incompatible and forces a full rebuild, while still allowing safe stale
pruning from its valid inventory. The identity is a build-wide gate, not an
artifact `InputRef`, so no dependency edge is introduced. Tests tamper only
the recorded identity — sources untouched — and prove `same inputs +
different identity = rebuild`, plus unchanged no-op reuse and byte-identical
clean/incremental manifests.

---

# Slice 14C — template dependency closure

Closed the Slice 13 false-reuse finding: template-rendered artifacts recorded
only their directly selected template, so changing an `{% extends %}` parent
or an `{% include %}` partial kept stale output.

MiniJinja 2.x caches loaded templates and exposes no per-render dependency
hook, so the conservative (explicitly sanctioned) design was chosen: every
template-rendered artifact depends on the complete loaded template set via a
new `InputRef::TemplateSet`, whose digest is taken over the manifest's
`templates` map. All templates load up front, so any change — inheritance,
include, nested include — changes the set digest and rebuilds every
template-rendered artifact; feeds, sitemap, search, and statics carry no
template input and stay reusable. No template-source parser, no DAG, no
schema break (`InputRef::Template` is retained only for old-manifest
deserialization). Verified with extends/include/nested/unrelated-template
tests plus a manifest-closure assertion, and by re-running the original
BASE-1→BASE-2 repro (now `rebuilt: 2`, page reflects BASE-2).

---

# Slices 14D–14E — filesystem hardening and documentation truth pass

14D hardened the output filesystem boundary: the output root is canonicalized
once; pre-existing symlinked ancestor directories are refused for both writes
and removals; a write refuses an existing final-component symlink; stale
deletion never follows a final symlink; and pruning compares `(device, inode)`
identity so a stale logical path cannot delete a current artifact through a
case-insensitive or Unicode-normalizing filesystem alias. Conservative
refusal/skip is preferred when identity is ambiguous. The check/use race
(TOCTOU) is documented as a known limitation, not claimed closed. No manifest,
reuse, or generation semantics changed.

14E was a documentation truth pass: superseded "future/not implemented"
language for incremental builds, manifests, and template dependencies was
corrected; ARCHITECTURE §5/§11/§13/§14 and README now state the reuse
predicate, generation gate, conservative `TemplateSet` invalidation, pruning
ordering, atomic-but-not-whole-tree failure semantics, and the TOCTOU boundary
accurately; historical ADRs keep their slice-scoped reasoning with explicit
follow-up notes. No behavior changed.

---

# Slice 15A — fail-closed source discovery

Remediated the fresh review's BLOCKING finding R14-1: Markdown discovery
(`discover::collect_markdown`) and template discovery
(`build::collect_templates`) swallowed `read_dir` errors, turning a transient
source read failure into an apparently empty tree. Because pruning is
`previous manifest − current plan`, the missing artifacts then looked stale and
were deleted; the truncated inventory was recorded as truth, silently. The
review reproduced this with a `chmod 000` content subdirectory: the build
succeeded, dropped the page, pruned its previous output, and recorded the
shortened inventory.

Discovery now propagates `read_dir` and per-entry failures as
`BuildError::Read` (naming the directory) via the shared `discovery_error`
helper — the `collect_static_files` pattern, no new abstraction. A missing
directory (`NotFound`) and an empty directory remain valid empty results, so
optional collection sources, the default `content/`, and `templates/` still
work. Pruning needed no extra guard: ingestion (Markdown discovery) runs before
planning, and template discovery runs after planning but before any write,
prune, or manifest replacement, so a discovery failure aborts with the previous
manifest and published output intact.

Verified by three new tests (two unix permission-based end-to-end rebuilds —
content and templates — asserting `Err`, a path-naming message, unchanged
previous manifest bytes, an unchanged published page, and no partial manifest;
plus a missing-template-root test) and an unreadable-subdirectory unit test,
with skip-when-unenforced detection for privileged CI. `docs/adr/0021`
records the invariant. Symlink cycles that previously ended only via a
swallowed error now fail explicitly; source symlink policy itself (R14-4) is
unchanged and remains open.

---

# Slice 15B — current/current filesystem-alias collision detection

Remediated the fresh review's BLOCKING finding R14-2: collision validation
compared logical path strings only, so two distinct planned artifacts that
alias on the output filesystem (`posts/Foo/index.html` vs
`posts/foo/index.html` on APFS; NFC vs NFD `café`; `static/Index.json` vs
the generated `index.json`) were both written, one silently overwriting the
other, with a manifest digest the filesystem contradicted.

`BuildPlan::validate_output_paths` now replicates the planned tree, in
sorted order, into a transient probe directory inside the output root and
rejects the first pair of distinct logical paths that resolve to one
object — before any write, prune, or manifest replacement, and before every
reuse decision. The filesystem itself is the oracle, so no case-folding or
Unicode-normalization tables were introduced; `(device, inode)` identity is
used only inside the probe to attribute a proven collision, and
pre-existing hard links in the output tree can never confuse it. Logical
route/slug rules are unchanged, and 14D's stale-vs-current protection keeps
its distinct responsibility. `docs/adr/0022` records the design, including
the identity-vs-pathname distinction.

Verified by nine new tests: case, Unicode, and multi-alias rejection with
exact-pair assertions; static/generated aliasing; incremental aliasing with
previous-manifest and output preservation; a file-blocking-subdirectory case
plus its folded variant; a hard-link non-collision guard; and probe-residue
checks after failed and successful builds. Filesystem-dependent tests use
the 14D runtime-probe pattern (dual-mode assertions, no faked aliasing), so
they run meaningfully on insensitive/normalizing volumes and still assert
correct coexistence elsewhere.

---

# Slice 15C — route-to-URL path encoding

Remediated the fresh review's finding R14-3: logical routes containing
URL-significant characters (`issue#12`, `100%-done`, `a?x=1`) were
concatenated raw into every generated URL, so `#` became a fragment, `?` a
query introducer, and `%` an invalid escape across listing links, canonical
URLs, OpenGraph metadata, JSON-LD, RSS, sitemap, and search-index paths.

`signal_core::encode_route_path` is now the single route-to-URL boundary,
used directly or via `canonical_url` by every URL producer (plus
`encode_url_path` for feed `self` links, which are artifact paths rather
than routes). The policy is conservative per-segment encoding — unreserved
bytes literal, everything else uppercase-hex `%XX`, separators structural —
with routes treated as literal data (a literal `%` always encodes, so no
input is misread as an escape) and Unicode preserved as UTF-8 without
normalization. Route identity, slug validation, filesystem output paths,
menu URL semantics, and semantic input digests are unchanged; URL-path
encoding precedes HTML/JSON/XML escaping and replaces none of it.
`docs/adr/0023` records the policy.

Because encoding changes generated bytes without changing recorded semantic
inputs, `GENERATION_BEHAVIOR_VERSION` was bumped `1` → `2` per the ADR 0019
policy (no new `InputRef`, no digest changes); the frozen sample manifest
golden was refreshed for that field only, and public output is
byte-identical since all fixture routes are unreserved.

Verified by six core unit tests (exact encoded outputs for delimiters,
percent forms, sub-delims, Unicode, structure preservation, and
no-query/no-fragment structure) plus two end-to-end tests asserting the
encoded route consistently across section/term/taxonomy listings, entry
canonical/OG/JSON-LD/`route` context, RSS links/GUIDs, sitemap locations,
and search id/URL — including unchanged filesystem paths and intact
HTML/JSON escaping.

---

# Slice 15E — discovery metadata fail-closed, URL consistency, manifest trust

Remediated five post-15C review findings without architectural change.

**15D-1 (BLOCKING): entry metadata is fail-closed.** Slice 15A had closed
`read_dir` and entry-iteration errors, but both discovery walkers classified
entries with `Path::is_dir()`, which reports `false` when metadata cannot be
obtained. A symlink through an unreadable parent (or symlink-depth
exhaustion, or an I/O fault on stat) therefore skipped the entry silently —
reproducibly deleting published output, pruning it as stale, and rewriting
the manifest with a successful exit. Both walkers now obtain metadata
explicitly and propagate its failure via the shared `discovery_error`
helper; follow/skip policy is unchanged and now documented (Markdown and
templates follow symlinks, static skips them, unresolvable links are fatal
everywhere). Verified by content-symlink and template-symlink rebuild tests
asserting `Err`, path-naming diagnostics, unchanged manifest/output, no
partial manifest, and incremental recovery byte-equal to a clean build.

**15D-4: static per-entry errors propagate.** `collect_static_files`
dropped single-entry iteration errors with `filter_map(Result::ok)`; they
now propagate through the same helper (intentional symlink skips
unchanged). Covered by an unreadable-subtree rebuild test; the per-entry
branch shares the helper and pattern with the Markdown walker, and cannot
be triggered deterministically without a mid-read race.

**15D-2/15D-3: one URL per image and menu.** Front-matter `image` reached
templates raw while `og:image`/JSON-LD carried the 15C-encoded form (two
URLs for one image); the context value is now encoded with
`encode_url_path`, with absolute and protocol-relative values passing
through. Internal menu URLs likewise emit the encoded form while active
matching still compares raw logical routes and external URLs pass through
untouched. Markdown authored URLs are unchanged (queries and fragments are
legitimate there). Verified by end-to-end image tests and menu unit tests
(encoded output, active state, literal-data percent handling, externals).

**15D-5: manifest trust wording.** No mechanism change (pruning stays
previous-minus-plan; no provenance): README, ADR 0017, and ARCHITECTURE
§13 now state the manifest is the trusted inventory authority rather than
claiming unknown files can never be affected.

`GENERATION_BEHAVIOR_VERSION` was bumped `2` → `3` per ADR 0019 (image
`src` and menu `href` bytes change for previously valid inputs); the frozen
sample manifest golden was refreshed for that field only, with byte-identical
public output. Deliberately deferred: 15D-6 (`serde_yaml`), 15D-7 (probe
diagnostic, inert cleanups), 15D-8 (performance).

## BR-1 — real-site integration soak

Built the full private site (5 Markdown sources across 3 collections, 6
topics terms, 3 menus, 35 static assets) with Signal against hand-written
semantic-mirror templates (extends/include/loops/conditionals, no
site-specific engine code). Result: 58/58 artifacts, all 13 HTML routes
identical to Hugo's paths, all static bytes identical.

- Link audit over the Signal output: 79 internal references, zero missing;
  menu active states exact; feeds/sitemap/search parse; mermaid sources and
  alert asides render; front-matter `image` reaches `src`/`og:image`/JSON-LD
  as one URL (15D-2 path exercised by live content).
- Incremental soak, 9 mutations (edit/add/delete/rename-slug/taxonomy/
  image/template/config): every incremental build byte-equal (output and
  manifest) to a clean build of the same tree; prune counts exact (e.g. a
  removed term drops precisely its page+feed). No-op builds reuse 58/58
  with a stable manifest.
- Failure soak, 4 scenarios (invalid template, malformed front matter,
  unreadable source parent, invalid config): every build failed with output
  and manifest preserved, and recovery rebuilt to clean-equivalent state.
- Engine differences, all classified: Goldmark smart quotes, source- vs
  rendered-word reading time, and template chrome (accepted); heading-anchor
  `--` collapsing (`v041--v042` vs `v041-v042`) and feed date-tie ordering
  (slug-ascending tie-break) closed as documentation in BR-2. No BUG found.
