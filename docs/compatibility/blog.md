# Blog compatibility inventory

What Signal must support to reproduce the private Hugo blog (the external
dogfooding target) with the same public behavior, features, and visual
presentation. The implementation may differ from Hugo completely; only
**observable behavior** counts.

Ground rules, inherited from the repository boundary:

- Only features **exercised by the blog** are requirements. Hugo capabilities
  the blog never uses are not recorded as needs.
- The blog's templates, CSS, and client JS are re-implemented in the blog
  repository (the "theme port"). Anything satisfiable inside that port is
  **External/theme-only**; it is listed for completeness, not as engine work.
- No blog fixtures, blog-specific conditionals, or blog URLs belong in Signal.
  The blog stays an external build against a released Signal.

Evidence basis: the blog's `hugo.toml`, all of `content/`, `layouts/`,
`assets/`, `static/`, and its README/WRITING docs, inspected at commit
`098c08b` (2026-09-08, "Add GitHub icon to site header"). Signal evidence cites
crate sources and docs at the current beta. The general Hugo dogfooding map in
[`docs/migration/hugo.md`](../migration/hugo.md) stays valid; this document is
the site-specific inventory and gaps list ([gaps.md](gaps.md)).

Statuses: **Supported**, **Partial**, **Missing**, **External/theme-only**.

## Site shape

- Three content roots: `articles/` (2 dated posts with `YYYY-MM-DD-` prefixed
  filenames), `projects/` (1 dated page + `_index.md`), `about/` (`_index.md`
  only).
- One taxonomy: `topics`. One menu: `main` (weight-ordered: Articles,
  Projects, About). RSS limit 20. `enableRobotsTXT` and `enableGitInfo` on.
- Outputs: home `html+rss+json`, section `html+rss`, page `html`.
- Content uses only: front matter (YAML and TOML), headings H2–H4, fenced code
  (`mermaid`, `sh`, `text`), pipe tables, blockquotes, one `> [!WARNING]`
  alert, links, lists, emphasis, inline code, and a horizontal rule.
  **No shortcodes, no math, no code-block attributes, no raw HTML.**

## Compatibility matrix

