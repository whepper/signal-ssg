//! End-to-end golden-tree tests for the vertical slices.
//!
//! Builds each fixture into a temp dir, compares the output tree
//! byte-for-byte against its `expected/` directory, then builds a second
//! time and verifies identical output (determinism).

use signal_cli::build_site_from_disk;
use std::path::{Path, PathBuf};

/// Collect `(relative_path, bytes)` for a directory tree in sorted order.
pub fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    collect(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(Result::ok).collect())
        .unwrap_or_default();
    let mut paths: Vec<PathBuf> = entries.iter().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            // `.signal/` is internal build state, not public output: goldens
            // compare the servable tree only. Manifests are verified
            // separately against their own frozen expectations.
            if path.file_name().is_some_and(|name| name == ".signal") {
                continue;
            }
            collect(root, &path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push((rel, bytes));
        }
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/minimal-site"
    ))
}

#[test]
fn minimal_fixture_has_required_structure() {
    let root = fixture_root();
    assert!(root.join("signal.toml").is_file(), "signal.toml must exist");
    assert!(
        root.join("content/posts/hello-world.md").is_file(),
        "fixture post must exist"
    );
    assert!(
        root.join("templates/post.html").is_file(),
        "fixture template must exist"
    );
    assert!(
        root.join("expected/posts/hello-world/index.html").is_file(),
        "golden expected output must exist"
    );
}

#[test]
fn build_matches_golden_tree() {
    let root = fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    let summary = build_site_from_disk(&root, dir.path()).expect("build succeeds");
    // One entry page plus one section page for the posts collection, plus
    // the sitemap (base_url is configured; no [feed] means no feeds) and
    // the always-on search index.
    assert_eq!(summary.pages_written, 4);

    let actual = snapshot_tree(dir.path());
    let expected = snapshot_tree(&root.join("expected"));
    assert_eq!(
        actual.len(),
        expected.len(),
        "output file count differs: {actual:?}"
    );
    for ((actual_path, actual_bytes), (expected_path, expected_bytes)) in
        actual.iter().zip(expected.iter())
    {
        assert_eq!(actual_path, expected_path, "output path differs");
        assert_eq!(
            String::from_utf8_lossy(actual_bytes),
            String::from_utf8_lossy(expected_bytes),
            "content differs for {actual_path:?}"
        );
    }
}

#[test]
fn build_is_deterministic_across_runs() {
    let root = fixture_root();
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, first.path()).expect("first build");
    build_site_from_disk(&root, second.path()).expect("second build");
    assert_eq!(snapshot_tree(first.path()), snapshot_tree(second.path()));
    assert_eq!(
        std::fs::read(first.path().join(".signal/manifest.json")).expect("manifest"),
        std::fs::read(second.path().join(".signal/manifest.json")).expect("manifest"),
        "manifest bytes must be deterministic too"
    );
}

fn sample_fixture_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/sample-site"
    ))
}

fn assert_tree_matches(root: &Path, out: &Path) {
    let actual = snapshot_tree(out);
    let expected = snapshot_tree(&root.join("expected"));
    assert_eq!(
        actual.len(),
        expected.len(),
        "output file count differs: {actual:?}"
    );
    for ((actual_path, actual_bytes), (expected_path, expected_bytes)) in
        actual.iter().zip(expected.iter())
    {
        assert_eq!(actual_path, expected_path, "output path differs");
        assert_eq!(
            actual_bytes, expected_bytes,
            "content differs for {actual_path:?}"
        );
    }
}

#[test]
fn sample_fixture_builds_all_slice_pages() {
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    let summary = build_site_from_disk(&root, dir.path()).expect("build succeeds");
    // 3 entry pages (section roots render as sections, the draft is skipped)
    // + 3 section pages + the home page + 1 topics index + 2 term pages
    // + 1 main feed + 3 section feeds + 1 label feed + 2 term feeds
    // + 1 sitemap + 1 search index + 1 robots.txt + 1 themed 404
    // + 2 static assets (hero SVG + mermaid stub).
    // The draft-only topic never produces a page.
    assert_eq!(summary.pages_written, 23);
    assert_eq!(summary.drafts_skipped, 1);
    for rel in [
        "index.html",
        "index.xml",
        "index.json",
        "sitemap.xml",
        "robots.txt",
        "404.html",
        "posts/index.html",
        "posts/index.xml",
        "posts/alpha/index.html",
        "posts/beta/index.html",
        "projects/index.html",
        "projects/index.xml",
        "projects/gadget/index.html",
        "notes/index.html",
        "notes/index.xml",
        "topics/index.html",
        "topics/index.xml",
        "topics/rust/index.html",
        "topics/rust/index.xml",
        "topics/systems/index.html",
        "topics/systems/index.xml",
    ] {
        assert!(
            dir.path().join(rel).is_file(),
            "expected output page {rel} missing"
        );
    }
    assert!(
        !dir.path().join("topics/unseen-topic/index.html").exists(),
        "draft-only topics must not produce term pages"
    );
    assert_tree_matches(&root, dir.path());
}

