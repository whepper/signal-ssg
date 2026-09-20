---
title: Templates
description: Template selection, inheritance, contexts, filters, and escaping.
---

Signal uses MiniJinja templates stored below the site's `templates/` directory. Templates receive explicit, read-only values — never the model itself, never the filesystem, never the network.

## Which template renders what

Each artifact kind selects its template by configuration, with a fixed default:

| Page | Template setting | Default |
|---|---|---|
| Entry page | collection `template` | `post.html` |
| Section listing | collection `section_template` | `section.html` |
| Home page | `site.home_template` | `home.html` |
| Taxonomy index | taxonomy `index_template` | `topics.html` |
| Taxonomy term | taxonomy `term_template` | `topic.html` |
| Not-found page | `site.not_found_template` | (opt-in only) |

See [Configuration](../configuration/) for the corresponding tables. Feeds, the sitemap, robots.txt, and the search index never touch templates: they serialize from normalized data through fixed schemas.

## Inheritance

Templates can use MiniJinja inheritance and includes:

```text
templates/
├── base.html
├── home.html
├── post.html
└── section.html
```

A page can extend a shared layout:

```html
{% extends "base.html" %}

{% block content %}
<article>
  <h1>{{ title }}</h1>
  {{ content | safe }}
</article>
{% endblock %}
```

A change to *any* loaded template conservatively invalidates every template-rendered artifact (the build tracks the complete loaded set, not a per-page closure). This is deliberate: it stays sound without parsing template sources. See [Incremental builds](../builds/).

## Entry contexts

Entry pages can receive values including:

```text
site_title
site_description
title
description
date
date_formatted
last_modified
last_modified_formatted
author
image
image_alt
content
toc
reading_time
tags
related
has_mermaid
extra
route
collection
canonical_url
og_title
og_description
og_url
og_type
og_image
json_ld
menus
```

Keys appear only when their inputs exist — metadata is omitted, never fabricated. `site_description` carries the configured site description on every template-rendered page; templates use it as the fallback behind a page's own `description` (e.g. `{{ description | default(site_description) }}`).

- `tags` lists taxonomy terms in authored front-matter order (first-occurrence deduplicated), which is what eyebrows and "first topic" picks render.
- `extra` carries the entry's non-typed front matter verbatim (e.g. `extra.repo`, `extra.eyebrow`) and is absent when the entry defines none.
- `related` carries up to `[related] limit` entry summaries (title, route, date, tags, image, reading time) sharing the most topics; it is absent when nothing overlaps.
- `toc` carries the heading hierarchy with fragment ids; it is absent on pages without listable headings.
- `has_mermaid` is present only on pages containing a Mermaid block, so diagram loaders load conditionally.
- `content` is the rendered Markdown body; insert it with `{{ content | safe }}`.
- `route` is the page's route in URL-path (encoded) form, safe to render into links directly.
- `reading_time` is whole minutes derived from the word count.
- `canonical_url` and the `og_*` keys appear when `base_url` (and the relevant inputs) exist; `json_ld` carries an `Article` object.

## Listing contexts

Listing pages receive entry summaries rather than the complete site model. Summaries carry the same display fields everywhere (title, description, date, route, tags in authored order, hero `image`/`image_alt` in URL-path form, reading time): heroes, cards, and listings all render from one projection.

- **Section pages** receive `title`, optional `description` and `content` (from the section-root `_index.md` when present, else collection configuration), `entries` (the collection's summaries), `route`, `collection`, plus shared page metadata (`canonical_url`, website-type OpenGraph tags, `WebPage` JSON-LD) and `menus`.
- **Home page** receives `featured` (one hero summary, when an entry is marked `featured`) and `recent` (recent summaries), with the same shared metadata and `menus`. No `title` key is set: base templates fall back to the bare site title.
- **Taxonomy index** receives `title`, `route`, and `topics` rows (`label`, `slug`, `route`, `count`).
- **Taxonomy term pages** receive `title`, `topic` (the label), `route`, and `entries` (member summaries, newest-first).
- **The 404 page** receives only `site_title`, `base_url`, and `menus` — every menu item renders inactive, since an error page matches no route. No canonical URL, OpenGraph, or JSON-LD is fabricated.

## Menus

When `[menus.main]` is configured, every template-rendered page receives `menus.main`: items in configuration order, each with `label`, `url`, and an `active` flag for the current page. Render navigation like this:

```html
{% if menus %}
<nav aria-label="Main">
  {% for item in menus.main %}
  <a{% if item.active %} aria-current="page"{% endif %} href="{{ item.url }}">{{ item.label }}</a>
  {% endfor %}
</nav>
{% endif %}
```

External menu URLs pass through untouched. Internal targets must resolve to generated routes — see [Reference validation](../validation/).

## Date formatting

Templates format stored `YYYY-MM-DD` dates with the `date_format` filter:

```html
<time datetime="{{ date }}">{{ date | date_format("%-d %B %Y") }}</time>
```

Supported verbs: `%Y` (2026), `%m` (09), `%d` (02), `%-d` (2), `%B` (September), `%b` (Sep), `%%` (literal `%`). Anything else passes through literally. Missing, non-string, and invalid dates render as the empty string, so the filter applies safely to optional dates. Month names are fixed English; formatting never depends on machine locale or timezone.

The preformatted `date_formatted` / `last_modified_formatted` values use the site's `date_format` setting, so most templates never call the filter directly.

## Auto-escaping

HTML templates are auto-escaped. Rendered Markdown body content is already HTML, so it is inserted using:

```html
{{ content | safe }}
```

Do not mark untrusted metadata as `safe`. The `json_ld` value is pre-serialized and likewise inserted with `| safe`.

## Rendering boundary

Signal loads templates into memory before rendering. Template code has no implicit filesystem or network loader. Literal URLs written directly in templates (scripts, stylesheets) are never parsed or validated by the reference checker — keep them correct by inspection; the repository's own CI double-checks the generated HTML as a backstop.
