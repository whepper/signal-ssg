# Page inspection

`signal inspect` is a read-only, page-scoped machine-readable view of the
understanding Signal has after ingesting and validating a site. It is useful
to coding/content agents because it exposes resolved publishing facts rather
than asking an agent to reconstruct them from Markdown, configuration, and
build behavior.

It is deliberately not a repository browser and not a second content model.

## Invocation

Select a canonical route or a model source reference:

```sh
signal inspect --root my-site --format json /posts/hello-world/
signal inspect --root my-site --format json content/posts/hello-world.md
```

`--root` defaults to `.`. `--format json` is currently the only public format.
On validation failure or an unknown/non-entry selector, the command emits the
existing miette diagnostic on stderr, produces no JSON document, and exits
nonzero. The command runs Signal's read-only validation prefix: configuration
loading, ingestion, model freeze, structural validation, template loading, and
reference validation. It does not require an output directory, build artifacts,
a manifest, or network access. Drafts and other sources excluded during ingest
are not available through this command; an unknown or unpublished selector is
an error.

The source-reference form is a root-relative path such as
`content/posts/hello-world.md`; it is matched against the model identity
`(collection, source-relative-path)`. A bare collection-relative path such as
`hello-world.md` is intentionally not accepted because it can be ambiguous
across collections. It is not a filesystem read, so a selector cannot escape
the selected site root or disclose an unrelated file.

## Why this is not repository access

An agent can read the Markdown directly, but the source does not state all of
the following in the form Signal publishes it:

- the collection route prefix and filename/slug rules;
- section-root handling for `_index.md` and `index.md`;
- the effective author fallback from `site.author`;
- canonical URLs and URL-path encoding;
- normalized image URLs and authored-versus-sorted taxonomy order;
- parse-time heading ids, including duplicate and Unicode normalization;
- document-relative link resolution and validated fragment targets;
- the existing shared-tag related-entry projection;
- deterministic page-scoped advisory diagnostics.

The interface returns those values as a bounded public projection. OpenCode
can still use its normal repository tools to read and edit the source. Signal
does not interpret Markdown, front matter, templates, or diagnostics as agent
instructions.

## Public schema: `signal.inspect/v1`

The JSON root is an object with `schema` and one `page` object:

```json
{
  "schema": "signal.inspect/v1",
  "page": {
    "source": { "collection": "posts", "path": "alpha.md" },
    "route": "/posts/alpha/",
    "url": "/posts/alpha/",
    "publication": { "state": "published" }
  }
}
```

The following fields are the stable v1 inspection contract:

| Field | Meaning |
|---|---|
| `schema` | Always `signal.inspect/v1`; bumped for a breaking contract change. |
| `page.source` | Stable collection and collection-relative source reference. |
| `page.collection` | Owning collection. |
| `page.route` | Signal's logical route. |
| `page.url` | URL-path form used by rendered links and search documents. |
| `page.canonical_url` | Present when `site.base_url` is configured. |
| `page.publication.state` | `published`; draft sources never enter this view. |
| `page.title`, `description`, `date`, `last_modified` | Effective typed metadata. Dates remain `YYYY-MM-DD`. |
| `page.author` | Entry override, falling back to configured site author. |
| `page.image`, `image_alt`, `featured`, `tags`, `word_count` | Other effective, normalized page facts. `image_alt` is included only when an effective hero image exists; `tags` preserves authored display order; `featured` is the page flag, while actual home-hero eligibility also depends on the configured home collection. |
| `page.headings` | Bounded normalized headings and fragment ids. |
| `page.outbound_links` | Bounded links from the normalized body; internal targets are resolved using the same rules as validation. |
| `page.inbound_links` | Bounded model-represented internal links pointing to this route. |
| `page.assets` | Bounded, sorted, deduplicated source asset paths referenced by the page. |
| `page.related` | Bounded existing shared-tag related projection, with source references and shared terms. |
| `page.diagnostics` | Bounded existing advisory diagnostics whose subject is this route. |

Every bounded collection is an object with `items`, `total`, and `truncated`.
The current bound is 100 items per collection. The contract reports truncation
rather than silently changing the meaning of the result.

The item shapes are also part of v1:

- `outbound_links.items[]` has `raw`, `kind` (`internal` or `external`), and
  an optional `target`; internal targets are canonical logical routes or
  output-relative generated files, and an optional `fragment` is preserved
  without the leading `#`. External links have no `target`.
- `inbound_links.items[]` has a `source` reference and the authored `raw`
  destination. It includes model-represented inline links, including a
  self-link when present.
- `related.items[]` has a stable `source`, URL-path `route`, `title`, and
  sorted `shared_tags` that explain the existing shared-tag candidate.
- `diagnostics.items[]` has the existing stable `code`, advisory `severity`,
  `subject`, and measured `message`.
- `assets.items[]` contains sorted, deduplicated `static/`-relative source
  asset paths. External image URLs are not assets.

The interface does not include rendered HTML, Markdown, normalized body plain
text, code blocks, unknown front-matter fields, template contents, arbitrary
repository files, binary assets, or the whole site in one response. The source
file remains the source for those things. This keeps the operation useful for
context without turning it into a content or repository dump.

Outbound and inbound links are the inline links represented by Signal's
normalized `RenderedBody`. Reference-style links, shortcut links, bare URLs,
template-literal URLs, and raw HTML remain outside the structured model, just
as they are for `signal check`.

## Example: `fixtures/sample-site`

From the repository's real `fixtures/sample-site`, this command:

