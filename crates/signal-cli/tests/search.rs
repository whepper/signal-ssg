//! Static search index (A7.1): the frozen `index.json` contract, its
//! structural normalization, the narrow dependency model, and
//! `signal explain index.json`.
//!
//! These tests pin the *published artifact*, not the Markdown parser: the
//! contract, ordering, exclusions, and rebuild matrix are asserted through
//! a real build, and the explain record is asserted through the real
//! `explain` path. The design decision is ADR 0034.

use signal_cli::build::build_site_from_disk;
use signal_cli::explain::explain_asset_from_disk;
use signal_cli::link_check::check_site_from_disk;
use signal_core::ArtifactKind;
use std::path::Path;

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

fn write_bytes(dir: &Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// Deterministic W × H RGB gradient PNG, encoded in-test.
fn gradient_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        pixel.0 = [
            ((x + u32::from(seed)) % 256) as u8,
            (y % 256) as u8,
            ((x + y) % 256) as u8,
        ];
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("fixture encodes");
    bytes
}

/// One site covering the contract: two collections, a draft, a section
/// root, a hero image, `[images]`, `[social]`, and a normalization probe.
fn search_site(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                r#"[site]
title = "Search"
base_url = "https://example.com/"
author = "Site Author"
date_format = "%Y-%m-%d"

[collections.posts]
source = "content/posts"
route_prefix = "/posts/"

[collections.notes]
source = "content/notes"
route_prefix = "/notes/"

[images]
widths = [640]
format = "webp"

[social]

[menus.main]
items = [
  { label = "Posts", url = "/posts/" },
  { label = "Notes", url = "/notes/" },
]

[feed]
limit = 2

[taxonomy]
route_prefix = "/topics/"
title = "Topics"
"#,
            ),
            (
                "content/posts/alpha.md",
                "---\ntitle: Alpha\ndate: 2026-02-01\ndescription: The alpha summary.\ntopics: [\"Rust\", \"Systems\"]\nimage: images/hero.png\nimage_alt: Hero\n---\n\nAlpha body prose with `inline code`.\n\n## Alpha Heading\n\n```rust\nfn hidden() {}\n```\n",
            ),
            (
                "content/posts/beta.md",
                "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta body prose.\n",
            ),
            (
                "content/posts/draft.md",
                "---\ntitle: Draft\ndraft: true\n---\n\nDraft body must never be indexed.\n",
            ),
            (
                "content/posts/probe.md",
                r#"---
title: Probe
---

Prose with **bold** and *emphasis*.

## Heading Text

A [link label](/posts/beta/) and ![image alt](/images/hero.png).

> [!NOTE]
> Alert body words.

| Col A | Col B |
| --- | --- |
| cell one | cell two |

```rust
fn fenced_code() {}
```

```mermaid
flowchart LR
```

    indented code line

Math inline $x^2$ here.

Line with hard break at end.  
Second line after break.

Paragraph with   repeated    spaces.

- list item one

<div>raw html words</div>

Café naïve 日本語 emoji 🎉 apostrophe's hyphen-word.
"#,
            ),
            (
                "content/notes/_index.md",
                "---\ntitle: Notes\n---\n\nSection body must never be indexed.\n",
            ),
            (
                "content/notes/gamma.md",
                "---\ntitle: Gamma\n---\n\nGamma body prose.\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
            (
                "templates/topics.html",
                "<html><body>topics</body></html>",
            ),
            (
                "templates/topic.html",
                "<html><body>topic</body></html>",
            ),
            ("static/extra.css", "body{}"),
        ],
    );
    write_bytes(dir, "static/images/hero.png", &gradient_png(800, 600, 0));
}

/// Build once, mutate, build again; return the rebuilt path set.
fn rebuilt_after(root: &Path, mutate: impl FnOnce(&Path)) -> Vec<String> {
    let out = root.join("out");
    build_site_from_disk(root, &out).expect("first build");
    mutate(root);
    build_site_from_disk(root, &out)
        .expect("second build")
        .rebuilt_paths
}

