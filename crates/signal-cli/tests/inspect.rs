//! Integration coverage for the page-scoped inspection projection.

use signal_cli::inspect::{inspect_page_from_disk, inspection_json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn signal_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_signal"))
}

fn write_site(dir: &Path, files: &[(&str, String)]) {
    for (relative, contents) in files {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, contents).expect("write");
    }
}

fn inspection_site(dir: &Path) {
    let headings = (0..101)
        .map(|index| format!("## Heading {index}\n\nBody {index}.\n"))
        .collect::<String>();
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Inspection\"\nbase_url = \"https://example.com/\"\nauthor = \"Site Author\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[related]\nlimit = 3\n"
                    .to_string(),
            ),
            (
                "content/posts/alpha.md",
                format!(
                    "---\ntitle: Alpha\ndate: 2026-02-01\nlastmod: 2026-02-15\ndescription: Alpha summary.\ntopics: [\"Rust\", \"Systems\"]\nfeatured: true\nimage: images/alpha.svg\n---\n\nAlpha links to [Beta](/posts/beta/) and [Encoded](/posts/caf%C3%A9/).\n\n{headings}"
                ),
            ),
            (
                "content/posts/beta.md",
                "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta links to [Alpha](/posts/alpha/#heading-0)."
                    .to_string(),
            ),
            (
                "content/posts/café.md",
                "---\ntitle: Encoded\n---\n\nEncoded route target."
                    .to_string(),
            ),
            (
                "content/posts/orphan-alt.md",
                "---\ntitle: Orphan alt\nimage_alt: No hero\n---\n\nNo hero image."
                    .to_string(),
            ),
            (
                "content/posts/draft.md",
                "---\ntitle: Draft\ndraft: true\n---\n\nNever inspectable.".to_string(),
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>".to_string(),
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>".to_string(),
            ),
            ("static/images/alpha.svg", "<svg></svg>\n".to_string()),
        ],
    );
}

#[test]
fn inspection_is_deterministic_and_exposes_resolved_page_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    let before = std::fs::read(dir.path().join("content/posts/alpha.md")).expect("source");

    let first = inspect_page_from_disk(dir.path(), "/posts/alpha/").expect("inspects");
    let second =
        inspect_page_from_disk(dir.path(), "content/posts/alpha.md").expect("source lookup");
    let first_json = inspection_json(&first);
    assert_eq!(
        first_json,
        inspection_json(&second),
        "selector must not change output"
    );
    assert_eq!(
        first_json,
        inspection_json(&first),
        "repeated serialization is stable"
    );
    assert_eq!(
        std::fs::read(dir.path().join("content/posts/alpha.md")).expect("source"),
        before,
        "inspection must not modify source"
    );

    let value: serde_json::Value = serde_json::from_str(&first_json).expect("json");
    assert_eq!(value["schema"], "signal.inspect/v1");
    assert_eq!(value["page"]["source"]["collection"], "posts");
    assert_eq!(value["page"]["source"]["path"], "alpha.md");
    assert_eq!(value["page"]["route"], "/posts/alpha/");
    assert_eq!(value["page"]["url"], "/posts/alpha/");
    assert_eq!(
        value["page"]["canonical_url"],
        "https://example.com/posts/alpha/"
    );
    assert_eq!(value["page"]["publication"]["state"], "published");
    assert_eq!(value["page"]["title"], "Alpha");
    assert_eq!(value["page"]["author"], "Site Author");
    assert_eq!(value["page"]["image"], "/images/alpha.svg");
    assert_eq!(value["page"]["tags"][0], "Rust");
    assert_eq!(value["page"]["headings"]["total"], 101);
    assert_eq!(
        value["page"]["headings"]["items"].as_array().unwrap().len(),
        100
    );
    assert_eq!(value["page"]["headings"]["truncated"], true);
    assert_eq!(
        value["page"]["outbound_links"]["items"][0]["target"],
        "/posts/beta/"
    );
    assert_eq!(
        value["page"]["outbound_links"]["items"][1]["target"],
        "/posts/café/"
    );
    assert_eq!(
        value["page"]["inbound_links"]["items"][0]["raw"],
        "/posts/alpha/#heading-0"
    );
    assert_eq!(value["page"]["assets"]["items"][0], "images/alpha.svg");
    assert_eq!(
        value["page"]["related"]["items"][0]["route"],
        "/posts/beta/"
    );
    assert_eq!(
        value["page"]["related"]["items"][0]["shared_tags"][0],
        "Rust"
    );
    assert_eq!(
        value["page"]["diagnostics"]["items"][0]["code"],
        "hero-alt-missing"
    );

    let encoded = inspection_json(
        &inspect_page_from_disk(dir.path(), "/posts/café/").expect("encoded route"),
    );
    let encoded_value: serde_json::Value = serde_json::from_str(&encoded).expect("encoded json");
    assert_eq!(encoded_value["page"]["route"], "/posts/café/");
    assert_eq!(
        encoded_value["page"]["inbound_links"]["items"][0]["raw"],
        "/posts/caf%C3%A9/"
    );

    let orphan = inspection_json(
        &inspect_page_from_disk(dir.path(), "/posts/orphan-alt/").expect("orphan alt"),
    );
    let orphan_value: serde_json::Value = serde_json::from_str(&orphan).expect("orphan json");
    assert!(orphan_value["page"].get("image_alt").is_none());
    assert!(
        !first_json.contains("Never inspectable")
            && !first_json.contains("Alpha links to")
            && !first_json.contains("Body 0."),
        "body content is intentionally not dumped: {first_json}"
    );
}

