# Content

Signal content is Markdown with front matter.

## Front matter

YAML:

```yaml
---
title: Hello, Signal
description: A short summary.
date: 2026-09-19
author: Jane Doe
topics:
  - Rust
  - Static Sites
featured: true
image: /images/hello.png
image_alt: Signal logo
---
```

TOML:

```toml
+++
title = "Hello, Signal"
date = 2026-09-19
topics = ["Rust", "Static Sites"]
+++
```

## Fields

| Field | Purpose |
|---|---|
| `title` | Page title; required |
| `description` | Short summary |
| `date` | Publication date, `YYYY-MM-DD` |
| `lastmod` | Last-modification date |
| `author` | Entry-specific author |
| `draft` | Exclude the entry from the build |
| `featured` | Make the entry eligible for the home-page hero |
| `image` | Hero image |
| `image_alt` | Hero image alternative text |
| `topics` / `tags` | Taxonomy terms |
| `slug` | Override the filename-derived slug |

Unknown fields are retained for future features.

## Sections

A collection can use `_index.md` as its section-root document.

## Markdown presentation

Signal currently supports deterministic heading anchors and TOC projection, syntax-highlighted code blocks, Mermaid source blocks for client-side rendering, semantic Markdown alerts, and safe author-controlled links and images.

Author raw HTML is not passed through unchanged.

## URLs

Relative destinations and `http(s)` are allowed. Executable or unsupported schemes such as `javascript:`, `data:`, and `vbscript:` are rejected.

## Ordering

Generated listings use deterministic newest-first ordering for dated entries, followed by stable ordering for undated entries.
