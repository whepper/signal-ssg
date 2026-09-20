//! Development server (`signal serve`): rebuild and reserve the existing
//! production pipeline.
//!
//! `serve` calls [`crate::build::build_site_from_disk`] for the initial
//! build and every rebuild — the same entry point as `signal build`. There
//! is no serve-specific ingestion, planning, rendering, pruning, or
//! manifest logic; [`crate::build_plan::BuildPlan`] (incremental
//! reuse/rebuild/stale decisions) remains authoritative.
//! No live reload is implemented: refresh the browser after a rebuild.
//!
//! Failure model (inherited from the build, not redesigned here):
//! failures before any write (invalid content, config, templates, output
//! collisions, broken references) leave the served tree untouched; a
//! failure mid-execution
//! may leave a mix of new and old files with the previous manifest intact,
//! and the next successful build reconciles via the existing `BuildPlan`.
//! The serve loop never exits on a rebuild error and never rolls files
//! back by hand.
//!
//! Concurrency: one blocking thread serves HTTP while the calling thread
//! watches, coalesces events, and rebuilds. Builds never run concurrently:
//! an event arriving mid-build is processed after the current build
//! completes. No async runtime. Ctrl-C terminates the process (default
//! disposition); no half-written manifest can result because manifest
//! replacement is atomic in the build itself.

use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use notify::{Config, RecursiveMode, Watcher};

use crate::build::BuildSummary;
use crate::errors::BuildError;

/// Default quiet window for coalescing filesystem events into one rebuild.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Options for [`serve_forever`]. Mirrors `signal build` roots plus bind address.
#[derive(Clone, Debug)]
pub struct ServeOptions {
    /// Site root containing `signal.toml`.
    pub root: PathBuf,
    /// Output directory to generate and serve.
    pub out: PathBuf,
    /// Interface to bind, e.g. `127.0.0.1`.
    pub host: String,
    /// Port to bind; `0` picks an ephemeral port and reports it.
    pub port: u16,
    /// Quiet window after the last event before rebuilding.
    pub debounce: Duration,
}

/// Guess the `Content-Type` for a served file from its extension.
///
/// Unknown extensions fall back to `application/octet-stream`.
pub fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Decode `%XX` sequences in a URL path. Malformed sequences fail closed.
fn percent_decode(raw: &str) -> Option<String> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hex = |b: u8| (b as char).to_digit(16);
                let (hi, lo) = (hex(bytes[i + 1])?, hex(bytes[i + 2])?);
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Map a request URL path to a file under the output directory.
///
/// Returns `None` when the request must not be served (undecodable,
/// traversal attempt, or missing file). Directory routes resolve to their
/// `index.html`, with or without trailing slash, mirroring
/// `route_to_output_path`. The result never escapes `out_dir`: decoded
/// `.`/`..` segments are rejected before joining, and the joined path is
/// verified to stay within the canonical output root.
pub(crate) fn resolve_request_path(out_dir: &Path, url_path: &str) -> Option<PathBuf> {
    let path_only = url_path.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path_only)?;
    let mut relative = PathBuf::new();
    for segment in decoded.split('/') {
        if segment.is_empty() {
            continue;
        }
        // Decoded `.`/`..` never become path components; anything else is a
        // plain file or directory name (backslashes are ordinary names on
        // Unix and stay jailed by the prefix check below).
        if segment == "." || segment == ".." {
            return None;
        }
        relative.push(segment);
    }
    // Belt and braces: even a surprising segment must stay jailed.
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return None;
        }
    }
    let root = out_dir.canonicalize().ok()?;
    let mut candidate = root.join(&relative);
    if candidate.is_dir() {
        // Directory routes resolve to their index, with or without a
        // trailing slash (`/posts/example` serves the directory index when
        // one exists).
        candidate.push("index.html");
    }
    if !candidate.is_file() {
        return None;
    }
    // Final containment: symlinks inside the output tree could otherwise
    // point the canonical path elsewhere.
    let canonical = candidate.canonicalize().ok()?;
    if !canonical.starts_with(&root) {
        return None;
    }
    Some(canonical)
}