/// Read all `<loc>` values from a sitemap with quick-xml: fails on
/// malformed XML, so this validates as well as asserts.
fn sitemap_locs(xml: &str) -> Vec<String> {
    let mut reader = quick_xml::reader::Reader::from_str(xml);
    let mut locs = Vec::new();
    let mut pending: Option<String> = None;
    loop {
        match reader.read_event().expect("well-formed sitemap") {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) if e.name().into_inner() == "loc" => {
                pending = Some(String::new());
            }
            quick_xml::events::Event::Text(e) if pending.is_some() => {
                pending
                    .as_mut()
                    .expect("pending")
                    .push_str(&e.xml10_content());
            }
            quick_xml::events::Event::End(e) if e.name().into_inner() == "loc" => {
                locs.push(pending.take().expect("loc text"));
            }
            _ => {}
        }
    }
    locs
}

/// Collect `(link, pubDate?)` items from an RSS feed; fails on malformed XML.
fn feed_items(xml: &str) -> Vec<(String, Option<String>)> {
    let mut reader = quick_xml::reader::Reader::from_str(xml);
    let mut items = Vec::new();
    let mut current: Option<(Option<String>, Option<String>)> = None;
    let mut field: Option<&str> = None;
    loop {
        match reader.read_event().expect("well-formed feed") {
            quick_xml::events::Event::Eof => break,
            quick_xml::events::Event::Start(e) if e.name().into_inner() == "item" => {
                current = Some((None, None));
            }
            quick_xml::events::Event::Start(e)
                if current.is_some()
                    && (e.name().into_inner() == "link" || e.name().into_inner() == "pubDate") =>
            {
                field = Some(if e.name().into_inner() == "link" {
                    "link"
                } else {
                    "pubDate"
                });
            }
            quick_xml::events::Event::Text(e) => {
                if let (Some((link, date)), Some(kind)) = (current.as_mut(), field) {
                    let text = e.xml10_content().into_owned();
                    if kind == "link" {
                        *link = Some(text);
                    } else {
                        *date = Some(text);
                    }
                }
            }
            quick_xml::events::Event::End(e) if e.name().into_inner() == "item" => {
                let (link, date) = current.take().expect("item");
                items.push((link.expect("item link"), date));
                field = None;
            }
            quick_xml::events::Event::End(_) => {
                field = None;
            }
            _ => {}
        }
    }
    items
}

#[test]
fn sample_feeds_select_order_and_date_items() {
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, dir.path()).expect("build succeeds");

    // Main feed: all 3 regular entries would qualify, but `[feed] limit = 2`
    // truncates to the newest two.
    let main = std::fs::read_to_string(dir.path().join("index.xml")).expect("main feed");
    let items = feed_items(&main);
    assert_eq!(items.len(), 2, "limit truncates: {items:?}");
    assert_eq!(items[0].0, "https://example.com/projects/gadget/");
    assert_eq!(items[1].0, "https://example.com/posts/alpha/");
    assert_eq!(
        items[0].1.as_deref(),
        Some("Sun, 01 Mar 2026 00:00:00 +0000")
    );
    // Newest-first: every pubDate descends.
    let dates: Vec<&str> = items.iter().filter_map(|(_, d)| d.as_deref()).collect();
    let mut sorted = dates.clone();
    sorted.sort();
    sorted.reverse();
    assert_eq!(dates, sorted);

    // Term feed: tag members only, newest-first; beta has no description so
    // its item carries a body excerpt instead.
    let rust = std::fs::read_to_string(dir.path().join("topics/rust/index.xml")).expect("term");
    let items = feed_items(&rust);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, "https://example.com/posts/alpha/");
    assert_eq!(items[1].0, "https://example.com/posts/beta/");
    assert!(
        rust.contains("Synthetic body for the beta post"),
        "excerpt: {rust}"
    );
    assert!(
        !rust.contains("Unpublished Draft"),
        "drafts excluded: {rust}"
    );
}