```sh
signal inspect --root fixtures/sample-site --format json /posts/alpha/
```

produces the following result (JSON whitespace is compacted here for
readability; field values are the real command output):

```json
{
  "schema": "signal.inspect/v1",
  "page": {
    "source": {
      "collection": "posts",
      "path": "alpha.md"
    },
    "collection": "posts",
    "route": "/posts/alpha/",
    "url": "/posts/alpha/",
    "canonical_url": "https://example.com/posts/alpha/",
    "publication": {
      "state": "published"
    },
    "title": "Alpha Post",
    "description": "The newer featured post.",
    "date": "2026-02-01",
    "last_modified": "2026-02-15",
    "author": "Guest Writer",
    "image": "/images/alpha.svg",
    "image_alt": "Abstract synthetic diagram",
    "featured": true,
    "tags": ["Rust", "Systems"],
    "word_count": 176,
    "headings": {
      "items": [
        { "level": 2, "text": "Getting Started", "id": "getting-started" },
        { "level": 3, "text": "What's new?", "id": "whats-new" },
        { "level": 2, "text": "Getting Started", "id": "getting-started-1" },
        { "level": 2, "text": "Über den Tellerrand", "id": "über-den-tellerrand" },
        { "level": 3, "text": "Using code and emphasis", "id": "using-code-and-emphasis" },
        { "level": 2, "text": "Field Notes in Code", "id": "field-notes-in-code" }
      ],
      "total": 6,
      "truncated": false
    },
    "outbound_links": {
      "items": [
        { "raw": "/posts/beta/", "kind": "internal", "target": "/posts/beta/" },
        { "raw": "/posts/beta/", "kind": "internal", "target": "/posts/beta/" }
      ],
      "total": 2,
      "truncated": false
    },
    "inbound_links": {
      "items": [],
      "total": 0,
      "truncated": false
    },
    "assets": {
      "items": ["images/alpha.svg"],
      "total": 1,
      "truncated": false
    },
    "related": {
      "items": [
        {
          "source": { "collection": "projects", "path": "gadget.md" },
          "route": "/projects/gadget/",
          "title": "Gadget Project",
          "shared_tags": ["Systems"]
        },
        {
          "source": { "collection": "posts", "path": "beta.md" },
          "route": "/posts/beta/",
          "title": "Beta Post",
          "shared_tags": ["Rust"]
        }
      ],
      "total": 2,
      "truncated": false
    },
    "diagnostics": {
      "items": [],
      "total": 0,
      "truncated": false
    }
  }
}
```

The duplicate outbound links are intentional: the fixture contains two
inline links to Beta. Inspection preserves authored link order and does not
deduplicate it. The two related pages are Signal's deterministic shared-tag
projection, not editorial recommendations.

## Authoring and audit workflows

### Authoring

1. Read the selected source page with the editor's normal repository tools.
2. Run `signal inspect ... --format json` before editing if exact route,
   effective metadata, heading ids, publication state, or existing links
   matter.
3. Make the smallest source edit that preserves the requested meaning, voice,
   and metadata.
4. Run `signal check` and the relevant tests/build.
5. Review the diff before applying or publishing it.

Signal does not rewrite Markdown, front matter, or generated output.

### Audit

Ask an agent to separate:

- deterministic facts and diagnostics returned by Signal;
- editorial opportunities it proposes, such as clarity, organization, or
  potentially useful links.

The agent may use the inspection's exact routes, heading ids, related pages,
and diagnostic subjects to ground its suggestions. Those suggestions remain
agent/editorial judgements; Signal never turns them into diagnostics and never
assigns a content-quality score.

## Diagnostics and publication semantics

`inspect` reuses `signal_cli::diagnostics::analyze`, the same advisory
analysis used by `check` and `explain`. It does not add a diagnostic engine,
plugin registry, configurable threshold, artifact, manifest field, or
persisted report. Hard validation failures still fail the command before JSON
is emitted.

Only published model entries can be selected. A draft is not an alternate
inspection mode and is not exposed through an unpublished-content escape
hatch. This prevents a normal context request from silently disclosing content
that Signal would not publish.

## Security, privacy, and trust boundary

Inspection is deterministic and read-only. It performs no network access,
modifies no source or output file, changes no build manifest, and does not
follow a page selector as an arbitrary filesystem path. It stays within the
existing ingestion and reference-validation boundary, including Signal's
existing Markdown/template symlink-following policy. The operator-selected
site root and any symlink targets reachable through that policy remain part
of the trust boundary; the inspection command adds no new path-following
behavior.

Markdown, front matter, templates, and content-derived values are untrusted
data. The JSON interface preserves them as data; it never evaluates them as
instructions. The bounded typed projection intentionally omits bodies,
unknown front matter, templates, and arbitrary files to reduce exposure.

The command is local like the rest of Signal's filesystem pipeline. An agent
or model using it may be local or remote, so the caller must still apply its
own normal policy for sending repository content and command output to a model.
Signal does not claim that structured output makes an agent trustworthy.

MCP is not part of this first capability. A client can invoke the deterministic
CLI directly; an MCP adapter would be justified only if a real workflow shows
that the CLI invocation boundary is materially worse for clients. Signal does
not need to run continuously for the CLI operation.

## Deliberately deferred

- body/plain-text excerpts or bounded body requests;
- whole-site inspection responses;
- mutation, publishing, deployment, or commit operations;
- arbitrary repository/file access;
- semantic search, embeddings, LLM calls, or automatic editorial judgments;
- an MCP server;
- a public quality score or diagnostic suppression/configuration system.
