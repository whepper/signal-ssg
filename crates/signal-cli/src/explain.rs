//! Read-only build-plan diagnostics (`signal build --explain`).
//!
//! Rendering lives at the CLI boundary: [`render_plan`] turns an already
//! computed [`BuildPlan`] into deterministic human-readable text, and
//! [`explain_site_from_disk`] constructs that plan through the exact same
//! path execution uses ([`validated_plan`](crate::build::validated_plan)
//! plus [`plan`](crate::build_plan::plan)) — then stops. It resolves
//! nothing, writes nothing, prunes nothing, and persists no manifest.
//!
//! Output conventions follow the `signal build` summary: two-space
//! indentation, relative artifact paths only, no timestamps, no colors, no
//! absolute paths. Decisions render in plan (sorted-path) order and the
//! stale list renders sorted, so repeated runs are byte-identical.

use std::path::Path;

use signal_core::{SignalConfig, SiteModel};

use crate::build_plan::{BuildDecision, BuildPlan};
use crate::errors::BuildError;

/// Render one [`BuildPlan`] as deterministic human-readable text.
///
/// Pure function of the plan: rendering the same plan twice yields
/// byte-identical output.
pub fn render_plan(planned: &BuildPlan) -> String {
    let mut out = String::new();
    out.push_str("Build plan\n");
    out.push_str("==========\n");
    out.push_str("\nSummary:\n");
    out.push_str(&format!("  artifacts: {}\n", planned.decisions.len()));
    out.push_str(&format!("  reuse:     {}\n", planned.reused_count()));
    out.push_str(&format!("  rebuild:   {}\n", planned.rebuilt_count()));
    out.push_str(&format!("  stale:     {}\n", planned.stale.len()));
    out.push_str("\nReuse:\n");
    let mut reused = false;
    for decision in &planned.decisions {
        if let BuildDecision::Reuse { spec, .. } = decision {
            out.push_str(&format!("  {}\n", spec.path));
            reused = true;
        }
    }
    if !reused {
        out.push_str("  (none)\n");
    }
    out.push_str("\nRebuild:\n");
    let mut rebuilt = false;
    for decision in &planned.decisions {
        if let BuildDecision::Rebuild { spec, reason } = decision {
            out.push_str(&format!("  {}\n", spec.path));
            out.push_str(&format!("    reason: {reason}\n"));
            rebuilt = true;
        }
    }
    if !rebuilt {
        out.push_str("  (none)\n");
    }
    out.push_str("\nPrune:\n");
    if planned.stale.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for stale in &planned.stale {
            out.push_str(&format!("  {stale}\n"));
        }
    }
    out
}

/// Construct the plan execution would use, without executing it.
///
/// Runs the canonical pre-execution pipeline — the identical
/// [`validate_current_site`](crate::pipeline::validate_current_site)
/// construction [`build_site`](crate::build::build_site) starts from
/// (structural validation plus reference validation) — then plans and
/// returns before any resolve, write, prune, or manifest persist.
/// A site that would fail the build's pre-plan gates (invalid routes,
/// menus, output collisions, broken references) fails here identically:
/// `--explain` describes the plan for a site that would otherwise pass
/// validation, never a plan for a site the build would reject.
/// Validation runs as in a build, including its transient
/// filesystem-alias probe (created and removed during validation); no
/// artifacts are resolved or written, nothing is pruned, and
/// `.signal/manifest.json` is never touched.
pub fn explain_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<String, BuildError> {
    let validated = crate::pipeline::validate_current_site(root, out_dir, config, model)?;
    let previous = crate::manifest::load_previous(out_dir);
    let planned = crate::build_plan::plan(
        &validated.plan.specs,
        config,
        model,
        &validated.renderer,
        root,
        out_dir,
        &previous,
    )?;
    Ok(render_plan(&planned))
}

/// Convenience: ingest from disk and explain in one call (used by the CLI).
pub fn explain_site_from_disk(root: &Path, out_dir: &Path) -> Result<String, BuildError> {
    let loaded = crate::pipeline::load_validated_site(root, out_dir)?;
    explain_validated_site(
        root,
        out_dir,
        &loaded.config,
        &loaded.model,
        loaded.validated.plan,
        loaded.validated.renderer,
    )
}