fn assert_rebuilds(mutate: impl FnOnce(&Path)) {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let rebuilt = rebuilt_after(dir.path(), mutate);
    assert!(
        rebuilt.iter().any(|path| path == "index.json"),
        "search index must rebuild, rebuilt: {rebuilt:?}"
    );
}

fn assert_reuses(mutate: impl FnOnce(&Path)) {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let rebuilt = rebuilt_after(dir.path(), mutate);
    assert!(
        !rebuilt.iter().any(|path| path == "index.json"),
        "search index must be reused, rebuilt: {rebuilt:?}"
    );
}

fn append(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    let mut body = std::fs::read_to_string(&path).expect("read");
    body.push_str(text);
    std::fs::write(path, body).expect("write");
}

fn replace(root: &Path, rel: &str, from: &str, to: &str) {
    let path = root.join(rel);
    let body = std::fs::read_to_string(&path).expect("read");
    assert!(body.contains(from), "{rel} must contain {from:?}");
    std::fs::write(path, body.replacen(from, to, 1)).expect("write");
}

/// Insert one front-matter line before the closing `---` (appending to the
/// file would land in the body and change `plain_text`).
fn add_front_matter(root: &Path, rel: &str, line: &str) {
    let path = root.join(rel);
    let body = std::fs::read_to_string(&path).expect("read");
    let end = body.find("\n---\n").expect("closing front matter");
    let mut out = String::with_capacity(body.len() + line.len() + 1);
    out.push_str(&body[..end]);
    out.push('\n');
    out.push_str(line);
    out.push_str(&body[end..]);
    std::fs::write(path, out).expect("write");
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[test]
fn search_index_contract_is_frozen() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");

    let json = std::fs::read_to_string(out.join("index.json")).expect("index");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["version"], serde_json::Value::from(1));

    let docs = parsed["documents"].as_array().expect("documents");
    // Drafts never reach the model and section roots are listings, so
    // neither appears; documents are route-ordered.
    let ids: Vec<&str> = docs.iter().map(|d| d["id"].as_str().expect("id")).collect();
    assert_eq!(
        ids,
        vec![
            "/notes/gamma/",
            "/posts/alpha/",
            "/posts/beta/",
            "/posts/probe/"
        ]
    );

    // Every document carries exactly the frozen field set: no more, no
    // fewer (required fields), no `null` optionals.
    for doc in docs {
        let object = doc.as_object().expect("document object");
        for key in object.keys() {
            assert!(
                matches!(
                    key.as_str(),
                    "id" | "title"
                        | "url"
                        | "description"
                        | "content"
                        | "tags"
                        | "collection"
                        | "date"
                ),
                "unexpected field {key:?} in {doc}"
            );
        }
        for required in ["id", "title", "url", "content", "tags", "collection"] {
            assert!(object.contains_key(required), "missing {required}: {doc}");
        }
        for optional in ["description", "date"] {
            assert!(
                object.get(optional).is_none_or(|v| !v.is_null()),
                "optional {optional} must be omitted, not null: {doc}"
            );
        }
    }

    let alpha = docs
        .iter()
        .find(|d| d["id"] == "/posts/alpha/")
        .expect("alpha");
    assert_eq!(alpha["title"], "Alpha");
    assert_eq!(alpha["url"], "/posts/alpha/");
    assert_eq!(alpha["description"], "The alpha summary.");
    assert_eq!(alpha["collection"], "posts");
    assert_eq!(alpha["date"], "2026-02-01");
    // Tags are the sorted canonical identity, not authored order.
    assert_eq!(alpha["tags"], serde_json::json!(["Rust", "Systems"]));

    // Schema-evolution guard: alpha has every optional field, so its
    // serialized shape is the complete contract. Adding, removing, or
    // renaming any field fails here — a schema change cannot land silently
    // under version 1 (ADR 0034: schema changes need an explicit decision).
    let alpha_keys: std::collections::BTreeSet<&str> = alpha
        .as_object()
        .expect("alpha object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        alpha_keys,
        [
            "id",
            "title",
            "url",
            "description",
            "content",
            "tags",
            "collection",
            "date"
        ]
        .into_iter()
        .collect()
    );

    // Beta has no description; gamma has no date: both omitted, never
    // fabricated.
    let beta = docs
        .iter()
        .find(|d| d["id"] == "/posts/beta/")
        .expect("beta");
    assert!(beta.get("description").is_none(), "beta: {beta}");
    assert_eq!(beta["date"], "2026-01-01");
    let gamma = docs
        .iter()
        .find(|d| d["id"] == "/notes/gamma/")
        .expect("gamma");
    assert!(gamma.get("date").is_none(), "gamma: {gamma}");
    assert!(gamma.get("description").is_none(), "gamma: {gamma}");
}