### Content model and front matter

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Front-matter formats | Both YAML (`---`) and TOML (`+++`) in live use | Supported | blog: two articles use different delimiters · signal: `split_front_matter` + tests | none |
| `title` / `date` / `description` | Required fields on every article and project | Supported | blog: all front matter blocks · signal: `ContentEntry`, ingest validation | none |
| `topics` display order | Topic eyebrows render in **as-authored order** (`delimit`, hero/card take `index . 0`) | Supported | blog: `article-card.html`, `list-item.html`, hero meta · signal: `tag_order` preserves front-matter order for summaries and the `tags` context key; canonical sorted set stays the indexing contract (gaps.md #11 resolved) | none |
| `featured` / `image` / `image_alt` | Home hero selection (first featured, newest first) and hero image with alt | Supported | blog: `index.html`, article front matter · signal: typed fields, `Home::featured`, ingest URL-safety; `EntrySummary` carries `image`/`image_alt` in URL-path form so heroes and cards render without the full entry | none |
| `toc` flag | Per-article TOC gate (`toc: true` + enough headings) | Supported | blog: `articles/single.html` gates on `.Params.toc` · signal: unknown fields survive to templates via the `extra` context key (`extra.toc`); the `Toc` projection itself is complete | none |
| `author` override | Per-article author, falls back to site author | Supported | blog: second article sets `author` · signal: entry → site-default fallback | none |
| `lastmod` | Explicit "updated" date (byline, article info, JSON-LD) | Partial | blog: documented field, **not set in any current file** — live "Updated" values come from Git (see Git info row) · signal: `lastmod` → `last_modified` fully supported | none for the field itself; Git sourcing is a separate gap |
| `draft` | Exclude from production build | Supported | blog: WRITING.md draft workflow · signal: ingest skips drafts | none |
| Filename-derived slugs | `2026-09-02-<slug>.md` → `/articles/2026-09-02-<slug>/` (date prefix kept verbatim) | Supported | blog: README content schema · signal: filename stem kept verbatim | none |
| `repo` (project pages) | "View on GitHub" link on project pages | Supported | blog: `projects/single.html` renders `.Params.repo` · signal: `repo` reaches templates as `extra.repo` | none |
| Raw HTML disabled | Author HTML must not pass through (`unsafe = false`) | Supported | blog: hugo.toml, WRITING.md ("not supported") · signal: author HTML nodes detached (stricter than Hugo's escaping; no content uses raw HTML) | none |

### Sections and pages

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Section roots (`_index.md`) | `about/` and `projects/` roots carry title + body content rendered above their listings | Supported | blog: `content/*/_index.md`, `about/list.html` · signal: `section_root` ingest, section context `content` | none |
| Articles listing | Newest-first, no pagination | Supported | blog: `.Pages.ByDate.Reverse` · signal: `collection_summaries` ordering contract | none |
| Project pages | Title, description deck, body, repo footer link | Supported | blog: `projects/single.html` · signal: entry template context (repo link: see `repo` row) | none |
| Home page | Featured hero (first featured by date) + latest-8 article grid | Supported | blog: `index.html` (`first 8`) · signal: `Home` generator, `HOME_RECENT_LIMIT = 8` | none |
| 404 page | Themed 404 with site chrome (header/footer/search) | Supported | blog: `layouts/404.html` · signal: `site.not_found_template` renders `404.html` from the template set with site context (gaps.md #6 resolved) | none |

### Taxonomy

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Topics index `/topics/` | Alphabetical term list with member counts | Supported | blog: `_default/terms.html` (`.Data.Terms.Alphabetical`) · signal: `topic_terms` case-insensitive alphabetical + `count` | none |
| Term pages | Newest-first member lists + per-term RSS | Supported | blog: `_default/taxonomy.html`, `topics/<term>/index.xml` · signal: `TopicTerms` + `TaxonomyFeeds` | none |
| Term URL slugs | `AI Security` → `/topics/ai-security/` | Supported | blog: built output · signal: `slugify` produces identical ASCII slugs | none |

### Markdown, code, and diagrams

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Core Markdown | Tables, emphasis, headings, lists, hr, images, links, blockquotes | Supported | blog: article bodies · signal: Comrak with table/strikethrough/autolink extensions | none |
| Code highlighting + copy button | Every fenced block highlighted (class-based tokens, theme-styled) with a copy affordance | Supported | blog: `_markup/render-codeblock.html` · signal: `code.rs` — `div.code-block`, class spans, `data-code-copy` button | Theme CSS must target Signal's wrapper/token class names (Chroma vs. Syntect naming differs) |
| Code-block attributes (`filename`, `linenos`, `hl_lines`) | Optional fenced-block enhancements | External/theme-only | blog: defined in WRITING.md, **unused by content** · signal: attributes deliberately ignored | none today |
| Mermaid | Client-side rendering, gated per page; static HTML keeps the diagram source as an expandable no-JS fallback (`<details>`) | Supported | blog: `render-codeblock-mermaid.html` + page-store gate · signal: `pre.mermaid` source preservation + `has_mermaid` gate ✓ + duplicated escaped source in a collapsed `details.mermaid-source` fallback | none (wrapper class naming is a theme-CSS target) |
| GitHub-style alerts | `> [!WARNING]` (etc.) → tinted callout with text label; `role="alert"` for warning/caution, `role="note"` otherwise | Supported | blog: `_default/_markup/render-blockquote.html` · signal: `alerts.rs` emits `aside.alert.alert-*` with matching `role` attributes | none (element/class naming is a theme-CSS target) |
| KaTeX math | Per-article opt-in (`math: true`), self-hosted assets, passthrough delimiters | External/theme-only | blog: `math.html` partial + vendored KaTeX; **no content sets `math`, no delimiters in content** · signal: the flag reaches templates as `extra.math` for gating; assets are plain static files | none today |
| Heading anchors + fragments | Stable heading IDs for TOC links | Partial | blog: `.Fragments.Headings` · signal: deterministic anchors; known engine differences on punctuation-adjacent runs (BR-1, `docs/migration/hugo.md`), internally consistent — verified in the real build (Hugo `#pass-1--hide-the-name` vs Signal `#pass-1-hide-the-name`; both self-consistent) | Accepted difference (documented) unless external deep links must survive |
| TOC structure | 2-level hierarchy (H2 with nested H3) built from heading data | Supported | blog: `articles/single.html` renders 2 levels · signal: `Toc` recursive items (H1 excluded, H2–H6, `level` exposed) — real build verified: gating children on `item.level == 3` reproduces Hugo's 36-link TOC exactly | none |

### Feeds, sitemap, robots, search

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Main RSS `/index.xml` | Limited to 20 items; front-matter description as summary | Supported | blog: `services.rss limit = 20` · signal: `DEFAULT_FEED_LIMIT = 20`, description-first excerpts | none |
| Per-term RSS | `/topics/<term>/index.xml` | Supported | blog: built output · signal: `TaxonomyFeeds` (term feeds plus the label-index feed) | none |
| Section RSS | `/articles/index.xml`, `/projects/index.xml`, `/about/index.xml` | Supported | blog: `outputs section = ["html","rss"]` · signal: `[feed]` plans one `SectionFeeds` artifact per collection (gaps.md #4 resolved) | none |
| Topics index feed | `/topics/index.xml` | Supported | blog: Hugo default taxonomy outputs · signal: `TaxonomyFeeds` plans the label-index feed next to term feeds (gaps.md #4 resolved) | none |
| Sitemap | `sitemap.xml` from public routes | Supported | blog: robots.txt references it · signal: `Sitemap` generator over route inventory | none |
| robots.txt | `User-agent: * / Allow: / / Sitemap: <abs URL>` | Supported | blog: `layouts/robots.txt` · signal: `[robots]` plans `robots.txt` with an allow-all policy plus the `Sitemap:` line whenever the sitemap exists (gaps.md #5 resolved) | none |
| Search index `/index.json` | Full-text projection of all regular pages feeding client-side search | Supported | blog: `layouts/index.json` · signal: versioned `index.json` with equivalent fields (`title/description/content/url/collection/date/tags` vs. `permalink/section/topics`; wrapped in `{version, documents}`) | none in engine; theme search JS reads Signal's schema in the port |
| Search UI | Overlay (Ctrl/Cmd+K), FlexSearch, lazy-loaded | External/theme-only | blog: `_partials/search.html`, `static/js/search.js`, vendored FlexSearch · signal: static passthrough + index artifact only | none |

### Metadata

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Canonical URLs | `<link rel="canonical">` everywhere | Supported | blog: `head.html` · signal: `canonical_url` on every page kind | none |
| OpenGraph | `og:type` article/website, title/description/url/image | Supported | blog: `head.html` · signal: `og_*` context keys, omitted when absent | none |
| JSON-LD | `Article` with `datePublished`/`dateModified`/author; `WebPage` elsewhere | Supported | blog: `head.html` · signal: `article_json_ld`/`webpage_json_ld`; `dateModified` from `last_modified` (front matter or Git-derived — see Git info row) | none |
| JSON-LD shape | Structured-data object in the head | Partial | real-build finding: the blog's live Hugo output double-encodes the JSON-LD (a quoted, escaped JSON *string* — invalid as structured data); Signal emits a proper object. Hugo also uses RFC-3339 timestamps (`2026-09-05T16:51:46+02:00`) where Signal emits bare dates (`2026-09-05` — valid ISO 8601) | none (double-encoding is Hugo-specific and should not be reproduced; date granularity is an acceptable difference) |
| Meta description | Per-page description, falling back to the site description | Supported | blog: `head.html` uses `.Site.Params.description` fallback · signal: `site.description` → `site_description` context key on every template-rendered page; templates fall back with `{{ description | default(site_description) }}` (gaps.md #10 resolved) | none |
| Site params | `author`, `role`, `tagline`, `description`, `showReadingTime`, `showRelated` feed header/footer/metadata/behavior switches | Partial | blog: `hugo.toml [params]` · signal: `author` supported; `related` covers `showRelated`'s engine side; `description` now supported as `site_description`; copy-like params (`role`, `tagline`) can live as template constants | Template constants for copy |
| Twitter card / theme-color / RSS alternate | Static meta/link tags in head | External/theme-only | blog: `head.html` | none |

### Navigation and assets

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Main menu | Weight-ordered items (Articles, Projects, About) incl. active state | Supported | blog: `[menus.main]` weights · signal: config order = display order (site config lists items in weight order), `menus.main` with `{label,url,active}` | none |
| Static assets | `js/` (incl. vendored Mermaid/FlexSearch/medium-zoom/KaTeX), `fonts/`, `images/`, `favicon.svg`, vendored KaTeX CSS | Supported | blog: `static/**` · signal: verbatim static passthrough | none |
| CSS pipeline | `assets/css/*.css` concatenated, minified, fingerprinted, served with SRI integrity | Missing | blog: `head.html` `resources.Concat/minify/fingerprint` · signal: static passthrough only — no bundling/fingerprint/integrity | Asset pipeline or external pre-bundling (gaps.md #9) |
| HTML minification | `hugo --minify` output | Supported | signal: opt-in `[output] minify_html` — deterministic HTML-aware minification preserving code, Mermaid, scripts, JSON-LD, entities, attributes, comments | none (serialization differs by implementation, not semantics; gaps.md #13 resolved) |

### Dates, reading time, Git

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Date display | "2 January 2006" bylines; "2 Jan 2006" cards; "02 Jan 2006" project list — three formats across contexts | Supported | blog: `Date.Format` calls · signal: `date_format` template filter over the shared strftime-style subset, alongside the site-wide `date_formatted` default (gaps.md #12 resolved; Go layout syntax intentionally not reproduced) | none |
| Reading time | "N min read" / "N minutes" next to dates | Partial | blog: `showReadingTime` · signal: `reading_time` = ceil(source words/200); Hugo counts rendered words — real-build observation: 20 vs 22 minutes on the long article, 19 vs 19 on the shorter one (accepted-diff, `docs/migration/hugo.md`) | Accepted difference, or align counting |
| Git information | `enableGitInfo`: "Updated <date>" in byline and article info, JSON-LD `dateModified`, sitemap `lastmod` from last commit touching the file | Supported | blog: hugo.toml; content commit dates (2026-09-05+) differ from front-matter dates (2026-09-02/03), so the live site renders "Updated" · signal: opt-in `[git] last_modified = true`; front-matter `lastmod` wins; advisory when Git is unavailable ([ADR 0024](../adr/0024-git-last-modified.md)) | none |
| Ordering determinism | Same-date entries (article vs. project on 2026-09-02) ordered stably on shared surfaces | Partial | blog: Hugo tie-breaks put the project first on shared term pages · signal: slug-ascending tie-break puts the article first (BR-1) | Accepted difference (documented) |

### Client behavior and accessibility (theme port)

| Area | Blog requirement | Signal status | Evidence | Required work |
| ---- | ---------------- | ------------- | -------- | ------------- |
| Light/dark mode | localStorage + `prefers-color-scheme`, `theme-color` metas, Mermaid re-render on toggle | External/theme-only | blog: inline head script, `main.js` · signal: static passthrough | none |
| TOC scroll-spy | IntersectionObserver active-heading highlight | External/theme-only | blog: README customization notes | none |
| Copy buttons | Code-block copy (server-rendered hook ✓) and copy-article-link | Supported / External | blog: `data-code-copy` + `data-copy-url` · signal: engine emits the code copy button; the share-rail button is template-side | none |
| Image zoom | medium-zoom on article images | External/theme-only | blog: `articles/single.html` scripts · signal: static passthrough | none |
| Responsive/mobile layout | Breakpoint-driven sidebar collapse etc. | External/theme-only | blog: `assets/css/` | none |
| Accessibility | Skip link, aria-labels, alert roles, reduced-motion, focus states | External/theme-only | blog: `baseof.html`, CSS · signal: engine-side alert `role` attributes are emitted (see alerts row) | none |

## Real-build baseline (Slices 1–3 complete, theme port verified)

The experiment behind this inventory's current status column: the blog was
built with Signal from a disposable Git clone (real history, so
`[git] last_modified` exercised live data) using a `signal.toml` plus a
MiniJinja theme port that live **outside** Signal, then compared against
`hugo` output for the same commit.

Headline results: every content page, section, term page, term feed, the
home page, search index, and sitemap generate and match (sitemap URL sets
identical 14/14; term-feed membership identical; Git-derived "Updated" dates
identical; home hero image, topic eyebrows, list/card dates, and
meta-description fallback verified byte-equal in content after the Slice 3
capabilities). The remaining artifact-set difference is the still-open
gap (CSS pipeline)
plus the documented search-index schema adaptation. Notable verified nuances: the
blog's live Hugo JSON-LD is double-encoded (Hugo-specific, not reproduced),
reading time diverges only on the longest article (20 vs 22 minutes), and
reproducing the blog's strict 2-level TOC requires gating Signal's recursive
`Toc` on `item.level` — which the engine exposes.

## Present in the theme, never exercised by content

Recorded so nobody implements them for this site's sake (the blog's own rule:
"do not assume Hugo behavior is required merely because Hugo provides it"):

- Shortcodes `card`, `details`, `step`, `steps`, `tab`, `tabs` — templates
  exist, zero invocations in `content/`.
- KaTeX math pipeline — fully wired (partial, vendored assets, SRI), but no
  article sets `math: true` and no `$$`/`\(`/`\[` delimiters appear.
- Code-block attributes (`filename`, `linenos`, `hl_lines`) — documented,
  unused.
- `eyebrow` section/page param — referenced by layouts, never set in content.
- A `callout` shortcode — mentioned in the blog README's layout list only; no
  template exists and WRITING.md explicitly directs authors to alerts.

If future articles start using these, the gaps to revisit are: front-matter
`extra` exposure (math/toc gating), shortcode support (none), and fenced-block
attribute parsing (none).

## First implementation slice (delivered)

The blog already built against Signal at inventory time; the slice named in
the inventory is now implemented as generic capabilities:

1. **Non-typed front matter is template-visible.** Unknown fields survive
   ingestion (`ContentEntry.extra`, deterministic `BTreeMap`) and reach
   entry and section templates as `extra`, covering per-article TOC gating
   (`extra.toc`) and the project "View on GitHub" link (`extra.repo`) with
   no blog-specific typing.
2. **Alerts carry ARIA roles** (`role="alert"` for warning/caution,
   `role="note"` otherwise), and **Mermaid blocks keep a no-JS fallback**:
   the escaped source is duplicated in a collapsed
   `details.mermaid-source` next to the unchanged `pre.mermaid` render
   target.
3. **Git-derived `last_modified`** behind `[git] last_modified = true`
   (front-matter `lastmod` wins; advisory when Git is unavailable) —
   restoring "Updated" bylines, JSON-LD `dateModified`, and sitemap
   `lastmod`. See [ADR 0024](../adr/0024-git-last-modified.md).
4. **Related entries** reach entry templates as `related`: strongest
   shared-topic overlaps first, capped via `[related] limit` (default 3),
   deterministic, and manifest-tracked so pages rebuild when a listed
   entry changes.

Slice 2 (delivered): section + taxonomy-label RSS feeds, robots.txt, and the
themed 404 artifact.

Slice 3 (delivered): authored topic display order (`tag_order`), the
`site.description` → `site_description` fallback key, and the `date_format`
template filter. Minification slice (delivered): opt-in `[output]
minify_html` (gaps.md #13 resolved). Remaining candidate, not blocking
rendering: CSS pipeline.

See [gaps.md](gaps.md) for the numbered list with concrete behaviors.
