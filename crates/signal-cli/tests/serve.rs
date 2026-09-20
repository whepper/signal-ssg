//! CLI integration for `signal serve`: supervised server processes,
//! HTTP behavior, watcher rebuilds, failure survival, and output exclusion.
//!
//! Watcher tests assert eventual state with generous timeouts — never
//! exact event counts or OS-specific ordering.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

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
                "[site]\ntitle = \"Serve Site\"\nbase_url = \"https://example.com/\"\nhome_collection = \"posts\"\nnot_found_template = \"404.html\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[feed]\n",
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
            (
                "templates/home.html",
                "<html><body>home</body></html>",
            ),
            (
                "templates/404.html",
                "<html><body>custom missing</body></html>",
            ),
            ("static/app.js", "console.log(1);"),
            ("static/style.css", "body { color: red; }"),
            ("static/note.txt", "plain text"),
            ("static/pixel.png", "PNG-BYTES"),
        ],
    );
}

/// A running `signal serve` child with its merged output lines.
struct Server {
    child: std::sync::Mutex<Child>,
    lines: Receiver<String>,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Server {
    fn start(dir: &Path, out: &Path) -> Self {
        let mut child = Command::new(signal_bin())
            .arg("serve")
            .arg("--root")
            .arg(dir)
            .arg("--out")
            .arg(out)
            .arg("--port")
            .arg("0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn signal serve");
        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx_out.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut serving_line = None;
        // Drain until the server reports its bound address.
        let mut port = None;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    if line.starts_with("Serving http://") {
                        serving_line = Some(line.clone());
                        port = parse_port(&line);
                        break;
                    }
                }
                Err(_) => {
                    if child_exited(&mut child) {
                        panic!("serve exited before serving");
                    }
                }
            }
        }
        let port = port.unwrap_or_else(|| panic!("no Serving line: {serving_line:?}"));
        // Drain trailing startup lines ("Watching ...") so later
        // assertions only observe rebuild activity.
        let drain_until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < drain_until {
            if rx.recv_timeout(Duration::from_millis(200)).is_err() {
                break;
            }
        }
        // Keep any already-read lines available to later assertions.
        Self {
            child: std::sync::Mutex::new(child),
            lines: rx,
            port,
        }
    }

    /// Wait up to `timeout` for a line containing `needle`, draining others.
    fn wait_for(&self, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::ZERO);
            if remaining.is_zero() {
                panic!("timed out waiting for {needle:?}");
            }
            match self.lines.recv_timeout(remaining) {
                Ok(line) => {
                    if line.contains(needle) {
                        return line;
                    }
                }
                Err(_) => panic!("output ended while waiting for {needle:?}"),
            }
        }
    }

    fn assert_alive(&self) {
        match self
            .child
            .lock()
            .expect("lock")
            .try_wait()
            .expect("wait status")
        {
            None => {}
            Some(status) => panic!("serve exited unexpectedly: {status}"),
        }
    }

    fn request(&self, method: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
        http_request(self.port, method, path)
    }
}

fn child_exited(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(Some(_)))
}