#[test]
fn content_normalization_is_structural_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let json = std::fs::read_to_string(out.join("index.json")).expect("index");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let content = parsed["documents"]
        .as_array()
        .expect("documents")
        .iter()
        .find(|d| d["id"] == "/posts/probe/")
        .and_then(|d| d["content"].as_str())
        .expect("probe content");

    // Structural normalization keeps prose, headings, inline code, link
    // labels, image alt text, alert bodies, table cells, math literals,
    // list items, and hard-break-separated lines…
    for kept in [
        "Prose with bold and emphasis",
        "Heading Text",
        "A link label",
        "image alt",
        "Alert body words",
        "Col A Col B cell one cell two",
        "Math inline $x^2$ here",
        "Line with hard break at end. Second line after break",
        "list item one",
        "Café naïve 日本語 emoji 🎉 apostrophe's hyphen-word",
    ] {
        assert!(content.contains(kept), "missing {kept:?}: {content}");
    }
    // …collapses repeated whitespace…
    assert!(
        content.contains("Paragraph with repeated spaces"),
        "whitespace not collapsed: {content}"
    );
    // …and excludes Markdown syntax, fenced/indented/Mermaid code, raw
    // HTML, alert markers, table separators, and every URL.
    for excluded in [
        "**",
        "```",
        "fn fenced_code",
        "flowchart",
        "indented code line",
        "[!NOTE]",
        "| --- |",
        "<div>",
        "raw html words",
        "/posts/beta/",
        "/images/hero.png",
    ] {
        assert!(
            !content.contains(excluded),
            "leaked {excluded:?}: {content}"
        );
    }
    // No lexical normalization: case, diacritics, non-Latin scripts,
    // emoji, apostrophes, and hyphens all survive verbatim.
    assert!(
        content.contains("Café naïve 日本語 emoji 🎉 apostrophe's hyphen-word"),
        "case/diacritics/emoji/apostrophe/hyphen lost: {content}"
    );
}

#[test]
fn search_index_is_deterministic_across_runs_and_output_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let first = dir.path().join("out-first");
    let second = dir.path().join("out-second");
    build_site_from_disk(dir.path(), &first).expect("first build");
    build_site_from_disk(dir.path(), &second).expect("second build");
    let a = std::fs::read(first.join("index.json")).expect("index a");
    let b = std::fs::read(second.join("index.json")).expect("index b");
    assert_eq!(
        a, b,
        "independent output dirs must produce identical indexes"
    );
    // Rebuilding in place reproduces the same bytes too.
    build_site_from_disk(dir.path(), &first).expect("rebuild");
    assert_eq!(std::fs::read(first.join("index.json")).expect("index"), a);
}

// ---------------------------------------------------------------------------
// Dependency guards
// ---------------------------------------------------------------------------

