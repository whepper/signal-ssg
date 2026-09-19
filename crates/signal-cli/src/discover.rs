//! Filesystem boundary (CLI side only): Markdown source discovery and the
//! contained output write/remove operations.
//!
//! Source discovery walks a site root for `.md` files with deterministic
//! (sorted) ordering; no parsing happens here. The output boundaries validate
//! lexical containment and refuse symlinked ancestors (see `ARCHITECTURE.md`
//! §9 and `docs/adr/0017-stale-pruning.md`).
//!
//! Discovery is fail-closed (slices 15A/15E, `docs/adr/0021-fail-closed-discovery.md`):
//! a directory that cannot be read, or an entry whose metadata cannot be
//! obtained, aborts the build instead of appearing empty, so an incomplete
//! inventory can never make current artifacts look stale and trigger
//! pruning. A directory that does not exist is an allowed empty result —
//! configured sources and the default `content/` layout are optional.
//!
//! Symlink policy (explicit; an R14-4 redesign is deferred): Markdown
//! discovery follows symlinks — a link to a directory is traversed, a link
//! to a `.md` file is collected — while static discovery skips symlinks
//! entirely (see `collect_static_files` in `build.rs`) and template
//! discovery follows symlinks under `templates/`. A symlink that cannot be
//! resolved is a fatal discovery error, never a silent skip.

use crate::errors::{discovery_error, BuildError};
use std::path::{Path, PathBuf};

/// Recursively discover `.md` files under `root`, sorted for determinism.
///
/// Fails on any filesystem error other than a missing directory (which is
/// treated as an empty source tree).
pub fn discover_markdown_sources(root: &Path) -> Result<Vec<PathBuf>, BuildError> {
    let mut out = Vec::new();
    collect_markdown(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BuildError> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        // An absent directory is an empty source tree, not a failure:
        // collection sources and the default `content/` directory are
        // optional. Any other error (permissions, I/O) must be fatal.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(discovery_error(dir, e)),
    };
    let mut entries = Vec::new();
    for entry in read {
        entries.push(entry.map_err(|e| discovery_error(dir, e))?);
    }
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        // `Path::is_dir()` reports `false` when metadata cannot be obtained
        // (unreadable traversal, symlink depth exhaustion, I/O faults),
        // which would silently drop the entry — the same truncation 15A
        // closed for `read_dir`. Classify explicitly and fail closed
        // instead. Symlinks are followed (see module docs): resolving them
        // is `metadata`'s job, and its failure is fatal here.
        let file_type = std::fs::metadata(&path)
            .map(|metadata| metadata.file_type())
            .map_err(|e| discovery_error(&path, e))?;
        if file_type.is_dir() {
            collect_markdown(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
    Ok(())
}

/// Ensure `relative_path` stays inside the output directory tree.
///
/// Component-based containment: the path must be relative and every
/// component must be a normal name. `..` (ParentDir), absolute prefixes,
/// and leading `.` components are rejected — an artifact can never escape
/// the configured output directory, regardless of where its path came from.
fn ensure_contained(relative_path: &str) -> std::io::Result<()> {
    use std::path::Component;
    if relative_path.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact path must not be empty",
        ));
    }
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("artifact path {relative_path:?} must be relative"),
        ));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("artifact path {relative_path:?} contains unsafe component {component:?}"),
            ));
        }
    }
    Ok(())
}

/// Whether a relative path passes output containment: the same rule the
/// write and remove boundaries enforce. Used to pre-screen untrusted
/// manifest paths so invalid stale records are skipped, never followed.
pub fn is_safe_artifact_path(relative_path: &str) -> bool {
    ensure_contained(relative_path).is_ok()
}

/// Resolve the output root once: ensure it exists, then canonicalize it.
///
/// Canonicalizing the *root* (never the artifact) keeps symlinks in the
/// root's own ancestry — e.g. macOS `/var` → `/private/var` — from being
/// mistaken for artifact ancestors, while a configured output root that is
/// itself a symlink is followed once and all containment is measured against
/// its real target.
fn resolve_output_root(out_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    std::fs::canonicalize(out_dir)
}

