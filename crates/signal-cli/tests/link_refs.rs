//! Integration for structured internal reference validation: valid sites
//! build, broken references fail closed before writes, `signal check`
//! detects without building, and `signal serve` inherits the gate.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
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

fn valid_site(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Links\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[menus.main]\nitems = [\n  { label = \"Posts\", url = \"/posts/\" },\n  { label = \"Ext\", url = \"https://example.org/\" },\n]\n",
            ),
            (
                "content/posts/alpha.md",
                "---\ntitle: Alpha\ndate: 2026-02-01\nimage: images/a.svg\n---\n\n## Install\n\nAlpha body with a [beta link](/posts/beta/) and a [relative link](../beta/) and [external](https://example.com/).\n\n![diagram](/images/a.svg)\n",
            ),
            (
                "content/posts/beta.md",
                "---\ntitle: Beta\ndate: 2026-03-01\n---\n\n## Usage\n\nBeta links to [install step](/posts/alpha/#install) and [local usage](#usage).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
            ("static/images/a.svg", "<svg></svg>\n"),
        ],
    );
}

fn build(dir: &Path, out: &Path) -> std::process::Output {
    Command::new(signal_bin())
        .arg("build")
        .arg("--root")
        .arg(dir)
        .arg("--out")
        .arg(out)
        .output()
        .expect("run build")
}

fn check(dir: &Path) -> std::process::Output {
    Command::new(signal_bin())
        .arg("check")
        .arg("--root")
        .arg(dir)
        .output()
        .expect("run check")
}

fn snapshot_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
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
fn valid_references_build_and_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");
    let output = build(dir.path(), &out);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.join("posts/alpha/index.html").exists());
    assert!(out.join("posts/beta/index.html").exists());

    let output = check(dir.path());
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("references: "), "got:\n{stdout}");
}

#[test]
fn broken_route_fails_closed_before_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");
    assert!(build(dir.path(), &out).status.success());
    let before = snapshot_tree(&out);

    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta links to [ghost](/projects/foo/).\n",
    )
    .expect("break");
    let output = build(dir.path(), &out);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("broken internal reference"),
        "got:\n{stderr}"
    );
    assert!(stderr.contains("/projects/foo/"), "got:\n{stderr}");
    // Fail closed: no generated output modified, manifest unchanged.
    assert_eq!(snapshot_tree(&out), before);

    // `signal check` detects the same reference without writing anything.
    let check_out = dir.path().join("check-out");
    let output = check(dir.path());
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("/projects/foo/"),
        "got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!check_out.exists());
    let _ = check_out;
}

#[test]
fn broken_asset_references_fail_closed() {
    for (path, content) in [
        (
            "content/posts/alpha.md",
            "---\ntitle: Alpha\ndate: 2026-02-01\n---\n\nAlpha with a ![missing](/images/gone.svg).\n",
        ),
        (
            "content/posts/alpha.md",
            "---\ntitle: Alpha\ndate: 2026-02-01\nimage: images/gone.svg\n---\n\nAlpha body.\n",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        valid_site(dir.path());
        let out = dir.path().join("out");
        assert!(build(dir.path(), &out).status.success());
        let before = snapshot_tree(&out);
        std::fs::write(dir.path().join(path), content).expect("break");
        let output = build(dir.path(), &out);
        assert!(!output.status.success(), "{path}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("broken internal reference"),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(snapshot_tree(&out), before, "{path}");
    }
}

#[test]
fn broken_fragment_fails_with_fragment_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");
    assert!(build(dir.path(), &out).status.success());

    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta links to [nope](/posts/alpha/#missing).\n",
    )
    .expect("break");
    let output = build(dir.path(), &out);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(stderr.contains("fragment does not exist"), "got:\n{stderr}");
    assert!(stderr.contains("#missing"), "got:\n{stderr}");
}

/// `signal serve` inherits the gate through `build_site_from_disk`: an
/// invalid edit reports the validation error, keeps serving, and recovers.
#[test]
fn serve_inherits_reference_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    valid_site(dir.path());
    let out = dir.path().join("out");

    let mut child = Command::new(signal_bin())
        .arg("serve")
        .arg("--root")
        .arg(dir.path())
        .arg("--out")
        .arg(&out)
        .arg("--port")
        .arg("0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");
    let (tx, rx) = mpsc::channel();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stream.expect("piped"));
            for line in reader.lines() {
                if tx.send(line.expect("line")).is_err() {
                    break;
                }
            }
        });
    }
    let wait_for = |needle: &str| -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::ZERO);
            if remaining.is_zero() {
                panic!("timed out waiting for {needle:?}");
            }
            match rx.recv_timeout(remaining) {
                Ok(line) if line.contains(needle) => return line,
                Ok(_) => {}
                Err(_) => panic!("output ended waiting for {needle:?}"),
            }
        }
    };
    let serving = wait_for("Serving http://");
    // "Serving http://127.0.0.1:57934/"
    let port: u16 = serving
        .rsplit(':')
        .next()
        .expect("port")
        .trim_end_matches('/')
        .parse()
        .expect("numeric port");
    let fetch = |path: &str| http_get(port, path);

    let (status, body) = fetch("/posts/beta/");
    assert_eq!(status, 200);
    assert!(body.contains("Beta links to"));

    // Break a reference: validation error, previous output still served.
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta links to [ghost](/projects/foo/).\n",
    )
    .expect("break");
    wait_for("broken internal reference");
    assert!(child.try_wait().expect("status").is_none());
    let (status, body) = fetch("/posts/beta/");
    assert_eq!(status, 200);
    assert!(body.contains("Beta links to"));

    // Fix recovers on the next rebuild.
    std::fs::write(
        dir.path().join("content/posts/beta.md"),
        "---\ntitle: Beta\ndate: 2026-03-01\n---\n\nBeta fixed body here.\n",
    )
    .expect("fix");
    wait_for("Build complete");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (status, body) = fetch("/posts/beta/");
        assert_eq!(status, 200);
        if body.contains("Beta fixed body here.") {
            break;
        }
        if Instant::now() >= deadline {
            panic!("recovered output never served");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("split");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .expect("status")
        .split_whitespace()
        .nth(1)
        .expect("code")
        .parse()
        .expect("numeric");
    (
        status,
        String::from_utf8_lossy(&raw[split + 4..]).to_string(),
    )
}