#[test]
fn searchable_changes_rebuild_the_index() {
    // Searchable fields (the exact projection) — each must rebuild.
    assert_rebuilds(|root| {
        replace(
            root,
            "content/posts/alpha.md",
            "title: Alpha",
            "title: Alpha Two",
        )
    });
    assert_rebuilds(|root| {
        replace(
            root,
            "content/posts/alpha.md",
            "The alpha summary.",
            "A different summary.",
        )
    });
    assert_rebuilds(|root| {
        append(
            root,
            "content/posts/alpha.md",
            "\n\nA brand new sentence.\n",
        )
    });
    assert_rebuilds(|root| {
        replace(
            root,
            "content/posts/beta.md",
            "topics: [\"Rust\"]",
            "topics: [\"Rust\", \"Extra\"]",
        )
    });
    assert_rebuilds(|root| {
        replace(
            root,
            "content/posts/beta.md",
            "date: 2026-01-01",
            "date: 2026-01-02",
        )
    });
    // Route change (nothing links to gamma).
    assert_rebuilds(|root| {
        std::fs::rename(
            root.join("content/notes/gamma.md"),
            root.join("content/notes/gamma-renamed.md"),
        )
        .expect("rename")
    });
    // Collection change also changes the route and the `collection` field.
    assert_rebuilds(|root| {
        std::fs::rename(
            root.join("content/notes/gamma.md"),
            root.join("content/posts/gamma.md"),
        )
        .expect("move")
    });
}

#[test]
fn unsearchable_changes_reuse_the_index() {
    // Presentation, configuration, and non-projected model fields never
    // reach `search_documents`. Each change below alters other artifacts
    // (or nothing) but must leave `index.json` reused — measured through
    // the real build plan, never by inspecting planner branches.

    // Rendering and static inputs.
    assert_reuses(|root| append(root, "templates/post.html", "\n<!-- touched -->\n"));
    assert_reuses(|root| append(root, "static/extra.css", "\n/* touched */\n"));
    assert_reuses(|root| {
        write_bytes(
            root,
            "static/images/unrelated.png",
            &gradient_png(64, 64, 7),
        )
    });
    // The hero's bytes are not part of any document.
    assert_reuses(|root| write_bytes(root, "static/images/hero.png", &gradient_png(800, 600, 9)));

    // Configuration: search is deliberately not config-driven. `base_url`,
    // date formatting, menus, minification, feeds, taxonomy, images, social
    // cards, and the site author all change other artifacts only.
    assert_reuses(|root| {
        replace(
            root,
            "signal.toml",
            "base_url = \"https://example.com/\"",
            "base_url = \"https://other.example/\"",
        )
    });
    assert_reuses(|root| {
        replace(
            root,
            "signal.toml",
            "date_format = \"%Y-%m-%d\"",
            "date_format = \"%-d %B %Y\"",
        )
    });
    assert_reuses(|root| {
        replace(
            root,
            "signal.toml",
            "label = \"Posts\"",
            "label = \"Articles\"",
        )
    });
    assert_reuses(|root| append(root, "signal.toml", "\n[output]\nminify_html = true\n"));
    assert_reuses(|root| replace(root, "signal.toml", "limit = 2", "limit = 1"));
    assert_reuses(|root| {
        replace(
            root,
            "signal.toml",
            "title = \"Topics\"",
            "title = \"Labels\"",
        )
    });
    assert_reuses(|root| replace(root, "signal.toml", "widths = [640]", "widths = [320, 640]"));
    assert_reuses(|root| replace(root, "signal.toml", "[social]", "[social]\nwidth = 1000"));
    assert_reuses(|root| {
        replace(
            root,
            "signal.toml",
            "author = \"Site Author\"",
            "author = \"Other\"",
        )
    });

    // Entry fields outside the projection.
    assert_reuses(|root| add_front_matter(root, "content/posts/beta.md", "lastmod: 2026-03-03"));
    assert_reuses(|root| add_front_matter(root, "content/posts/beta.md", "featured: true"));
    assert_reuses(|root| add_front_matter(root, "content/posts/beta.md", "image_alt: ignored"));

    // Unpublished and non-entry content.
    assert_reuses(|root| append(root, "content/posts/draft.md", "\n\nMore draft words.\n"));
    assert_reuses(|root| {
        write_site(
            root,
            &[(
                "content/posts/new-draft.md",
                "---\ntitle: New Draft\ndraft: true\n---\n\nDraft words.\n",
            )],
        )
    });
    assert_reuses(|root| append(root, "content/notes/_index.md", "\n\nMore section words.\n"));

    // Code is excluded from `plain_text`, so editing fenced, Mermaid, or
    // indented code changes the rendered page but not the projection.
    assert_reuses(|root| {
        replace(
            root,
            "content/posts/alpha.md",
            "fn hidden() {}",
            "fn hidden(name: &str) {}",
        )
    });
    assert_reuses(|root| {
        replace(
            root,
            "content/posts/probe.md",
            "flowchart LR",
            "flowchart TD",
        )
    });
    assert_reuses(|root| {
        replace(
            root,
            "content/posts/probe.md",
            "    indented code line",
            "    indented code changed",
        )
    });
}

