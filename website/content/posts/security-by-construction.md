---
title: Security by construction
date: 2026-09-18
description: Signal treats templates, URLs, routes, and output paths as explicit security boundaries.
topics:
  - Security
  - Architecture
---

# Security by construction

A static generator still handles untrusted inputs: Markdown, front matter, routes, template data, and filesystem state.

Signal makes important safety properties part of the data flow.

## Author-controlled URLs

Markdown links and images use a shared allow-list. Relative destinations and `http(s)` are allowed; executable schemes such as `javascript:`, `data:`, and `vbscript:` are rejected.

Raw author HTML is detached at parse, so Markdown cannot simply inject an arbitrary element.

## Template boundary

Templates receive explicit render contexts. The renderer loads template source into memory and HTML templates are auto-escaped.

A template does not receive an implicit filesystem or network loader.

## Output boundary

Every artifact goes through one write boundary. Relative paths are checked for unsafe components, symlinked ancestors are rejected, and stale pruning only considers paths recorded by the previous build manifest.

These boundaries make the build easier to reason about and test.
