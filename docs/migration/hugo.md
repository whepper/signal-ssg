# Hugo feature map — dogfooding knowledge

Distilled from dogfooding Signal against a Hugo site. A feature is **used**
only if live site sources prove it — never because Hugo supports it.
"Signal support" describes product capability; migration slices are Signal's
roadmap, not the site's plan.

```text
Feature | Used? | Signal support (slice 1) | Migration strategy | Status
```

| Feature | Used? | Signal support | Migration strategy | Status |
|---|---|---|---|---|
| Section lists (`.ByDate.Reverse`, no pagination) | yes | `SectionIndex` + `entries_in_collection_by_date`; `first-N` limits only, verified no paginator | native | done |
| Home page (featured hero + latest grid) | yes | `Home` generator (newest featured + up to 8 recent) over `home_collection` | native | done |
| Section titles/descriptions | yes | `_index` entry wins, else collection `title`/`description` config | native | done |
|---|---|---|---|---|
| YAML front matter (`---`) | yes | `signal-markdown::split_front_matter` | native | done |
| TOML front matter (`+++`) | yes (minority of pages) | same parser, incl. bare-date datetimes | native | done |
| `title` / `description` / `date` | yes | `ContentEntry` fields, required title | native | done |
| `topics` taxonomy (sole) | yes | merged into `tags` index + `entries_tagged`; `[taxonomy]` presents it with live terms | native | done |
| Topic term pages (`/topics/<slug>/`, newest-first, cross-collection) | yes | `TopicTerms` + `EntrySummary` reuse; URLs/labels/order parser-verified identical | native | done |
| `tags` / `categories` | no | accepted as alias into `tags` | — | done |
| `draft` | yes (convention) | skipped at ingest | native | done |
| `slug` override | fixture only | slug override, else filename stem verbatim | native | done |
| `_index.md` branch bundles | yes (section content pages) | section-root routing in ingest | native | done |
| `featured` / `image`+`image_alt` | yes | `featured` drives home hero; `image` is normalized to site-root URLs and unsafe schemes are rejected at ingest; the selected homepage hero additionally exposes `featured.responsive_image` from the same optional `[images]` pipeline used by entry pages, with `featured.image` as fallback | native | done |
| `toc` / heading IDs + anchors | yes | deterministic anchors (`signal-markdown::anchor_base`: Unicode-lowercase; keep alphanumerics, `-`, `_`; each whitespace run → one `-`; all other punctuation dropped with no separator emitted; leading/trailing `-` trimmed; empty → `"section"`; repeats gain `-1`, `-2`, …) + `Toc` projection from normalized headings; H1 excluded, H2–H6 nested; no config. Engine difference vs Hugo: punctuation-adjacent runs collapse, e.g. Hugo `v041--v042` vs Signal `v041-v042` (BR-1); fragment links are consistent within each engine | native | done |
| `math` (KaTeX, self-hosted, gated) | no live page | captured in `extra` | slice 8, gated asset loading | gap |
| `author` / `lastmod` | yes | typed `author` / `last_modified` (entry override → site default → omitted); JSON-LD `dateModified` | native | done |
| Canonical URLs (`base_url` + route) | yes | derived in projection, exposed as `canonical_url`; route stays distinct | native | done |
| OpenGraph (`og:title/description/url/type/image`) | yes | explicit `og_*` context keys, omitted when unavailable; `og:site_name` not used upstream, omitted | native | done |
| JSON-LD (`Article` / `WebPage`) | yes | `serde_json`-built, `</`-escaped, only available fields; no schema framework | native | done |
| `repo` (projects) | yes | captured in `extra` | slice 2 project template | gap |
| `eyebrow` / `description` section params | layouts only | `description` supported | slice 2 | partial |
| Date-prefixed filenames → URL slug | yes | stem kept verbatim (date prefix included) | native | done |
| Menus (`menus.main`) | yes | config-driven `MenuConfig` resolved per page to `{label, url, active}`; internal/external validation, exact-match active, no artifact | native | done |
| Layouts / partials / `baseof` inheritance | yes | MiniJinja `extends` in fixture | slice 2 theme port | partial |
| Render hooks: codeblock (Chroma+copy+attrs) | yes | syntect class-span highlighting + `div.code-block` wrapper with `data-language` and server-rendered copy button; per-block filename/linenos/hl-lines attrs deliberately out (no live usage) | native | done |
| Render hooks: mermaid (diagram+fallback+`page.Store` gate) | yes | source preserved as escaped text in `pre.mermaid` for the site's client renderer; `has_mermaid` context gates loader inclusion; no build-time browser | native | done |
| Render hooks: blockquote alerts (`[!WARNING]`) | yes | NOTE/TIP/IMPORTANT/WARNING/CAUTION → semantic `<aside>` at AST level; unknown kinds stay blockquotes | native | done |
| Shortcodes (`card`, `details`, `steps`, `tabs`) | **no** (defined in theme, unused by content) | none | implement on first real use | deferred |
| Goldmark smart quotes (`&ldquo;`) | incidental | Comrak emits plain entities | accept engine difference | accepted |
| Reading time | yes | `ceil(words/200)`, min 1 (source words) | Hugo counts rendered words — observed to differ by a minute on long pages; accept or align in slice 5 | accepted-diff |
| Date formatting | yes | `YYYY-MM-DD` stored; `site.date_format` controls presentation, locale- and timezone-free | native | done |
| Related content (topics-weighted) | yes | `entries_tagged` index exists | slice 4 projection | gap |
| RSS (home + section + taxonomy, limit 20) | yes | the full feed family from normalized data: main feed, one feed per collection, the taxonomy label-index feed, plus per-term feeds; `[feed]` opt-in with limit (default 20). Ordering contract: dated entries newest-first, undated last, ties broken by slug ascending then `ContentId` (`compare_by_date_desc`; BR-1 observed Hugo placing a same-date project page before an article where Signal applies the slug tie-break) | native | done |
| Taxonomy RSS (`topics/index.xml`, per-term) | yes | label-index feed (`<root>/index.xml`, one item per term stamped with the newest member's date) plus per-term feeds (`<root>/<slug>/index.xml`) | native | done |
| Sitemap / robots.txt / 404 | yes | sitemap from the explicit route inventory; robots.txt from the `[robots]` presence gate (allow-all + `Sitemap:` line when the sitemap exists); themed `404.html` from `site.not_found_template` through the normal template pipeline (routeless, `TemplateSet` + config inputs) | native | done |
| Search (`index.json` + FlexSearch) | yes | versioned static `index.json` from normalized data (regular entries, route-ordered); no engine or UI in Signal — client integration is external | native | done |
| CSS pipeline (concat/minify/fingerprint+SRI) | yes | `static/` verbatim passthrough | slice 7: fingerprinting, if warranted | gap |
| Static passthrough (`static/` → root) | yes | verbatim copy in `build_site` | native | done |
| Image processing (`resources.*`) | no (CSS-only `resources.Get`) | optional `[images]` derivatives and `[social]` cards are available but are not used by this site | no migration required; revisit if content requires it | not-used |
| Aliases / redirects | no | none | only if content requires them | deferred |
| Pagination | no (`first N` limits only) | none | only if content requires it | deferred |
| i18n / `enableGitInfo` / HTML minify | config-only | none | per-feature decision later | deferred |
