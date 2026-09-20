//! CLI integration for `signal build --explain`: the actual binary reports
//! the plan without writing artifacts, pruning stale outputs, or
//! persisting the manifest.

use std::path::{Path, PathBuf};
use std::process::Command;

fn signal_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_signal"))
}

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

fn fixture(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Explain CLI\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[feed]\n",
            ),
            (
                "content/posts/alpha.md",
                "---\ntitle: Alpha\ndate: 2026-02-01\n---\n\nAlpha body words here.\n",
            ),
            (
                "content/posts/beta.md",
                "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta body words here.\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
            ("static/asset.txt", "asset-1"),
        ],
    );
}

fn run_build(dir: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(signal_bin());
    cmd.arg("build")
        .arg("--root")
        .arg(dir)
        .arg("--out")
        .arg(out);
    for flag in extra {
        cmd.arg(flag);
    }
    cmd.output().expect("run signal")
}

fn has_artifact(out: &Path) -> bool {
    out.join("posts/alpha/index.html").exists() || out.join(".signal/manifest.json").exists()
}

#[test]
fn explain_before_first_build_reports_no_usable_manifest_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");

    let output = run_build(dir.path(), &out, &["--explain"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("Build plan\n"), "got:\n{stdout}");
    assert!(
        stdout.contains("reason: no usable manifest"),
        "got:\n{stdout}"
    );
    assert!(stdout.contains("Prune:\n  (none)\n"), "got:\n{stdout}");
    // Read-only: no artifacts, no manifest — even though validation may
    // create the (empty) output directory for its transient alias probe.
    assert!(!has_artifact(&out), "explain must write nothing");
    // Deterministic: a second run is byte-identical.
    let again = run_build(dir.path(), &out, &["--explain"]);
    assert!(again.status.success());
    assert_eq!(stdout, String::from_utf8(again.stdout).expect("utf8"));
}

#[test]
fn explain_after_build_reports_reuse_then_rebuild_reasons_without_side_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");

    let built = run_build(dir.path(), &out, &[]);
    assert!(built.status.success());
    assert!(has_artifact(&out));

    // Unchanged state: everything reuses.
    let output = run_build(dir.path(), &out, &["--explain"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("rebuild:   0\n"), "got:\n{stdout}");
    assert!(stdout.contains("Rebuild:\n  (none)\n"), "got:\n{stdout}");

    let manifest_before =
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest exists");

    // Edit one entry and delete another: mixed reuse/rebuild/stale.
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta changed\ndate: 2026-03-01\n---\n\nBeta body words here.\n",
    )
    .expect("edit");
    std::fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");

    let output = run_build(dir.path(), &out, &["--explain"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(
        stdout.contains("posts/beta/index.html\n    reason: entry changed: /posts/beta/"),
        "got:\n{stdout}"
    );
    let prune = stdout.split("Prune:\n").nth(1).expect("prune section");
    assert!(prune.contains("posts/alpha/index.html"), "got:\n{stdout}");
    // Untouched artifacts still reuse — explain itself changed nothing.
    assert!(stdout.contains("  asset.txt\n"), "got:\n{stdout}");
    assert_eq!(
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
        manifest_before
    );
    // Stale output still on disk; edited output still has old bytes.
    assert!(out.join("posts/alpha/index.html").exists());
    let beta = std::fs::read_to_string(out.join("posts/beta/index.html")).expect("beta");
    assert!(!beta.contains("Beta changed"), "got: {beta}");
}