fn parse_port(line: &str) -> Option<u16> {
    // "Serving http://127.0.0.1:57934/"
    line.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

fn http_request(port: u16, method: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    write!(
        stream,
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header/body split");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let body = raw[split + 4..].to_vec();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .expect("status line")
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("numeric status");
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    (status, headers, body)
}

fn header(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn wait_for_file(path: &Path, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if text.contains(needle) {
                return;
            }
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {path:?} to contain {needle:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn serve_help_lists_options() {
    let output = Command::new(signal_bin())
        .arg("serve")
        .arg("--help")
        .output()
        .expect("run help");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("utf8");
    for expected in ["--root", "--out", "--host", "--port"] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
}

#[test]
fn http_root_nested_404_and_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);

    // Root serves the generated index.
    let (status, headers, body) = server.request("GET", "/");
    assert_eq!(status, 200);
    assert!(header(&headers, "content-type").contains("text/html"));
    assert_eq!(body, std::fs::read(out.join("index.html")).expect("index"));

    // Nested route resolves to its index.html, byte-identical to disk.
    let (status, _, body) = server.request("GET", "/posts/alpha/");
    assert_eq!(status, 200);
    assert_eq!(
        body,
        std::fs::read(out.join("posts/alpha/index.html")).expect("alpha")
    );
    assert!(String::from_utf8_lossy(&body).contains("Alpha"));

    // Missing route serves the themed 404 with a 404 status.
    let (status, _, body) = server.request("GET", "/nope/");
    assert_eq!(status, 404);
    assert!(String::from_utf8_lossy(&body).contains("custom missing"));

    // Traversal attempts fail closed and never leak outside the output dir.
    for path in [
        "/../signal.toml",
        "/..%2Fsignal.toml",
        "/%2e%2e/signal.toml",
        "/posts/../../signal.toml",
    ] {
        let (status, _, body) = server.request("GET", path);
        assert_eq!(status, 404, "{path}");
        assert!(
            !String::from_utf8_lossy(&body).contains("Serve Site"),
            "{path}"
        );
    }

    // HEAD mirrors GET headers with an empty body.
    let (status, headers, body) = server.request("HEAD", "/posts/alpha/");
    assert_eq!(status, 200);
    assert!(header(&headers, "content-type").contains("text/html"));
    assert!(!header(&headers, "content-length").is_empty());
    assert!(body.is_empty());
}

#[test]
fn content_types_cover_served_kinds() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);

    let cases = [
        ("/posts/alpha/", "text/html"),
        ("/app.js", "text/javascript"),
        ("/style.css", "text/css"),
        ("/index.json", "application/json"),
        ("/index.xml", "application/xml"),
        ("/note.txt", "text/plain"),
        ("/pixel.png", "image/png"),
    ];
    for (path, expected) in cases {
        let (status, headers, body) = server.request("GET", path);
        assert_eq!(status, 200, "{path}");
        assert!(
            header(&headers, "content-type").contains(expected),
            "{path}: {:?}",
            header(&headers, "content-type")
        );
        assert!(!body.is_empty(), "{path}");
    }
    // Binary asset passes through byte-identical.
    let (_, _, body) = server.request("GET", "/pixel.png");
    assert_eq!(body, b"PNG-BYTES");
}

#[test]
fn file_edit_rebuilds_and_serves_new_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);

    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta body edited here.\n",
    )
    .expect("edit");
    server.wait_for("Build complete", Duration::from_secs(30));
    wait_for_file(
        &out.join("posts/beta/index.html"),
        "Beta body edited here.",
        Duration::from_secs(30),
    );
    let (status, _, body) = server.request("GET", "/posts/beta/");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("Beta body edited here."));
    server.assert_alive();
}

#[test]
fn file_deletion_prunes_served_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);
    assert!(out.join("posts/beta/index.html").exists());

    std::fs::remove_file(dir.path().join("content/posts/beta.md")).expect("delete");
    server.wait_for("Build complete", Duration::from_secs(30));
    let deadline = Instant::now() + Duration::from_secs(30);
    while out.join("posts/beta/index.html").exists() {
        if Instant::now() >= deadline {
            panic!("stale artifact was not pruned");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let (status, _, _) = server.request("GET", "/posts/beta/");
    assert_eq!(status, 404);
    server.assert_alive();
}

#[test]
fn invalid_edit_reports_error_and_keeps_serving() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);

    // Missing title is a pre-write content error: output must be untouched.
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ndate: 2026-03-01\n---\n\nBeta body words here.\n",
    )
    .expect("break");
    server.wait_for("Rebuild failed", Duration::from_secs(30));
    server.assert_alive();
    // Previously generated output remains available.
    let (status, _, body) = server.request("GET", "/posts/beta/");
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("Beta body words here."));
    // Fixing the file recovers on the next rebuild.
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta body fixed here.\n",
    )
    .expect("fix");
    server.wait_for("Build complete", Duration::from_secs(30));
    wait_for_file(
        &out.join("posts/beta/index.html"),
        "Beta body fixed here.",
        Duration::from_secs(30),
    );
}

#[test]
fn output_writes_do_not_trigger_rebuilds() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixture(dir.path());
    // Output nested under the site root: the critical exclusion case.
    let out = dir.path().join("out");
    let server = Server::start(dir.path(), &out);

    // Touching generated output directly must not schedule a rebuild.
    std::fs::write(out.join("probe.txt"), "from outside the build").expect("touch");
    std::thread::sleep(Duration::from_secs(2));
    server.assert_alive();
    // No rebuild line may have appeared since startup.
    match server.lines.try_recv() {
        Ok(line) => panic!("unexpected rebuild activity: {line}"),
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            panic!("serve output ended unexpectedly")
        }
    }
    std::fs::remove_file(out.join("probe.txt")).expect("cleanup");
}
