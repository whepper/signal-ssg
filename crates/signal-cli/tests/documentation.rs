//! Documentation truth-pass regression guards (slice 14E).
//!
//! The README and ARCHITECTURE.md are part of the project's contract. These
//! checks pin the claims corrected across slices 14A–14E — incremental builds
//! are implemented, template invalidation is the conservative template set
//! (never exact per-artifact closure), and the build is not whole-tree
//! transactional. They read the workspace files at compile time via
//! `include_str!`, mirroring the existing crate-boundary invariant tests.
//!
//! They are deliberately a small, exact set of phrases, not a documentation
//! linter.

const README: &str = include_str!("../../../README.md");
const ARCHITECTURE: &str = include_str!("../../../ARCHITECTURE.md");

const DOCS: [(&str, &str); 2] = [("README.md", README), ("ARCHITECTURE.md", ARCHITECTURE)];

#[test]
fn stale_pre_14e_claims_do_not_reappear() {
    for (name, text) in DOCS {
        for stale in [
            "Build manifest (future direction)",
            "manifest (future)",
            "future build manifest",
            "future incremental builds",
            "incremental builds, plugins",
            "no skipping or pruning yet",
        ] {
            assert!(
                !text.contains(stale),
                "{name} reintroduced stale claim {stale:?}"
            );
        }
    }
}

#[test]
fn architecture_documents_current_incremental_semantics() {
    for required in [
        "generation compatibility",
        "InputRef::TemplateSet",
        "check-then-use",
        "whole-tree transactional",
        "no per-artifact",
    ] {
        assert!(
            ARCHITECTURE.contains(required),
            "ARCHITECTURE.md is missing {required:?}"
        );
    }
}

#[test]
fn readme_documents_current_reuse_and_template_semantics() {
    for required in [
        "complete loaded template set",
        "previous manifest − current plan",
        "whole-tree transactional",
        "consults mtimes",
    ] {
        assert!(
            README.contains(required),
            "README.md is missing {required:?}"
        );
    }
}
