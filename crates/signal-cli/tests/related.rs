//! Related content (A8): architectural probes for the page-local `related`
//! projection.
//!
//! The design decision is ADR 0035. These probes protect the one property
//! the existing suite does not: a *newly created* entry can change other
//! entries' related projections, and that reverse dependency is carried by
//! each viewer's own `Query{related:<route>}` input — never by a reverse
//! edge, a shared artifact, or a second mechanism. Measured through real
//! build reuse, not planner branches.

use signal_cli::build::build_site_from_disk;
use std::path::Path;

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

/// Two collections. Alpha and Beta share the `Rust` topic (so each lists
/// the other as related); Note carries `Misc` and shares nothing.
fn related_site(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Related\"\nbase_url = \"https://example.com/\"\n\
                 [collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n\
                 [collections.notes]\nsource = \"content/notes\"\nroute_prefix = \"/notes/\"\n\
                 [taxonomy]\nroute_prefix = \"/topics/\"\n",
            ),
            (
                "content/posts/alpha.md",
                "---\ntitle: Alpha\ndate: 2026-02-01\ntopics: [\"Rust\"]\n---\n\nAlpha body words.\n",
            ),
            (
                "content/posts/beta.md",
                "---\ntitle: Beta\ndate: 2026-01-01\ntopics: [\"Rust\"]\n---\n\nBeta body words.\n",
            ),
            (
                "content/notes/n.md",
                "---\ntitle: Note\ndate: 2026-03-01\ntopics: [\"Misc\"]\n---\n\nNote body.\n",
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
        ],
    );
}

#[test]
fn new_tagged_entry_rebuilds_existing_viewers() {
    let dir = tempfile::tempdir().expect("tempdir");
    related_site(dir.path());
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("first builds");

    // A new entry sharing `Rust` joins Alpha's and Beta's related sets.
    // Neither viewer's own entry changed, yet both must rebuild: the
    // reverse dependency is expressed by each viewer's own
    // `Query{related:<route>}` input, recomputed against the current model.
    write_site(
        dir.path(),
        &[(
            "content/posts/gamma.md",
            "---\ntitle: Gamma\ndate: 2026-01-02\ntopics: [\"Rust\"]\n---\n\nGamma body.\n",
        )],
    );
    let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
    let rebuilt = summary.rebuilt_paths;
    for viewer in ["posts/alpha/index.html", "posts/beta/index.html"] {
        assert!(
            rebuilt.iter().any(|path| path == viewer),
            "viewer {viewer} must rebuild; rebuilt: {rebuilt:?}"
        );
    }
    // Precision: the new entry shares no topic with Note, so Note's related
    // projection (and its page) is untouched.
    for reused in ["notes/n/index.html", "topics/misc/index.html"] {
        assert!(
            !rebuilt.iter().any(|path| path == reused),
            "{reused} must stay reused; rebuilt: {rebuilt:?}"
        );
    }
}