// ---------------------------------------------------------------------------
// Explain
// ---------------------------------------------------------------------------

#[test]
fn explain_search_index_reports_contract_and_reuse() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");

    let text = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(
        text.contains("Artifact\n========\n  path: index.json\n"),
        "got:\n{text}"
    );
    assert!(text.contains("\nKind:\n  SearchIndex\n"), "got:\n{text}");
    assert!(
        text.contains("\nInputs:\n  Query(search_documents)\n"),
        "got:\n{text}"
    );
    assert!(text.contains("\nDocuments:\n  4\n"), "got:\n{text}");
    assert!(text.contains("\nDecision:\n  reuse\n"), "got:\n{text}");
    // Deterministic: explaining twice is byte-identical.
    let again = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert_eq!(text, again);
}

#[test]
fn explain_search_index_reports_the_rebuild_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    replace(
        dir.path(),
        "content/posts/alpha.md",
        "title: Alpha",
        "title: Alpha Two",
    );

    let text = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(
        text.contains("\nDecision:\n  rebuild: query changed: search_documents\n"),
        "got:\n{text}"
    );
    // A template edit rebuilds pages but not the search artifact.
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    append(dir.path(), "templates/post.html", "\n<!-- touched -->\n");
    let text = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(text.contains("\nDecision:\n  reuse\n"), "got:\n{text}");
}

#[test]
fn explain_other_planned_artifacts_by_output_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");

    // The generalization is deliberate: any planned artifact explains by
    // output path, not just the search index.
    let page = explain_asset_from_disk(dir.path(), &out, "posts/alpha/index.html").expect("page");
    assert!(page.contains("\nKind:\n  Page\n"), "got:\n{page}");
    assert!(
        page.contains("\nInputs:\n  Entry(/posts/alpha/)\n"),
        "got:\n{page}"
    );
    assert!(
        !page.contains("Documents:"),
        "pages have no document count:\n{page}"
    );

    // Unplanned output paths stay unknown-asset diagnostics.
    let err = explain_asset_from_disk(dir.path(), &out, "nope.json").expect_err("unknown");
    assert!(err.to_string().contains("unknown asset"), "got: {err}");
}

// ---------------------------------------------------------------------------
// build / check / explain agreement
// ---------------------------------------------------------------------------