#[test]
fn sample_sitemap_lists_public_urls_only() {
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, dir.path()).expect("build succeeds");

    let xml = std::fs::read_to_string(dir.path().join("sitemap.xml")).expect("sitemap");
    let locs = sitemap_locs(&xml);
    // 10 public URLs: home, 3 sections, 3 entries, taxonomy index, 2 terms.
    // No feeds, no static assets, no drafts.
    assert_eq!(
        locs,
        vec![
            "https://example.com/",
            "https://example.com/notes/",
            "https://example.com/posts/",
            "https://example.com/posts/alpha/",
            "https://example.com/posts/beta/",
            "https://example.com/projects/",
            "https://example.com/projects/gadget/",
            "https://example.com/topics/",
            "https://example.com/topics/rust/",
            "https://example.com/topics/systems/",
        ]
    );
    assert!(
        xml.contains("<lastmod>2026-02-15</lastmod>"),
        "alpha lastmod: {xml}"
    );
    assert!(!xml.contains("index.xml"), "feeds excluded: {xml}");
    assert!(!xml.contains("alpha.svg"), "assets excluded: {xml}");
}

#[test]
fn sample_search_index_is_versioned_and_selective() {
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, dir.path()).expect("build succeeds");

    let json = std::fs::read_to_string(dir.path().join("index.json")).expect("index");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["version"], serde_json::Value::from(1));
    let docs = parsed["documents"].as_array().expect("documents array");
    // 3 regular entries in route order; section roots and the draft are out.
    let ids: Vec<&str> = docs.iter().map(|d| d["id"].as_str().expect("id")).collect();
    assert_eq!(
        ids,
        vec!["/posts/alpha/", "/posts/beta/", "/projects/gadget/"]
    );

    let alpha = &docs[0];
    assert_eq!(alpha["title"], serde_json::Value::from("Alpha Post"));
    assert_eq!(alpha["url"], serde_json::Value::from("/posts/alpha/"));
    assert_eq!(alpha["collection"], serde_json::Value::from("posts"));
    assert_eq!(alpha["date"], serde_json::Value::from("2026-02-01"));
    let content = alpha["content"].as_str().expect("content");
    // Prose, headings, alert bodies, and Unicode flow in...
    for expected in [
        "Getting Started",
        "Do not run untrusted snippets",
        "Über den Tellerrand",
    ] {
        assert!(
            content.contains(expected),
            "missing {expected:?}: {content}"
        );
    }
    // ...while fenced code, Mermaid, markers, and markup stay out.
    for excluded in ["fn greet", "flowchart", "[!WARNING]", "<div", "code-block"] {
        assert!(
            !content.contains(excluded),
            "leaked {excluded:?}: {content}"
        );
    }

    // Beta has no description: omitted, not fabricated.
    let beta = &docs[1];
    assert!(beta.get("description").is_none(), "beta: {beta}");
}

#[test]
fn sample_build_is_deterministic_across_runs() {
    let root = sample_fixture_root();
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, first.path()).expect("first build");
    build_site_from_disk(&root, second.path()).expect("second build");
    assert_eq!(snapshot_tree(first.path()), snapshot_tree(second.path()));
    // The manifest is internal build state: excluded from the tree above,
    // so its bytes are compared explicitly — byte-identical, not merely
    // equivalent.
    let first_manifest =
        std::fs::read(first.path().join(".signal/manifest.json")).expect("manifest");
    let second_manifest =
        std::fs::read(second.path().join(".signal/manifest.json")).expect("manifest");
    assert_eq!(first_manifest, second_manifest);
}

#[test]
fn sample_manifest_matches_frozen_expectation() {
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(&root, dir.path()).expect("build succeeds");
    let actual = std::fs::read_to_string(dir.path().join(".signal/manifest.json"))
        .expect("manifest written");
    let expected =
        std::fs::read_to_string(root.join("expected-manifest.json")).expect("frozen manifest");
    assert_eq!(actual, expected, "manifest differs from frozen expectation");
}