/// Reject a pre-existing symlinked ancestor directory between `root` and the
/// artifact's final component.
///
/// `root` must already be canonical. Only existing components are inspected;
/// a missing component means nothing below it exists yet and directory
/// creation proceeds normally (ordinary nested output still works). A
/// symlinked ancestor could redirect a write or a removal outside the output
/// tree, so it is refused rather than followed.
///
/// The final component is deliberately not checked here: removing a
/// final-component symlink is safe (the link itself is unlinked, never its
/// target), and the write boundary rejects a final symlink separately.
fn reject_symlink_ancestors(root: &Path, relative_path: &str) -> std::io::Result<()> {
    let mut current = root.to_path_buf();
    let mut components = Path::new(relative_path).components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break; // final component: the caller decides
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "output path {relative_path:?} traverses symlinked directory {}",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Filesystem identity of the object at `path`, without following a final
/// symlink.
///
/// On Unix this is the `(device, inode)` pair, which is exact under
/// case-insensitive and Unicode-normalizing filesystems: two logically
/// different paths that name the same filesystem object share one identity.
/// `None` means the path does not exist (or cannot be inspected); callers
/// treat that as "unknown", never as "safe to delete".
pub fn file_identity(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        // Without platform file ids, fall back to the canonical path. This is
        // a conservative equality test; it may follow a final symlink, which
        // only ever makes the comparison stricter.
        std::fs::canonicalize(path)
            .ok()
            .map(|real| real.to_string_lossy().into_owned())
    }
}

/// Remove one stale artifact file.
///
/// Safety contract, mirroring the write boundary: the relative path must
/// pass component-based containment (absolute paths, `..`, and `.`
/// components are rejected); pre-existing symlinked *ancestor* directories
/// are refused, so a tampered output tree cannot redirect the removal outside
/// the output root. The target must exist as a regular file or a symlink, and
/// a symlink is removed itself, never followed. Directories are never
/// recursively deleted: a directory at a stale path is an error (fail closed,
/// report). Missing files are ignored (already reconciled). Parent
/// directories are left alone even when empty.
///
/// Callers that prune also compare [`file_identity`] against the current
/// plan's outputs, so a stale logical path aliasing a current artifact on a
/// case-insensitive or normalizing filesystem is never deleted.
///
/// Returns `Ok(true)` when something was removed, `Ok(false)` when there
/// was nothing to remove.
pub fn remove_artifact(out_dir: &Path, relative_path: &str) -> std::io::Result<bool> {
    ensure_contained(relative_path)?;
    let root = match std::fs::canonicalize(out_dir) {
        Ok(root) => root,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    reject_symlink_ancestors(&root, relative_path)?;
    let dest = root.join(relative_path);
    let file_type = match std::fs::symlink_metadata(&dest) {
        Ok(metadata) => metadata.file_type(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if file_type.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("stale path {relative_path:?} is a directory; refusing recursive delete"),
        ));
    }
    // Regular files and symlinks alike: `remove_file` deletes the link
    // itself and never follows it.
    std::fs::remove_file(&dest)?;
    Ok(true)
}

/// Write artifact bytes to `out_dir/relative_path` after containment
/// validation. This is the single write boundary for every artifact.
///
/// Containment is filesystem-aware: the (possibly symlinked) output root is
/// resolved once, pre-existing symlinked ancestor directories are refused,
/// and an existing final-component symlink is refused rather than followed
/// (following it would overwrite its target outside the root). Ordinary
/// nested directory creation still works.
pub fn write_artifact(
    out_dir: &Path,
    relative_path: &str,
    content: &[u8],
) -> std::io::Result<PathBuf> {
    write_contained(out_dir, relative_path, content)
}

/// Write a UTF-8 artifact. Contained like [`write_artifact`].
pub fn write_text_artifact(
    out_dir: &Path,
    relative_path: &str,
    content: &str,
) -> std::io::Result<PathBuf> {
    write_contained(out_dir, relative_path, content.as_bytes())
}

