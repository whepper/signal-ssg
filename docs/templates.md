# Templates

Signal uses MiniJinja templates stored below the site's `templates/` directory.

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

## Contexts

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
responsive_image
content
toc
reading_time
tags
related
has_mermaid
extra
canonical_url
og_title
og_description
og_url
og_type
og_image
twitter_card
twitter_title
twitter_description
twitter_image
social_image
json_ld
menus
```

Keys appear only when their inputs exist. `site_description` carries the
configured site description on every template-rendered page; templates use
it as the fallback behind a page's own `description` (e.g.
`{{ description | default(site_description) }}`). `tags` lists taxonomy
terms in authored front-matter order (first-occurrence deduplicated), which
is what eyebrows and "first topic" picks render. `extra` carries the entry's
non-typed front matter verbatim (e.g. `extra.repo`, `extra.toc`) and is
absent when the entry defines none. `related` carries up to
`[related] limit` entry summaries (title, route, date, tags, image, reading time)
sharing the most topics; it is absent when nothing overlaps. `has_mermaid`
gates client-side diagram loaders per page. `responsive_image` carries the
front-matter hero expressed as planned derivatives (flat fallback fields
plus per-format `sources` and a `has_picture` flag) on sites that
configure `[images]`; body images need no template support, since Comrak
`<img>` tags gain `srcset`/`sizes`/dimensions automatically.

### Homepage featured image

When `home_collection` selects a featured entry, the homepage's existing
`featured` summary may also carry `featured.responsive_image`. It has the same
shape as an entry page's `responsive_image`, but is resolved only for the full
entry actually selected as the featured hero. It is absent when there is no
featured entry, the hero is external or non-raster, or `[images]` is disabled.
`featured.image` and `featured.image_alt` remain available as the plain-image
fallback:

```html
{% if featured %}
  {% if featured.responsive_image %}
    {% if featured.responsive_image.has_picture %}
      <picture>
        {% for source in featured.responsive_image.sources %}
          <source type="{{ source.mime }}" srcset="{{ source.srcset }}">
        {% endfor %}
        <img
          src="{{ featured.responsive_image.src }}"
          srcset="{{ featured.responsive_image.srcset }}"
          sizes="{{ featured.responsive_image.sizes }}"
          width="{{ featured.responsive_image.width }}"
          height="{{ featured.responsive_image.height }}"
          alt="{{ featured.responsive_image.alt }}"
        >
      </picture>
    {% else %}
      <img
        src="{{ featured.responsive_image.src }}"
        srcset="{{ featured.responsive_image.srcset }}"
        sizes="{{ featured.responsive_image.sizes }}"
        width="{{ featured.responsive_image.width }}"
        height="{{ featured.responsive_image.height }}"
        alt="{{ featured.responsive_image.alt }}"
      >
    {% endif %}
  {% elif featured.image %}
    <img src="{{ featured.image }}" alt="{{ featured.image_alt | default('') }}">
  {% endif %}
{% endif %}
```

With multiple configured formats, `sources` is AVIF-first and
`has_picture` is true. With one format it is false, so the template can render
the fallback fields directly as a responsive `<img>`. Missing `image_alt`
becomes an empty `responsive_image.alt`; the plain fallback uses the same
`default('')` rule.

When the site configures `[social]`, participating entry pages receive a
generated social image: `og_image` and `twitter_image` carry its absolute URL
(superseding the hero), `twitter_card` is `summary_large_image`,
`twitter_title`/`twitter_description` mirror the Open Graph values, and
`social_image` is an object with `url` (URL-path form), `absolute_url`,
`width`, and `height`. All of these are absent on unconfigured sites and
on pages that opt out (`social_image: false`), so the keys double as the
gate: `{% if social_image %}`.

Listing pages receive entry summaries rather than the complete site model.
Summaries carry the same display fields everywhere (title, description,
date, route, tags in authored order, hero `image`/`image_alt` in URL-path
form, reading time): heroes, cards, and listings all render from one
projection. Only the homepage's `featured` object adds
`responsive_image`; section, taxonomy, feed, recent-list, and related
summaries do not.

## Date formatting

Templates format stored `YYYY-MM-DD` dates with the `date_format` filter:

```html
<time datetime="{{ date }}">{{ date | date_format("%-d %B %Y") }}</time>
```

Supported verbs: `%Y` (2026), `%m` (09), `%d` (02), `%-d` (2), `%B`
(September), `%b` (Sep), `%%` (literal `%`). Anything else passes through
literally. Missing, non-string, and invalid dates render as the empty
string, so the filter applies safely to optional dates. Month names are
fixed English; formatting never depends on machine locale or timezone.

This differs deliberately from Hugo, which formats with Go reference-date
layouts (`"2 January 2006"`): Signal templates use the strftime-style
subset above, shared with the `site.date_format` setting.

## Auto-escaping

HTML templates are auto-escaped. Rendered Markdown body content is already HTML, so it is inserted using:

```html
{{ content | safe }}
```

Do not mark untrusted metadata as `safe`.

## Rendering boundary

Signal loads templates into memory before rendering. Template code has no implicit filesystem or network loader.
