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
canonical_url
og_title
og_description
og_url
og_type
og_image
json_ld
menus
```

Listing pages receive entry summaries rather than the complete site model.

## Auto-escaping

HTML templates are auto-escaped. Rendered Markdown body content is already HTML, so it is inserted using:

```html
{{ content | safe }}
```

Do not mark untrusted metadata as `safe`.

## Rendering boundary

Signal loads templates into memory before rendering. Template code has no implicit filesystem or network loader.
