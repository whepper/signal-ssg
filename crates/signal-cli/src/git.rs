//! Git-derived content metadata (opt-in via `[git] last_modified`).
//!
//! Process execution lives here, in the crate that already owns the
//! filesystem boundary; `signal-core` never sees Git. One `git log`
//! invocation maps every tracked file under the site root to the author
//! date (`YYYY-MM-DD`) of the last commit that touched it.
//!
//! Failure semantics: Git metadata is advisory, never mandatory. A missing
//! `git` binary, a site outside a repository, or a failing repository query
//! all yield an empty map, and the build proceeds with no Git-derived dates.
//! Within a given repository state the mapping is deterministic — same
//! commits, same dates — but it intentionally reflects repository history,
//! so new commits legitimately change derived dates (and via the entry
//! digests, invalidate affected pages).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use signal_core::parse_ymd;

/// Map each file under `root` to its last-touch commit author date.
///
/// Keys are site-root-relative, `/`-separated paths (matching
/// `source.root_relative` at the ingest boundary); values are `YYYY-MM-DD`
/// strings. Newest commit wins: `git log` lists commits newest-first, and
/// only the first mapping per path is kept. Paths outside the site root
/// (when the site lives in a subdirectory of its repository) are ignored.
pub(crate) fn last_modified_dates(root: &Path) -> BTreeMap<String, String> {
    let mut dates = BTreeMap::new();
    // Site-root-relative prefix inside the repository (empty when the site
    // root IS the repository root). `rev-parse` also tells us whether Git
    // is usable here at all.
    let Some(prefix) = repo_prefix(root) else {
        return dates;
    };
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        // Raw UTF-8 paths: quoting would make matches unreliable.
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("log")
        // `\x01` marks each commit record, so dates and changed paths can
        // never be confused with each other or with commit separators.
        .arg("--format=%x01%ad")
        .arg("--date=short")
        .arg("--name-only")
        // Bounded scope: only commits touching anything under the site
        // root participate.
        .arg("--")
        .arg(".")
        .output()
    else {
        return dates;
    };
    if !output.status.success() {
        return dates;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Each record: the marker, the author date line, a blank line, then the
    // changed file paths. Commits arrive newest-first, so only the first
    // (newest) date per path is kept.
    for record in text.split('\u{1}').skip(1) {
        let mut lines = record.lines();
        let Some(date_line) = lines.next() else {
            continue;
        };
        let date = date_line.trim();
        if parse_ymd(date).is_none() {
            continue;
        }
        for path in lines {
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            // Repo-relative → site-root-relative; ignore anything outside
            // the site root.
            if let Some(relative) = path.strip_prefix(&prefix).filter(|p| !p.is_empty()) {
                dates
                    .entry(relative.to_string())
                    .or_insert_with(|| date.to_string());
            }
        }
    }
    dates
}

/// Site-root-relative prefix of `root` inside its repository, with the
/// trailing slash kept (empty string when `root` is the repository root).
/// `None` when Git is unavailable or `root` is not inside a work tree.
fn repo_prefix(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("--show-prefix")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as SysCommand;

    /// Run git in `root` with a fixed identity and optional date env.
    fn git(root: &Path, args: &[&str], author_date: Option<&str>) {
        let mut cmd = SysCommand::new("git");
        cmd.arg("-C")
            .arg(root)
            .arg("-c")
            .arg("user.name=Signal Test")
            .arg("-c")
            .arg("user.email=test@example.com")
            .args(args);
        if let Some(date) = author_date {
            cmd.env("GIT_AUTHOR_DATE", date);
            cmd.env("GIT_COMMITTER_DATE", date);
        }
        let output = cmd.output().expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_all(root: &Path, date: &str, message: &str) {
        git(root, &["add", "-A"], None);
        git(root, &["commit", "-m", message], Some(date));
    }

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, content).expect("write");
    }

    #[test]
    fn maps_files_to_their_last_commit_date() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init"], None);
        write(root, "content/posts/a.md", "A\n");
        commit_all(root, "2026-01-01T00:00:00 +0000", "a");
        write(root, "content/posts/b.md", "B\n");
        commit_all(root, "2026-03-05T00:00:00 +0000", "b");
        // Touch `a.md` again: the newest commit wins for that path.
        write(root, "content/posts/a.md", "A2\n");
        commit_all(root, "2026-06-01T00:00:00 +0000", "a again");

        let dates = last_modified_dates(root);
        assert_eq!(
            dates.get("content/posts/a.md").map(String::as_str),
            Some("2026-06-01")
        );
        assert_eq!(
            dates.get("content/posts/b.md").map(String::as_str),
            Some("2026-03-05")
        );
    }

    #[test]
    fn site_root_inside_a_repository_gets_prefix_stripped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        git(repo, &["init"], None);
        let root = repo.join("site");
        write(&root, "content/posts/a.md", "A\n");
        commit_all(repo, "2026-02-02T00:00:00 +0000", "site content");

        let dates = last_modified_dates(&root);
        assert_eq!(
            dates.get("content/posts/a.md").map(String::as_str),
            Some("2026-02-02"),
            "keys are site-root-relative, not repo-relative: {dates:?}"
        );
    }

    #[test]
    fn non_repository_yields_no_dates() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "content/posts/a.md", "A\n");
        assert!(last_modified_dates(dir.path()).is_empty());
    }

    #[test]
    fn deletion_commits_map_to_their_date_and_harm_nothing() {
        // A deleted path is listed under the commit that deleted it. The
        // map records that date; it is harmless — ingestion only ever looks
        // up paths of source files that exist.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init"], None);
        write(root, "keep.md", "K\n");
        write(root, "gone.md", "G\n");
        commit_all(root, "2026-01-01T00:00:00 +0000", "both");
        fs::remove_file(root.join("gone.md")).expect("remove");
        commit_all(root, "2026-02-02T00:00:00 +0000", "delete");

        let dates = last_modified_dates(root);
        assert_eq!(dates.get("keep.md").map(String::as_str), Some("2026-01-01"));
        assert_eq!(dates.get("gone.md").map(String::as_str), Some("2026-02-02"));
    }
}
