//! Pipeline contract: `build`, `check`, and `--explain` share one
//! canonical pre-execution validation and agree on every rejection.
//!
//! Uses the library entry points directly
//! ([`signal_cli::build_site_from_disk`],
//! [`signal_cli::link_check::check_site_from_disk`],
//! [`signal_cli::explain::explain_site_from_disk`]) so these tests pin
//! semantics, not CLI plumbing.

use signal_cli::{build_site_from_disk, explain, link_check};
use std::path::{Path, PathBuf};

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

fn base_templates() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "templates/post.html",
            "<html><body>{{ content | safe }}</body></html>",
        ),
        (
            "templates/section.html",
            "<html><body>section</body></html>",
        ),
    ]
}

fn valid_site(dir: &Path) {
    let mut files = vec![
        (
            "signal.toml",
            "[site]\ntitle = \"T\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        ),
        ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
    ];
    files.extend(base_templates());
    write_site(dir, &files);
}

fn snapshot_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    if !dir.is_dir() {
        return entries;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut children: Vec<PathBuf> = std::fs::read_dir(&current)
            .expect("read_dir")
            .map(|e| e.expect("entry").path())
            .collect();
        children.sort();
        for child in children {
            if child.is_dir() {
                stack.push(child);
            } else {
                let relative = child
                    .strip_prefix(dir)
                    .expect("prefix")
                    .to_string_lossy()
                    .replace('\\', "/");
                entries.push((relative, std::fs::read(&child).expect("read")));
            }
        }
    }
    entries.sort();
    entries
}

/// A site rejected by `build` for an invalid menu is rejected by `check`
/// with the same class of error, and `--explain` agrees too.
#[test]
fn check_and_explain_agree_with_build_on_invalid_menu() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = vec![
        (
            "signal.toml",
            "[site]\ntitle = \"T\"\n[menus.main]\nitems = [\n  { label = \"\", url = \"/posts/\" },\n]\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        ),
        ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
    ];
    files.extend(base_templates());
    write_site(dir.path(), &files);
    let out = dir.path().join("out");

    let build_err =
        build_site_from_disk(dir.path(), &out).expect_err("build rejects empty menu label");
    let check_err =
        link_check::check_site_from_disk(dir.path()).expect_err("check rejects same site");
    let explain_err =
        explain::explain_site_from_disk(dir.path(), &out).expect_err("explain rejects same site");
    for err in [&build_err, &check_err, &explain_err] {
        let text = format!("{err:?}");
        assert!(
            text.contains("EmptyLabel") || text.contains("empty") || text.contains("menu"),
            "menu failure expected, got: {text}"
        );
    }
}

/// A logical output collision (static file shadowing a generated page) is
/// rejected identically by all three commands.
#[test]
fn check_and_explain_agree_with_build_on_output_collision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = vec![
        (
            "signal.toml",
            "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        ),
        ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
        // Shadows the section page `posts/index.html`.
        ("static/posts/index.html", "shadow\n"),
    ];
    files.extend(base_templates());
    write_site(dir.path(), &files);
    let out = dir.path().join("out");

    let build_err = build_site_from_disk(dir.path(), &out).expect_err("build rejects collision");
    assert!(
        format!("{build_err:?}").contains("OutputCollision"),
        "got: {build_err:?}"
    );
    let check_err =
        link_check::check_site_from_disk(dir.path()).expect_err("check rejects collision");
    assert!(
        format!("{check_err:?}").contains("OutputCollision"),
        "got: {check_err:?}"
    );
    let explain_err =
        explain::explain_site_from_disk(dir.path(), &out).expect_err("explain rejects collision");
    assert!(
        format!("{explain_err:?}").contains("OutputCollision"),
        "got: {explain_err:?}"
    );
}

/// An invalid configured route prefix fails identically everywhere.
#[test]
fn check_and_explain_agree_with_build_on_invalid_route_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = vec![
        (
            "signal.toml",
            "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/../evil/\"\n",
        ),
        ("content/posts/a.md", "---\ntitle: A\n---\n\nA body.\n"),
    ];
    files.extend(base_templates());
    write_site(dir.path(), &files);
    let out = dir.path().join("out");

    build_site_from_disk(dir.path(), &out).expect_err("build rejects bad prefix");
    link_check::check_site_from_disk(dir.path()).expect_err("check rejects bad prefix");
    explain::explain_site_from_disk(dir.path(), &out).expect_err("explain rejects bad prefix");
}

/// A broken internal reference is rejected consistently by build, check,
/// and explain — and explain performs no execution to achieve it.
#[test]
fn build_check_and_explain_agree_on_broken_reference() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = vec![
        (
            "signal.toml",
            "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
        ),
        (
            "content/posts/a.md",
            "---\ntitle: A\n---\n\nSee [ghost](/nope/).\n",
        ),
    ];
    files.extend(base_templates());
    write_site(dir.path(), &files);
    let out = dir.path().join("out");

    let build_err = build_site_from_disk(dir.path(), &out).expect_err("build rejects");
    assert!(
        format!("{build_err:?}").contains("Reference"),
        "got: {build_err:?}"
    );
    let check_err = link_check::check_site_from_disk(dir.path()).expect_err("check rejects");
    assert!(
        format!("{check_err:?}").contains("Reference"),
        "got: {check_err:?}"
    );
    let explain_err =
        explain::explain_site_from_disk(dir.path(), &out).expect_err("explain rejects");
    assert!(
        format!("{explain_err:?}").contains("Reference"),
        "got: {explain_err:?}"
    );
    // Explain rejected before planning: no output tree was created.
    assert!(
        !out.join("posts/a/index.html").exists(),
        "explain must not execute"
    );
}

/// A valid site passes all three commands.
#[test]
fn valid_site_passes_build_check_and_explain() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    link_check::check_site_from_disk(dir.path()).expect("checks");
    let text = explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(text.contains("Build plan"), "got:\n{text}");
}

/// `check` is read-only: no output tree, no manifest mutation.
#[test]
fn check_does_not_write_or_persist() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let before_tree = snapshot_tree(&out);
    let before_manifest =
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest exists");

    let report = link_check::check_site_from_disk(dir.path()).expect("checks");
    assert_eq!(report.title, "T");
    assert_eq!(report.collections, vec!["posts".to_string()]);

    assert_eq!(snapshot_tree(&out), before_tree);
    assert_eq!(
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
        before_manifest
    );
}

/// `check` on an unbuilt site creates nothing.
#[test]
fn check_without_prior_build_creates_no_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    link_check::check_site_from_disk(dir.path()).expect("checks");
    assert!(!dir.path().join("dist").exists());
    assert!(!dir.path().join("out").exists());
    assert!(!dir.path().join(".signal").exists());
}