/// Explain an already-validated site state without revalidating.
///
/// Exists so the CLI loads once; behavior is identical to [`explain_site`].
#[allow(clippy::too_many_arguments)]
pub fn explain_validated_site(
    root: &Path,
    out_dir: &Path,
    config: &SignalConfig,
    model: &SiteModel,
    plan: crate::build::SpecPlan,
    renderer: signal_render::MiniJinjaRenderer,
) -> Result<String, BuildError> {
    let previous = crate::manifest::load_previous(out_dir);
    let planned = crate::build_plan::plan(
        &plan.specs,
        config,
        model,
        &renderer,
        root,
        out_dir,
        &previous,
    )?;
    Ok(render_plan(&planned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_plan::RebuildReason;

    fn write_site(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, content).expect("write");
        }
    }

    fn plan_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Explain\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[taxonomy]\nroute_prefix = \"/topics/\"\n[feed]\n",
                ),
                (
                    "content/posts/alpha.md",
                    "---\ntitle: Alpha\ndate: 2026-02-01\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
                ),
                (
                    "content/posts/beta.md",
                    "---\ntitle: Beta\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
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
                ("static/asset.txt", "asset-1"),
            ],
        );
    }

    fn snapshot_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut entries = Vec::new();
        if !dir.is_dir() {
            return entries;
        }
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let mut children: Vec<_> = std::fs::read_dir(&current)
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

    #[test]
    fn every_rebuild_reason_renders() {
        let cases: Vec<(RebuildReason, &str)> = vec![
            (RebuildReason::NoUsableManifest, "no usable manifest"),
            (RebuildReason::GenerationMismatch, "generation changed"),
            (RebuildReason::MissingRecord, "manifest record missing"),
            (RebuildReason::KindChanged, "artifact kind changed"),
            (RebuildReason::InputsChanged, "inputs changed"),
            (
                RebuildReason::EntryChanged {
                    route: "/blog/foo/".to_string(),
                },
                "entry changed: /blog/foo/",
            ),
            (
                RebuildReason::QueryChanged {
                    key: "summaries:posts".to_string(),
                },
                "query changed: summaries:posts",
            ),
            (RebuildReason::TemplateSetChanged, "template set changed"),
            (
                RebuildReason::TemplateChanged {
                    name: "post.html".to_string(),
                },
                "template changed: post.html",
            ),
            (RebuildReason::ConfigChanged, "configuration changed"),
            (
                RebuildReason::StaticChanged {
                    path: "images/foo.png".to_string(),
                },
                "static file changed: images/foo.png",
            ),
            (RebuildReason::OutputMissing, "output missing"),
            (RebuildReason::OutputChanged, "output changed"),
            (
                RebuildReason::InputError {
                    message: "boom".to_string(),
                },
                "input error: boom",
            ),
        ];
        assert_eq!(cases.len(), 14);
        for (reason, expected) in &cases {
            assert_eq!(reason.to_string(), *expected, "reason {reason:?}");
        }
    }

    #[test]
    fn unchanged_build_explains_all_reuse() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");
        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert!(text.contains("rebuild:   0\n"), "got:\n{text}");
        assert!(text.contains("stale:     0\n"), "got:\n{text}");
        assert!(text.contains("Reuse:\n"), "got:\n{text}");
        assert!(text.contains("Rebuild:\n  (none)\n"), "got:\n{text}");
        assert!(text.contains("Prune:\n  (none)\n"), "got:\n{text}");
        // Deterministic: explaining twice is byte-identical.
        let again = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert_eq!(text, again);
    }

    #[test]
    fn mixed_plan_renders_reuse_rebuild_and_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");

        // One content edit (rebuild with reason) plus one deletion (stale).
        std::fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta changed\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        std::fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");
        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        // Edited entry rebuilds with its entry reason…
        assert!(
            text.contains("  posts/beta/index.html\n    reason: entry changed: /posts/beta/\n"),
            "got:\n{text}"
        );
        // …the deleted entry's output is reported stale…
        assert!(text.contains("  posts/alpha/index.html\n"), "got:\n{text}");
        assert!(text.contains("Prune:\n"), "got:\n{text}");
        let prune = text.split("Prune:\n").nth(1).expect("prune section");
        assert!(
            prune.contains("posts/alpha/index.html"),
            "stale must list alpha: {prune}"
        );
        // …and untouched artifacts still reuse.
        assert!(text.contains("  asset.txt\n"), "got:\n{text}");
        // Deterministic rendering of the same state.
        let again = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert_eq!(text, again);
    }

    #[test]
    fn explain_is_read_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        plan_site(dir.path());
        let out = dir.path().join("out");
        crate::build::build_site_from_disk(dir.path(), &out).expect("builds");

        // Create pending work: an edit (rebuild) and a deletion (stale).
        std::fs::write(
            dir.path().join("content/posts/beta.md"),
            "---\ntitle: Beta changed\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
        )
        .expect("edit");
        std::fs::remove_file(dir.path().join("content/posts/alpha.md")).expect("delete");

        let before_outputs = snapshot_dir(&out);
        let before_manifest =
            std::fs::read(out.join(".signal/manifest.json")).expect("manifest exists");
        // The stale output is still on disk before explaining.
        assert!(out.join("posts/alpha/index.html").exists());

        let text = explain_site_from_disk(dir.path(), &out).expect("explains");
        assert!(text.contains("rebuild:"), "got:\n{text}");

        // Nothing was written, removed, or persisted.
        assert_eq!(snapshot_dir(&out), before_outputs);
        assert_eq!(
            std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
            before_manifest
        );
        assert!(out.join("posts/alpha/index.html").exists());
        // The edited output still holds the previous build's bytes.
        let beta = std::fs::read_to_string(out.join("posts/beta/index.html")).expect("beta");
        assert!(!beta.contains("Beta changed"), "got: {beta}");
    }
}