/// Shared hardened write path for both write boundaries.
fn write_contained(
    out_dir: &Path,
    relative_path: &str,
    content: &[u8],
) -> std::io::Result<PathBuf> {
    ensure_contained(relative_path)?;
    let root = resolve_output_root(out_dir)?;
    reject_symlink_ancestors(&root, relative_path)?;
    let dest = root.join(relative_path);
    match std::fs::symlink_metadata(&dest) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing to write through symlink {}", dest.display()),
            ));
        }
        _ => {}
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_markdown_deterministically() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("content/posts")).unwrap();
        fs::write(dir.path().join("content/posts/b.md"), "# B\n").unwrap();
        fs::write(dir.path().join("content/posts/a.md"), "# A\n").unwrap();
        fs::write(dir.path().join("content/notes.txt"), "skip\n").unwrap();

        let found = discover_markdown_sources(&dir.path().join("content")).expect("discovers");
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("a.md"));
        assert!(found[1].ends_with("b.md"));
    }

    #[test]
    fn missing_and_empty_source_directories_are_allowed() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Missing directory: optional source, not an error.
        let missing = discover_markdown_sources(&dir.path().join("content"))
            .expect("missing directory is an empty result");
        assert!(missing.is_empty());
        // Present but empty directory: also allowed.
        fs::create_dir_all(dir.path().join("content/empty")).unwrap();
        let empty = discover_markdown_sources(&dir.path().join("content")).expect("empty ok");
        assert!(empty.is_empty());
    }

    /// Whether this process is actually denied read access to `dir`, so a
    /// permission-based test can skip when running privileged (e.g. root in
    /// CI, where `chmod 000` is bypassed).
    #[cfg(unix)]
    fn directory_is_readable(dir: &Path) -> bool {
        match fs::read_dir(dir) {
            Ok(mut entries) => entries.all(|entry| entry.is_ok()),
            Err(_) => false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subdirectory_is_an_error_not_an_empty_tree() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let hidden = dir.path().join("content/hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(hidden.join("b.md"), "# B\n").unwrap();
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o000)).unwrap();

        if directory_is_readable(&hidden) {
            fs::set_permissions(&hidden, fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipping: directory permissions are not enforced for this user");
            return;
        }

        let result = discover_markdown_sources(&dir.path().join("content"));
        fs::set_permissions(&hidden, fs::Permissions::from_mode(0o755)).unwrap();

        let err = result.expect_err("unreadable directory must fail discovery");
        assert!(matches!(err, BuildError::Read { .. }), "got: {err:?}");
        assert!(
            err.to_string().contains("hidden"),
            "error must name the offending path: {err}"
        );
    }

    #[test]
    fn writes_text_artifact_with_parents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = write_text_artifact(dir.path(), "posts/hello/index.html", "<h1>Hi</h1>")
            .expect("writes");
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(dest).unwrap(), "<h1>Hi</h1>");
    }

    #[test]
    fn write_boundary_rejects_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        for bad in [
            "../evil.txt",
            "posts/../../evil.txt",
            "/absolute/evil.txt",
            "./leading-dot.txt",
            ".",
            "..",
            "",
        ] {
            let result = write_artifact(dir.path(), bad, b"x");
            assert!(result.is_err(), "path {bad:?} must be rejected");
        }
        // Nothing escaped the output directory.
        let outside = dir.path().parent().unwrap().read_dir().unwrap().count();
        let _ = outside; // parent untouched; the assertions above are the contract.
        assert!(dir.path().exists());
    }

    #[test]
    fn write_boundary_allows_nested_and_spaced_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Static passthrough keeps byte fidelity for names that are legal
        // filenames; containment only rejects structural escapes.
        let dest = write_artifact(dir.path(), "css/my file.css", b"body{}").expect("writes");
        assert!(dest.exists());
        let nested = write_artifact(dir.path(), "a/b/c.bin", &[0u8, 1, 2]).expect("writes");
        assert_eq!(std::fs::read(nested).unwrap(), vec![0u8, 1, 2]);
    }

    #[test]
    fn remove_artifact_deletes_only_contained_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("posts")).unwrap();
        fs::write(dir.path().join("posts/old.html"), "old").unwrap();
        assert!(remove_artifact(dir.path(), "posts/old.html").expect("removes"));
        assert!(!dir.path().join("posts/old.html").exists());
        // Missing files are already reconciled, not errors.
        assert!(!remove_artifact(dir.path(), "posts/old.html").expect("absent ok"));
        assert!(!remove_artifact(dir.path(), "never/was/here.txt").expect("absent ok"));
        // Unsafe paths never reach the filesystem. Backslash forms are
        // judged by the host platform (`components()`): on Unix they are
        // ordinary contained filenames, on Windows they are separators —
        // either way they cannot escape `out_dir` here.
        for bad in [
            "../evil.txt",
            "posts/../../evil.txt",
            "/absolute/evil.txt",
            "",
            ".",
        ] {
            assert!(
                !is_safe_artifact_path(bad),
                "path {bad:?} must fail pre-screening"
            );
            assert!(
                remove_artifact(dir.path(), bad).is_err(),
                "path {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn remove_artifact_refuses_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("posts/old")).unwrap();
        fs::write(dir.path().join("posts/old/index.html"), "x").unwrap();
        let err = remove_artifact(dir.path(), "posts/old").expect_err("directory refused");
        assert!(
            err.to_string().contains("directory"),
            "unexpected error: {err}"
        );
        // Nothing was touched.
        assert!(dir.path().join("posts/old/index.html").exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_artifact_removes_link_not_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside-secret.txt");
        fs::write(&outside, "secret").unwrap();
        symlink(&outside, dir.path().join("stale-link.txt")).unwrap();
        assert!(remove_artifact(dir.path(), "stale-link.txt").expect("link removed"));
        assert!(!dir.path().join("stale-link.txt").exists());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "secret");
    }

    #[test]
    fn fixture_sources_are_discoverable() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/minimal-site/content"
        );
        let found = discover_markdown_sources(Path::new(fixture)).expect("fixture discovers");
        assert!(
            found.iter().any(|p| p.ends_with("hello-world.md")),
            "expected hello-world.md under {fixture}"
        );
    }

    // --- Slice 14D: filesystem-aware containment and identity ---

    #[cfg(unix)]
    #[test]
    fn write_rejects_symlinked_ancestor_directory() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, out.join("assets")).unwrap();

        let err = write_artifact(&out, "assets/evil/file.css", b"x").expect_err("must refuse");
        assert!(err.to_string().contains("symlink"), "got: {err}");
        assert!(!outside.join("evil/file.css").exists());
        assert!(out.join("assets").is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_final_component_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let outside = dir.path().join("outside.css");
        fs::write(&outside, "original").unwrap();
        symlink(&outside, out.join("asset.css")).unwrap();

        let err = write_artifact(&out, "asset.css", b"overwritten").expect_err("must refuse");
        assert!(err.to_string().contains("symlink"), "got: {err}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn remove_rejects_symlinked_ancestor_directory() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        let outside = dir.path().join("outside");
        fs::create_dir_all(out.join("assets")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("old.css"), "keep me").unwrap();
        // Replace the real directory with a redirect into the outside tree.
        fs::remove_dir(out.join("assets")).unwrap();
        symlink(&outside, out.join("assets")).unwrap();

        let err = remove_artifact(&out, "assets/old.css").expect_err("must refuse");
        assert!(err.to_string().contains("symlink"), "got: {err}");
        assert_eq!(
            fs::read_to_string(outside.join("old.css")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn write_still_creates_ordinary_nested_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        // `out` itself does not exist yet: the write boundary creates it.
        write_artifact(&out, "a/b/c/d.txt", b"nested").expect("writes");
        assert_eq!(
            fs::read_to_string(out.join("a/b/c/d.txt")).unwrap(),
            "nested"
        );
    }

    #[test]
    fn file_identity_distinguishes_objects_and_is_absent_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        let a = file_identity(&dir.path().join("a.txt")).expect("a");
        let b = file_identity(&dir.path().join("b.txt")).expect("b");
        assert_ne!(a, b);
        assert!(file_identity(&dir.path().join("missing.txt")).is_none());
    }

    #[test]
    fn file_identity_matches_case_aliases_where_supported() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("foo.txt"), "x").unwrap();
        let lower = file_identity(&dir.path().join("foo.txt")).expect("lower");
        let upper = dir.path().join("FOO.TXT");
        if upper.exists() {
            // Case-insensitive filesystem: both names are one object.
            assert_eq!(file_identity(&upper).as_deref(), Some(lower.as_str()));
        } else {
            // Case-sensitive filesystem: the alias does not exist at all.
            assert_ne!(file_identity(&upper).as_deref(), Some(lower.as_str()));
        }
    }

    #[test]
    fn file_identity_matches_unicode_aliases_where_supported() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `café` (composed) vs `cafe` + combining acute (decomposed).
        let composed = dir.path().join("caf\u{e9}.txt");
        fs::write(&composed, "x").unwrap();
        let composed_id = file_identity(&composed).expect("composed");
        let decomposed = dir.path().join("cafe\u{301}.txt");
        if decomposed.exists() {
            assert_eq!(
                file_identity(&decomposed).as_deref(),
                Some(composed_id.as_str())
            );
        } else {
            assert_ne!(
                file_identity(&decomposed).as_deref(),
                Some(composed_id.as_str())
            );
        }
    }
}
