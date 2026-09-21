//! First-class assets (A1): cross-crate integration tests.
//!
//! These tests exercise behaviour across `signal-core` (reference
//! resolution, MIME), `signal-cli` discovery/planning (`Static` specs,
//! `artifact_inputs` asset edges, manifest asset digests), reference
//! validation, and the `check` / `explain` CLIs.

use signal_cli::{
    build::build_site_from_disk, explain::explain_asset_from_disk,
    link_check::check_site_from_disk, manifest::InputRef,
};
use std::path::{Path, PathBuf};

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

fn site_with_hero(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Assets\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\nimage: images/hero.jpg\n---\n\nBody with ![alt](/images/hero.jpg).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    write_bytes(dir, "static/images/hero.jpg", b"fake-jpeg-bytes");
}

fn out_dir(dir: &Path) -> PathBuf {
    dir.join("out")
}

#[test]
fn referenced_asset_is_planned_and_copied_verbatim() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    // The asset is a first-class planned artifact, resolved through the
    // same pipeline — not an ad-hoc copy.
    assert!(summary
        .specs
        .iter()
        .any(|s| s.path == "images/hero.jpg" && s.kind == signal_core::ArtifactKind::Static));
    let bytes = std::fs::read(out.join("images/hero.jpg")).expect("asset output");
    assert_eq!(bytes, b"fake-jpeg-bytes");
}

#[test]
fn page_plan_names_its_assets_as_static_inputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let manifest = signal_cli::manifest::parse_manifest(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("parses");
    let record = manifest
        .artifacts
        .get("posts/example/index.html")
        .expect("page record");
    assert!(
        record.inputs.contains(&InputRef::Static {
            path: "images/hero.jpg".to_string()
        }),
        "page must name its hero asset: {:?}",
        record.inputs
    );
    // The manifest records the asset digest once for every consumer.
    assert!(manifest.assets.contains_key("images/hero.jpg"));
}

#[test]
fn asset_bytes_change_rebuilds_asset_and_embedding_page_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    // A second entry without asset references stays reusable.
    write_site(
        dir.path(),
        &[(
            "content/posts/plain.md",
            "---\ntitle: Plain\n---\n\nNo images here.\n",
        )],
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");
    write_bytes(dir.path(), "static/images/hero.jpg", b"changed-bytes");

    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(
        text.contains("  images/hero.jpg\n    reason: static file changed: images/hero.jpg\n"),
        "got:\n{text}"
    );
    assert!(
        text.contains(
            "  posts/example/index.html\n    reason: static file changed: images/hero.jpg\n"
        ),
        "got:\n{text}"
    );
    // The unreferencing page still reuses.
    let reuse = text.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  posts/plain/index.html\n"), "got:\n{text}");
}

#[test]
fn output_is_deterministic_across_independent_builds() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(dir.path(), first.path()).expect("first builds");
    build_site_from_disk(dir.path(), second.path()).expect("second builds");
    let a = std::fs::read(first.path().join("images/hero.jpg")).expect("asset a");
    let b = std::fs::read(second.path().join("images/hero.jpg")).expect("asset b");
    assert_eq!(a, b);
    let ma = std::fs::read(first.path().join(".signal/manifest.json")).expect("manifest a");
    let mb = std::fs::read(second.path().join(".signal/manifest.json")).expect("manifest b");
    assert_eq!(ma, mb);
}

#[test]
fn missing_asset_fails_build_check_and_explain_identically() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Assets\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\n---\n\n![alt](/images/gone.jpg).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    let out = out_dir(dir.path());
    for result in [
        build_site_from_disk(dir.path(), &out).map(|_| ()),
        check_site_from_disk(dir.path()).map(|_| ()),
        signal_cli::explain::explain_site_from_disk(dir.path(), &out).map(|_| ()),
    ] {
        let err = result.expect_err("missing asset must fail");
        assert!(
            err.to_string().contains("gone.jpg"),
            "diagnostic must name the target: {err}"
        );
    }
    // Failed validation writes nothing.
    assert!(!out.join("images/gone.jpg").exists());
}

#[test]
fn escaping_asset_reference_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Assets\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\n---\n\n![alt](../../../etc/passwd).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    let out = out_dir(dir.path());
    let err = build_site_from_disk(dir.path(), &out).expect_err("must fail");
    assert!(
        err.to_string().contains("invalid internal reference"),
        "got: {err}"
    );
}

#[test]
fn asset_output_collision_with_generated_page_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    // A static file shadowing the section page output.
    write_bytes(dir.path(), "static/posts/index.html", b"shadow");
    let out = out_dir(dir.path());
    let err = build_site_from_disk(dir.path(), &out).expect_err("must collide");
    assert!(err.to_string().contains("collision"), "got: {err}");
}

#[test]
fn check_reports_asset_inventory() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    write_bytes(dir.path(), "static/js/unused.js", b"console.log(1);");
    let report = check_site_from_disk(dir.path()).expect("check passes");
    assert_eq!(report.assets.discovered, 2);
    assert_eq!(report.assets.referenced, 1);
    assert_eq!(report.assets.resolved, 1);
    assert_eq!(report.assets.missing, 0);
    assert_eq!(report.assets.unsafe_paths, 0);
    assert_eq!(report.assets.unreferenced, 1);
}

#[test]
fn explain_asset_reports_source_type_size_dependency_output_and_decision() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    // All accepted target spellings resolve to the same record.
    for target in [
        "images/hero.jpg",
        "/images/hero.jpg",
        "static/images/hero.jpg",
    ] {
        let text = explain_asset_from_disk(dir.path(), &out, target).expect("explains asset");
        assert!(
            text.contains("Source:\n  static/images/hero.jpg\n"),
            "got:\n{text}"
        );
        assert!(text.contains("Type:\n  image/jpeg\n"), "got:\n{text}");
        assert!(
            text.contains(&format!("Size:\n  {} bytes\n", b"fake-jpeg-bytes".len())),
            "got:\n{text}"
        );
        assert!(
            text.contains("Dependency:\n  /posts/example/\n"),
            "got:\n{text}"
        );
        assert!(
            text.contains("Output:\n  images/hero.jpg\n"),
            "got:\n{text}"
        );
        assert!(text.contains("Action:\n  copy\n"), "got:\n{text}");
        assert!(text.contains("Decision:\n  reuse\n"), "got:\n{text}");
    }
    // Deterministic: explaining twice is byte-identical.
    let a = explain_asset_from_disk(dir.path(), &out, "images/hero.jpg").expect("explains");
    let b = explain_asset_from_disk(dir.path(), &out, "images/hero.jpg").expect("explains");
    assert_eq!(a, b);
}

#[test]
fn explain_unknown_asset_is_a_model_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let err = explain_asset_from_disk(dir.path(), &out, "images/nope.jpg").expect_err("must fail");
    assert!(err.to_string().contains("unknown asset"), "got: {err}");
    let err = explain_asset_from_disk(dir.path(), &out, "../evil.jpg").expect_err("must fail");
    assert!(
        err.to_string().contains("invalid asset target"),
        "got: {err}"
    );
}

#[test]
fn explain_is_read_only_for_assets() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let before_manifest =
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest exists");
    let before_asset = std::fs::read(out.join("images/hero.jpg")).expect("asset exists");
    explain_asset_from_disk(dir.path(), &out, "images/hero.jpg").expect("explains");
    assert_eq!(
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
        before_manifest
    );
    assert_eq!(
        std::fs::read(out.join("images/hero.jpg")).expect("asset"),
        before_asset
    );
}