#[test]
fn draft_missing_and_traversal_selectors_are_not_exposed() {
    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    for selector in [
        "/posts/draft/",
        "content/posts/draft.md",
        "alpha.md",
        "../outside.md",
    ] {
        let error = inspect_page_from_disk(dir.path(), selector).expect_err("must reject");
        assert!(error.to_string().contains("unknown or non-entry page"));
    }
}

#[test]
fn malformed_metadata_and_broken_references_fail_before_inspection_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    std::fs::write(
        dir.path().join("content/posts/alpha.md"),
        "---\ntitle: [broken\n---\n\nBody.\n",
    )
    .expect("break metadata");
    assert!(inspect_page_from_disk(dir.path(), "/posts/alpha/").is_err());

    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\n---\n\n[missing](/posts/missing/)\n",
    )
    .expect("break reference");
    assert!(inspect_page_from_disk(dir.path(), "/posts/alpha/").is_err());
}

#[test]
fn empty_site_has_no_inspectable_page() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[("signal.toml", "[site]\ntitle = \"Empty\"\n".to_string())],
    );
    assert!(inspect_page_from_disk(dir.path(), "/").is_err());
}

#[cfg(unix)]
#[test]
fn inspection_keeps_existing_markdown_symlink_policy() {
    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    std::os::unix::fs::symlink("alpha.md", dir.path().join("content/posts/linked.md"))
        .expect("symlink");
    let inspection = inspect_page_from_disk(dir.path(), "/posts/linked/").expect("follows symlink");
    assert_eq!(inspection.page.source.path, "linked.md");
    assert_eq!(inspection.page.route, "/posts/linked/");
}

#[test]
fn cli_emits_only_the_json_contract() {
    let dir = tempfile::tempdir().expect("tempdir");
    inspection_site(dir.path());
    let output = Command::new(signal_bin())
        .args(["inspect", "--root"])
        .arg(dir.path())
        .args(["--format", "json", "/posts/alpha/"])
        .output()
        .expect("run inspect");
    assert!(output.status.success(), "stderr: {output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["schema"], "signal.inspect/v1");
    assert!(!value["page"].get("body").is_some());

    let unsupported = Command::new(signal_bin())
        .args(["inspect", "--root"])
        .arg(dir.path())
        .args(["--format", "text", "/posts/alpha/"])
        .output()
        .expect("run inspect");
    assert!(!unsupported.status.success());
}