#[test]
fn manifest_covers_plan_and_output_exactly() {
    use signal_cli::manifest::parse_manifest;
    let root = sample_fixture_root();
    let dir = tempfile::tempdir().expect("tempdir");
    let summary = build_site_from_disk(&root, dir.path()).expect("build succeeds");

    let manifest = parse_manifest(
        &std::fs::read_to_string(dir.path().join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("manifest parses");
    assert_eq!(manifest.schema_version, 1);

    // Planned paths == manifest paths...
    let mut planned: Vec<&str> = summary.specs.iter().map(|s| s.path.as_str()).collect();
    planned.sort();
    let mut recorded: Vec<&str> = manifest.artifacts.keys().map(String::as_str).collect();
    recorded.sort();
    assert_eq!(
        planned, recorded,
        "every planned artifact has exactly one record"
    );

    // ...== actual public output paths (`.signal/` excluded: build state).
    let mut actual: Vec<String> = snapshot_tree(dir.path())
        .into_iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect();
    actual.sort();
    let mut planned_owned: Vec<String> = planned.into_iter().map(str::to_string).collect();
    planned_owned.sort();
    assert_eq!(actual, planned_owned, "every output has a planned spec");

    // No duplicates, kinds preserved, output digests match actual bytes.
    for (path, record) in &manifest.artifacts {
        let bytes = std::fs::read(dir.path().join(path)).expect("output exists");
        assert_eq!(
            signal_cli::manifest::digest_bytes(&bytes).to_string(),
            record.output_digest.to_string(),
            "output digest mismatch for {path}"
        );
        let spec = summary
            .specs
            .iter()
            .find(|s| &s.path == path)
            .expect("spec exists");
        assert_eq!(&spec.kind, &record.kind, "kind mismatch for {path}");
    }

    // Input references resolve to recorded digests.
    for (path, record) in &manifest.artifacts {
        for input in &record.inputs {
            match input {
                signal_cli::manifest::InputRef::Entry { route } => {
                    assert!(
                        manifest.entries.contains_key(route),
                        "{path} references unrecorded entry {route}"
                    );
                }
                signal_cli::manifest::InputRef::Query { key } => {
                    assert!(
                        manifest.queries.contains_key(key),
                        "{path} references unrecorded query {key}"
                    );
                }
                signal_cli::manifest::InputRef::Template { name } => {
                    assert!(
                        manifest.templates.contains_key(name),
                        "{path} references unrecorded template {name}"
                    );
                }
                signal_cli::manifest::InputRef::Config => {}
                signal_cli::manifest::InputRef::TemplateSet => {
                    assert!(
                        !manifest.templates.is_empty(),
                        "{path} depends on the template set but none were recorded"
                    );
                }
                signal_cli::manifest::InputRef::Static { path: source } => {
                    assert_eq!(source, path, "static input must name its source file");
                }
            }
        }
    }
}

#[test]
fn failed_build_leaves_no_partial_manifest() {
    let root = sample_fixture_root();
    // Work on a scratch copy so the fixture stays pristine.
    let scratch = tempfile::tempdir().expect("tempdir");
    copy_tree(&root, scratch.path());
    let out = tempfile::tempdir().expect("tempdir");
    // A valid build first, so a previous manifest exists...
    build_site_from_disk(scratch.path(), out.path()).expect("build succeeds");
    let before = std::fs::read(out.path().join(".signal/manifest.json")).expect("manifest");
    // ...then break the site and rebuild into the same output directory.
    std::fs::write(
        scratch.path().join("content/posts/bad.md"),
        "---\ntitle: Bad\nslug: \"../../evil\"\n---\n\nBad.\n",
    )
    .expect("write bad source");
    let err = build_site_from_disk(scratch.path(), out.path()).expect_err("must fail");
    assert!(format!("{err:?}").contains("slug"), "got: {err:?}");
    // The previous manifest is untouched: manifests are replaced only
    // after successful build completion, atomically.
    assert_eq!(
        before,
        std::fs::read(out.path().join(".signal/manifest.json")).expect("manifest intact")
    );
}

fn copy_tree(source: &Path, dest: &Path) {
    for entry in walk_all(source) {
        let rel = entry.strip_prefix(source).expect("relative");
        let target = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).expect("mkdir");
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::copy(&entry, &target).expect("copy");
        }
    }
}

fn walk_all(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)
            .expect("read dir")
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
                out.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