/// Desired watch roots for one site: every input that can affect
/// generation. Mirrors ingestion: `signal.toml`, each collection's source
/// directory (or the `content/` fallback), `templates/`, `static/`, and
/// `.git` only when `[git] last_modified` is enabled.
///
/// Roots that do not exist yet are still returned: the caller falls back
/// to watching their parent so creation is noticed, then re-syncs.
pub(crate) fn watch_roots_for(
    root: &Path,
    config: Option<&signal_core::SignalConfig>,
) -> Vec<PathBuf> {
    let mut roots = vec![root.join("signal.toml")];
    match config {
        None => {
            roots.push(root.join("content"));
        }
        Some(config) => {
            if config.collections.is_empty() {
                roots.push(root.join("content"));
            } else {
                let mut names: Vec<&String> = config.collections.keys().collect();
                names.sort();
                for name in names {
                    roots.push(root.join(config.source_dir_for(name)));
                }
            }
            if config.git_last_modified() {
                roots.push(root.join(".git"));
            }
        }
    }
    roots.push(root.join("templates"));
    roots.push(root.join("static"));
    roots
}

/// Whether a watcher event path should trigger a rebuild.
///
/// The configured output directory never triggers (its writes are Signal's
/// own: artifacts, `.signal/manifest.json`, transient manifest temps, and
/// alias-validation probes). Everything else under a watch root does.
pub(crate) fn event_should_trigger(out_canonical: &Path, event_path: &Path) -> bool {
    if event_path == out_canonical || event_path.starts_with(out_canonical) {
        return false;
    }
    true
}

/// Outcome of one rebuild plus whether an HTTP server is already running.
fn rebuild_and_report(root: &Path, out: &Path) -> bool {
    match crate::build::build_site_from_disk(root, out) {
        Ok(summary) => {
            print_summary(&summary);
            true
        }
        Err(err) => {
            // Last-known-good keeps serving; the next successful build
            // reconciles through the existing BuildPlan.
            eprintln!("Rebuild failed: {err}");
            false
        }
    }
}

fn print_summary(summary: &BuildSummary) {
    println!(
        "Build complete: {} artifacts ({} reused, {} rebuilt, {} pruned)",
        summary.specs.len(),
        summary.reused,
        summary.rebuilt,
        summary.pruned
    );
    flush_stdout();
}

/// Stdout is block-buffered when piped (supervisors, tests): flush after
/// every status line so observers see rebuilds as they happen.
fn flush_stdout() {
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
}

/// Serve one HTTP request against the output directory.
fn handle_request(request: tiny_http::Request, out_dir: &Path) {
    let url = request.url().to_string();
    let method = request.method().clone();
    if !matches!(method, tiny_http::Method::Get | tiny_http::Method::Head) {
        let _ = request
            .respond(tiny_http::Response::from_string("Method not allowed").with_status_code(405));
        return;
    }
    let head_only = matches!(method, tiny_http::Method::Head);
    match resolve_request_path(out_dir, &url) {
        Some(path) => respond_file(request, &path, 200, head_only),
        None => {
            // Themed 404 when the site provides one, plain 404 otherwise.
            let not_found = out_dir.join("404.html");
            if not_found.is_file() {
                respond_file(request, &not_found, 404, head_only);
            } else {
                let _ = request
                    .respond(tiny_http::Response::from_string("Not found").with_status_code(404));
            }
        }
    }
}

fn respond_file(request: tiny_http::Request, path: &Path, status: u16, head_only: bool) {
    let content_type = content_type_for(path).to_string();
    let result = (|| -> std::io::Result<tiny_http::ResponseBox> {
        let file = File::open(path)?;
        let len = file.metadata()?.len().to_string();
        // HEAD carries the GET headers (including length) with an empty
        // body; GET streams the file.
        let mut response: tiny_http::ResponseBox = if head_only {
            tiny_http::Response::from_data(Vec::<u8>::new()).boxed()
        } else {
            tiny_http::Response::from_file(file).boxed()
        };
        response = response.with_status_code(status);
        response.add_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                .expect("static header name/value always valid"),
        );
        response.add_header(
            tiny_http::Header::from_bytes(&b"Content-Length"[..], len.as_bytes())
                .expect("static header name always valid"),
        );
        Ok(response)
    })();
    match result {
        Ok(response) => {
            let _ = request.respond(response);
        }
        Err(_) => {
            let _ = request
                .respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        }
    }
}