#[test]
fn build_check_and_explain_agree_on_the_search_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");

    // build plans and writes it.
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    let planned = summary
        .specs
        .iter()
        .find(|spec| spec.path == "index.json")
        .expect("build plans index.json");
    assert_eq!(planned.kind, ArtifactKind::SearchIndex);
    assert!(out.join("index.json").exists());

    // check validates the same planned spec set.
    let report = check_site_from_disk(dir.path()).expect("checks");
    assert_eq!(report.title, "Search");
    let checked = signal_cli::pipeline::load_validated_check(dir.path())
        .expect("checks")
        .validated
        .specs;
    let checked_spec = checked
        .iter()
        .find(|spec| spec.path == "index.json")
        .expect("check plans index.json");
    assert_eq!(checked_spec.kind, ArtifactKind::SearchIndex);

    // explain reports the same kind, the same declared input, and the same
    // decision the plan recorded (a fresh build rebuilt it; a second build
    // reuses it).
    let first = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(first.contains("\nKind:\n  SearchIndex\n"), "got:\n{first}");
    assert!(
        first.contains("\nInputs:\n  Query(search_documents)\n"),
        "got:\n{first}"
    );
    assert!(first.contains("\nDecision:\n  reuse\n"), "got:\n{first}");

    let second = build_site_from_disk(dir.path(), &out).expect("rebuild");
    assert_eq!(second.rebuilt, 0, "a clean rebuild must reuse everything");
    let after = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(after.contains("\nDecision:\n  reuse\n"), "got:\n{after}");

    // A searchable edit: explain reports the rebuild before the plan runs,
    // and the build then rebuilds exactly that artifact.
    replace(
        dir.path(),
        "content/posts/beta.md",
        "title: Beta",
        "title: Beta Two",
    );
    let rebuilt = explain_asset_from_disk(dir.path(), &out, "index.json").expect("explains");
    assert!(
        rebuilt.contains("\nDecision:\n  rebuild: query changed: search_documents\n"),
        "got:\n{rebuilt}"
    );
    let edited = build_site_from_disk(dir.path(), &out).expect("edited build");
    assert!(
        edited.rebuilt_paths.iter().any(|path| path == "index.json"),
        "rebuilt: {:?}",
        edited.rebuilt_paths
    );
}

// ---------------------------------------------------------------------------
// Build graph isolation
// ---------------------------------------------------------------------------

#[test]
fn search_artifact_declares_exactly_the_search_query_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");

    // The recorded build graph is the source of truth: the search artifact
    // consumes the search projection and nothing else. Adding `Config`,
    // `TemplateSet`, `Static`, or `Entry` to it would fail this assertion.
    let manifest: signal_cli::manifest::Manifest = serde_json::from_str(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("valid manifest");
    let record = manifest
        .artifacts
        .get("index.json")
        .expect("index.json is recorded");
    assert_eq!(record.kind, ArtifactKind::SearchIndex);
    assert_eq!(
        record.inputs,
        vec![signal_cli::manifest::InputRef::Query {
            key: "search_documents".to_string(),
        }]
    );
}

#[test]
fn search_urls_are_relative_route_identifiers() {
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("index.json")).expect("index"))
            .expect("valid JSON");
    for doc in parsed["documents"].as_array().expect("documents") {
        let id = doc["id"].as_str().expect("id");
        let url = doc["url"].as_str().expect("url");
        // `id` and `url` are the same stable route identifier.
        assert_eq!(id, url, "id and url must agree: {doc}");
        // Root-relative: the contract never embeds `site.base_url`.
        assert!(url.starts_with('/'), "url must be root-relative: {url}");
        assert!(
            !url.contains("example.com"),
            "url must not embed base_url: {url}"
        );
    }
}

#[test]
fn cli_explain_index_json_is_deterministic() {
    use std::process::Command;
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_signal"));
    let dir = tempfile::tempdir().expect("tempdir");
    search_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let run = || {
        let output = Command::new(&bin)
            .arg("explain")
            .arg("--root")
            .arg(dir.path())
            .arg("--out")
            .arg(&out)
            .arg("index.json")
            .output()
            .expect("run signal explain");
        assert!(output.status.success());
        String::from_utf8(output.stdout).expect("utf8")
    };
    let first = run();
    assert!(first.contains("\nKind:\n  SearchIndex\n"), "got:\n{first}");
    assert_eq!(first, run(), "explain must be byte-identical across runs");
}