/// Run the development server until the process is terminated.
///
/// Performs the initial [`crate::build::build_site_from_disk`], starts
/// serving once a build succeeds, then rebuilds on coalesced source
/// changes. Failed rebuilds are reported and the loop continues serving
/// the current output. Returns only on fatal setup errors (unbindable
/// address, unwarchable root); rebuild errors never propagate.
pub fn serve_forever(options: &ServeOptions) -> Result<(), BuildError> {
    println!("Building site...");
    flush_stdout();
    let mut serving = rebuild_and_report(&options.root, &options.out);
    if !serving {
        // A previous manual build may exist in the output directory, but
        // this process never validated it: serve only what this process
        // successfully builds. Keep watching; the fix itself is an event.
        eprintln!("No usable initial build yet; watching for fixes.");
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher =
        notify::RecommendedWatcher::new(tx, Config::default()).map_err(|e| BuildError::Write {
            path: options.root.display().to_string(),
            message: format!("could not start file watcher: {e}"),
        })?;
    // Roots depend on configuration, which may itself be broken or change:
    // start from best effort (defaults when unparseable) and re-sync after
    // every rebuild. The set persists across syncs so paths are never
    // watched twice.
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    sync_watches_into(
        &mut watcher,
        &mut watched,
        &options.root,
        load_config_best_effort(&options.root).as_ref(),
    );

    let mut out_canonical = options
        .out
        .canonicalize()
        .unwrap_or_else(|_| options.out.clone());
    let mut server_handle: Option<std::thread::JoinHandle<()>> = None;
    let start_server =
        |handle: &mut Option<std::thread::JoinHandle<()>>, host: &str, port: u16, out: &Path| {
            if handle.is_some() {
                return true;
            }
            let address = format!("{host}:{port}");
            match tiny_http::Server::http(&address) {
                Ok(server) => {
                    let bound = server.server_addr().to_string();
                    println!("Serving http://{bound}/");
                    println!("Watching {}", options_root_display(options));
                    flush_stdout();
                    let out = out.to_path_buf();
                    *handle = Some(std::thread::spawn(move || {
                        for request in server.incoming_requests() {
                            handle_request(request, &out);
                        }
                    }));
                    true
                }
                Err(err) => {
                    eprintln!("Could not bind {address}: {err}");
                    false
                }
            }
        };

    if serving
        && !start_server(
            &mut server_handle,
            &options.host,
            options.port,
            &options.out,
        )
    {
        return Err(BuildError::Write {
            path: format!("{}:{}", options.host, options.port),
            message: "could not bind HTTP server".to_string(),
        });
    }

    loop {
        // Block for the first relevant event, then drain the quiet window
        // so one logical save yields one rebuild. Events under the output
        // directory are Signal's own writes and never trigger.
        loop {
            match rx.recv() {
                Ok(Ok(event)) => {
                    if event.paths.iter().any(|p| {
                        event_should_trigger(&out_canonical, p)
                            || top_level_input_appeared(&options.root, p)
                    }) {
                        break;
                    }
                }
                Ok(Err(err)) => eprintln!("Watcher error: {err}"),
                Err(_) => {
                    return Err(BuildError::Write {
                        path: options.root.display().to_string(),
                        message: "file watcher disconnected".to_string(),
                    });
                }
            }
        }
        loop {
            match rx.recv_timeout(options.debounce) {
                Ok(Ok(_)) => continue,
                Ok(Err(err)) => {
                    eprintln!("Watcher error: {err}");
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(BuildError::Write {
                        path: options.root.display().to_string(),
                        message: "file watcher disconnected".to_string(),
                    });
                }
            }
        }
        // Refresh watch roots: the config edit that triggered this rebuild
        // may have added, removed, or renamed inputs (or toggled git dates).
        let config = load_config_best_effort(&options.root);
        sync_watches_into(&mut watcher, &mut watched, &options.root, config.as_ref());
        if rebuild_and_report(&options.root, &options.out) {
            // Refresh the exclusion root: the output directory may have
            // been created by the build that just succeeded.
            out_canonical = options
                .out
                .canonicalize()
                .unwrap_or_else(|_| options.out.clone());
            if !serving {
                serving = true;
                if !start_server(
                    &mut server_handle,
                    &options.host,
                    options.port,
                    &options.out,
                ) {
                    return Err(BuildError::Write {
                        path: format!("{}:{}", options.host, options.port),
                        message: "could not bind HTTP server".to_string(),
                    });
                }
            }
        }
    }
}

fn options_root_display(options: &ServeOptions) -> String {
    options.root.display().to_string()
}

/// Best-effort config load for watcher roots: a broken `signal.toml`
/// must not stop watching (the fix itself is an event).
fn load_config_best_effort(root: &Path) -> Option<signal_core::SignalConfig> {
    let text = std::fs::read_to_string(root.join("signal.toml")).ok()?;
    toml::from_str(&text).ok()
}

/// A top-level path appearing under the site root that matches a watched
/// input name (missing dir created, e.g. `templates/`): re-sync watches.
fn top_level_input_appeared(root: &Path, event_path: &Path) -> bool {
    if event_path.parent() != Some(root) {
        return false;
    }
    matches!(
        event_path.file_name().and_then(|name| name.to_str()),
        Some("signal.toml" | "content" | "templates" | "static" | ".git")
    )
}

/// Sync subscriptions, tracking currently watched paths to avoid duplicates.
fn sync_watches_into(
    watcher: &mut notify::RecommendedWatcher,
    watched: &mut BTreeSet<PathBuf>,
    root: &Path,
    config: Option<&signal_core::SignalConfig>,
) {
    let mut desired: BTreeSet<(PathBuf, RecursiveMode)> = BTreeSet::new();
    for watch_root in watch_roots_for(root, config) {
        if watch_root.is_dir() {
            desired.insert((watch_root, RecursiveMode::Recursive));
        } else if watch_root.is_file() || watch_root.ends_with("signal.toml") {
            // Watch the file itself when present; otherwise its parent so
            // creation is noticed. `signal.toml` may also be missing.
            if watch_root.is_file() {
                desired.insert((watch_root, RecursiveMode::NonRecursive));
            } else if let Some(parent) = watch_root.parent() {
                desired.insert((parent.to_path_buf(), RecursiveMode::NonRecursive));
            }
        } else if let Some(parent) = watch_root.parent() {
            if parent.is_dir() {
                desired.insert((parent.to_path_buf(), RecursiveMode::NonRecursive));
            }
        }
    }
    // Never watch the output tree via a parent fallback: if the output
    // directory itself is missing, watching the site root non-recursively
    // is still safe because event paths under it are filtered.
    let desired_paths: BTreeSet<PathBuf> = desired.iter().map(|(path, _)| path.clone()).collect();
    for watched_path in watched.clone() {
        if !desired_paths.contains(&watched_path) {
            let _ = watcher.unwatch(&watched_path);
            watched.remove(&watched_path);
        }
    }
    for (path, mode) in desired {
        if watched.insert(path.clone()) {
            if let Err(err) = watcher.watch(&path, mode) {
                eprintln!("Could not watch {}: {err}", path.display());
                watched.remove(&path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    "[site]\ntitle = \"Serve\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[feed]\n",
                ),
                (
                    "content/posts/alpha.md",
                    "---\ntitle: Alpha\ndate: 2026-02-01\n---\n\nAlpha body words here.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                ("static/app.js", "console.log(1);"),
                ("static/style.css", "body { color: red; }"),
            ],
        );
    }

    #[test]
    fn content_types_cover_served_kinds() {
        let cases = [
            ("a.html", "text/html; charset=utf-8"),
            ("a.css", "text/css; charset=utf-8"),
            ("a.js", "text/javascript; charset=utf-8"),
            ("a.json", "application/json"),
            ("a.xml", "application/xml"),
            ("a.txt", "text/plain; charset=utf-8"),
            ("a.svg", "image/svg+xml"),
            ("a.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.woff2", "font/woff2"),
            ("a.bin", "application/octet-stream"),
            ("noext", "application/octet-stream"),
        ];
        for (name, expected) in cases {
            assert_eq!(content_type_for(Path::new(name)), expected, "{name}");
        }
    }

    #[test]
    fn request_paths_resolve_like_generated_routes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        write_site(
            &out,
            &[
                ("index.html", "home"),
                ("posts/example/index.html", "example"),
                ("404.html", "missing"),
                ("index.xml", "feed"),
            ],
        );
        assert_eq!(
            resolve_request_path(&out, "/"),
            Some(out.join("index.html").canonicalize().expect("canon"))
        );
        assert_eq!(
            resolve_request_path(&out, "/posts/example/"),
            Some(
                out.join("posts/example/index.html")
                    .canonicalize()
                    .expect("canon")
            )
        );
        // Extensionless directory route also resolves.
        assert_eq!(
            resolve_request_path(&out, "/posts/example"),
            Some(
                out.join("posts/example/index.html")
                    .canonicalize()
                    .expect("canon")
            )
        );
        assert!(resolve_request_path(&out, "/nope/").is_none());
        // Traversal attempts fail closed, raw or encoded.
        assert!(resolve_request_path(&out, "/../secret").is_none());
        assert!(resolve_request_path(&out, "/..%2Fsecret").is_none());
        assert!(resolve_request_path(&out, "/%2e%2e/secret").is_none());
        assert!(resolve_request_path(&out, "/posts/../../secret").is_none());
        // Query strings do not affect resolution.
        assert_eq!(
            resolve_request_path(&out, "/posts/example/?x=1"),
            Some(
                out.join("posts/example/index.html")
                    .canonicalize()
                    .expect("canon")
            )
        );
    }

    #[test]
    fn output_events_never_trigger() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        std::fs::create_dir_all(out.join(".signal")).expect("mkdir");
        let canonical = out.canonicalize().expect("canon");
        assert!(!event_should_trigger(
            &canonical,
            &canonical.join("posts/alpha/index.html")
        ));
        assert!(!event_should_trigger(
            &canonical,
            &canonical.join(".signal/manifest.json")
        ));
        assert!(!event_should_trigger(
            &canonical,
            &canonical.join(".signal-alias-probe-1-2/index.html")
        ));
        assert!(!event_should_trigger(&canonical, &canonical));
        assert!(event_should_trigger(
            &canonical,
            &dir.path().join("content/posts/alpha.md")
        ));
    }

    #[test]
    fn watch_roots_mirror_ingestion_inputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        fixture(dir.path());
        let text = std::fs::read_to_string(dir.path().join("signal.toml")).expect("config");
        let config: signal_core::SignalConfig = toml::from_str(&text).expect("parses");
        let roots = watch_roots_for(dir.path(), Some(&config));
        for expected in ["signal.toml", "content/posts", "templates", "static"] {
            assert!(roots.contains(&dir.path().join(expected)), "{expected}");
        }
        assert!(!roots.iter().any(|p| p.ends_with(".git")));
        // Git opt-in adds the metadata root.
        let git_text = text + "\n[git]\nlast_modified = true\n";
        let git_config: signal_core::SignalConfig = toml::from_str(&git_text).expect("parses");
        assert!(watch_roots_for(dir.path(), Some(&git_config)).contains(&dir.path().join(".git")));
        // Unparseable config still yields the default roots.
        let fallback = watch_roots_for(dir.path(), None);
        assert!(fallback.contains(&dir.path().join("signal.toml")));
        assert!(fallback.contains(&dir.path().join("content")));
    }

    #[test]
    fn serve_initial_build_matches_build_output() {
        // The serve path builds through build_site_from_disk: identical
        // bytes to `signal build`, verified here at the library boundary
        // (the binary integration test below covers the server itself).
        let dir = tempfile::tempdir().expect("tempdir");
        fixture(dir.path());
        let out = dir.path().join("out");
        let summary = crate::build::build_site_from_disk(dir.path(), &out).expect("builds");
        assert!(!summary.specs.is_empty());
        assert_eq!(
            resolve_request_path(&out, "/posts/alpha/"),
            Some(
                out.join("posts/alpha/index.html")
                    .canonicalize()
                    .expect("canon")
            )
        );
    }
}
