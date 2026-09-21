//! Asset and image diagnostics (A6): cross-crate integration tests.
//!
//! These exercise the shared analysis in `signal_cli::diagnostics` through
//! both surfaces that render it — `signal check` and `signal explain` — and
//! pin the architectural rules: diagnostics are advisory, never artifacts,
//! never in the manifest, and never a substitute for hard validation.
//!
//! Fixture images are generated deterministically in-test (no binary blobs):
//! `gradient_png`/`gradient_jpeg` produce identical bytes on every run.

use signal_cli::diagnostics::{Diagnostic, Severity};
use signal_cli::{
    build::build_site_from_disk, explain::explain_asset_from_disk, link_check::check_site_from_disk,
};
use std::path::{Path, PathBuf};

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

fn write_bytes(dir: &Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

fn gradient_png(width: u32, height: u32) -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        pixel.0 = [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8];
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("fixture encodes");
    bytes
}

fn gradient_jpeg(width: u32, height: u32) -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        pixel.0 = [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8];
    }
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("fixture encodes");
    bytes
}

/// One site covering every diagnostic condition. `[images]` widths are
/// `[640, 1280]`, so:
///
/// - `oversized.png` (4000×3000) is ≥ 3× the largest candidate (1280);
/// - `redundant.png` (400×300) clamps both widths to 400;
/// - `efficient.png` (1200×900) is neither;
/// - `orphan.png` is referenced by nothing;
/// - `missing-alt.md` declares a hero with no `image_alt`;
/// - `decorative-alt.md` uses `![](…)`, the intentional decorative marker.
///
/// `css/site.css`, `js/app.js`, and `favicon.svg` are referenced only from
/// the template (invisible to the model) and must not be reported.
fn diagnostics_site(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Diagnostics\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [640, 1280]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/good.md",
                "---\ntitle: Good\nimage: images/efficient.png\nimage_alt: Efficient hero\n---\n\nA described ![inline image](/images/efficient.png) here and a ![JPEG image](/images/photo.jpg) too.\n",
            ),
            (
                "content/posts/big.md",
                "---\ntitle: Big\nimage: images/oversized.png\nimage_alt: Big hero\n---\n\nBig ![Big hero](/images/oversized.png) here.\n",
            ),
            (
                "content/posts/clamped.md",
                "---\ntitle: Clamped\n---\n\nSmall ![Clamped](/images/redundant.png) here.\n",
            ),
            (
                "content/posts/missing-alt.md",
                "---\ntitle: Missing alt\nimage: images/efficient.png\n---\n\nNo hero alt here.\n",
            ),
            (
                "content/posts/decorative-alt.md",
                "---\ntitle: Decorative alt\n---\n\nA rule ![](/images/efficient.png) here.\n",
            ),
            (
                "templates/post.html",
                "<html><head><link rel=\"stylesheet\" href=\"/css/site.css\" /><link rel=\"icon\" href=\"/favicon.svg\" /></head><body>{{ content | safe }}<script src=\"/js/app.js\"></script></body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
            ("static/css/site.css", "body{}"),
            ("static/js/app.js", "console.log(1);"),
            ("static/favicon.svg", "<svg></svg>"),
        ],
    );
    write_bytes(dir, "static/images/efficient.png", &gradient_png(1200, 900));
    write_bytes(
        dir,
        "static/images/oversized.png",
        &gradient_png(4000, 3000),
    );
    write_bytes(dir, "static/images/redundant.png", &gradient_png(400, 300));
    write_bytes(dir, "static/images/orphan.png", &gradient_png(300, 200));
    // A referenced JPEG keeps the JPEG path covered by the fixture too.
    write_bytes(dir, "static/images/photo.jpg", &gradient_jpeg(1200, 800));
}

fn out_dir(dir: &Path) -> PathBuf {
    dir.join("out")
}

#[test]
fn check_reports_the_expected_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let report = check_site_from_disk(dir.path()).expect("checks");
    let mut reported: Vec<(&str, &str, &str)> = report
        .diagnostics
        .iter()
        .map(|d| (d.code(), d.subject(), d.severity().as_str()))
        .collect();
    reported.sort_unstable();
    assert_eq!(
        reported,
        vec![
            ("hero-alt-missing", "/posts/missing-alt/", "warning"),
            ("image-alt-empty", "/posts/decorative-alt/", "info"),
            ("oversized-source", "images/oversized.png", "warning"),
            ("redundant-derivative-width", "images/redundant.png", "info"),
            ("unreferenced-asset", "images/orphan.png", "info"),
        ]
    );
    // Warnings sort before info; subjects sort within a severity.
    let severities: Vec<Severity> = report
        .diagnostics
        .iter()
        .map(Diagnostic::severity)
        .collect();
    let mut sorted = severities.clone();
    sorted.sort();
    assert_eq!(severities, sorted);
    assert_eq!(
        signal_cli::diagnostics::count(&report.diagnostics, Severity::Warning),
        2
    );
    assert_eq!(
        signal_cli::diagnostics::count(&report.diagnostics, Severity::Info),
        3
    );
}

#[test]
fn oversized_source_reports_measured_dimensions() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let report = check_site_from_disk(dir.path()).expect("checks");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code() == "oversized-source")
        .expect("oversized source reported");
    // Evidence is the measured relationship, not advice.
    assert_eq!(
        diagnostic.message(),
        "source is 4000×3000; the largest generated representation is 1280px wide"
    );
    assert_eq!(diagnostic.subject(), "images/oversized.png");
}

#[test]
fn redundant_widths_report_the_collapsing_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let report = check_site_from_disk(dir.path()).expect("checks");
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code() == "redundant-derivative-width")
        .expect("redundant widths reported");
    assert_eq!(
        diagnostic.message(),
        "requested widths 640 and 1280 render at the same 400px"
    );
    assert_eq!(diagnostic.severity(), Severity::Info);
}

#[test]
fn unreferenced_assets_are_raster_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let report = check_site_from_disk(dir.path()).expect("checks");
    let unreferenced: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code() == "unreferenced-asset")
        .map(Diagnostic::subject)
        .collect();
    // Template-referenced CSS/JS/SVG are not modeled and not reported.
    assert_eq!(unreferenced, vec!["images/orphan.png"]);
}

#[test]
fn empty_alt_text_is_informational_not_a_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let report = check_site_from_disk(dir.path()).expect("checks");
    let decorative: Vec<&Diagnostic> = report
        .diagnostics
        .iter()
        .filter(|d| d.subject() == "/posts/decorative-alt/")
        .collect();
    let decorative_codes: Vec<&str> = decorative.iter().map(|d| d.code()).collect();
    assert_eq!(decorative_codes, vec!["image-alt-empty"]);
    // An intentional decorative image is never a warning or a failure.
    assert_eq!(decorative[0].severity(), Severity::Info);
    // A described hero and described body image produce nothing.
    assert!(report
        .diagnostics
        .iter()
        .all(|d| d.subject() != "/posts/good/"));
}

#[test]
fn no_derivative_configuration_reports_no_image_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Plain\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
            ),
            (
                "content/posts/big.md",
                "---\ntitle: Big\n---\n\nBig ![Big](/images/oversized.png) here.\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    write_bytes(
        dir.path(),
        "static/images/oversized.png",
        &gradient_png(4000, 3000),
    );
    let report = check_site_from_disk(dir.path()).expect("checks");
    // No derivative pipeline means no publishing requirement to compare
    // against: Signal must not invent a maximum display width.
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code() != "oversized-source" && d.code() != "redundant-derivative-width"),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn explain_surfaces_the_subject_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    let oversized = explain_asset_from_disk(dir.path(), &out, "images/oversized.png")
        .expect("explains oversized");
    assert!(
        oversized.contains(
            "\nDiagnostics:\n  warning: source is 4000×3000; the largest generated representation is 1280px wide\n"
        ),
        "got:\n{oversized}"
    );

    // An unreferenced asset reports its own diagnostic.
    let orphan =
        explain_asset_from_disk(dir.path(), &out, "images/orphan.png").expect("explains orphan");
    assert!(
        orphan.contains("\nDiagnostics:\n  info: no content entry references this asset\n"),
        "got:\n{orphan}"
    );

    // A clean asset renders exactly as before A6: no Diagnostics section.
    let clean =
        explain_asset_from_disk(dir.path(), &out, "images/efficient.png").expect("explains clean");
    assert!(!clean.contains("Diagnostics:"), "got:\n{clean}");
}

#[test]
fn check_and_explain_agree_on_asset_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    let report = check_site_from_disk(dir.path()).expect("checks");
    for subject in [
        "images/oversized.png",
        "images/redundant.png",
        "images/orphan.png",
    ] {
        let from_check: Vec<&Diagnostic> = report
            .diagnostics
            .iter()
            .filter(|d| d.subject() == subject)
            .collect();
        assert!(!from_check.is_empty(), "fixture must diagnose {subject}");
        let from_explain =
            explain_asset_from_disk(dir.path(), &out, subject).expect("explains subject");
        for diagnostic in from_check {
            assert!(
                from_explain.contains(&diagnostic.message()),
                "{subject}: explain must render check's {:?} message, got:\n{from_explain}",
                diagnostic.code()
            );
        }
    }
}

#[test]
fn diagnostics_are_deterministic_and_read_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let first = check_site_from_disk(dir.path()).expect("checks");
    let second = check_site_from_disk(dir.path()).expect("checks");
    assert_eq!(first.diagnostics, second.diagnostics);

    // `check` writes nothing — diagnostics are analysis, not publishing.
    assert!(!out_dir(dir.path()).exists());
}

#[test]
fn diagnostics_do_not_change_output_or_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first build");

    let manifest = std::fs::read(out.join(".signal/manifest.json")).expect("manifest");
    let page = std::fs::read(out.join("posts/big/index.html")).expect("page");

    // Computing diagnostics changes nothing: no artifact, no manifest field.
    let report = check_site_from_disk(dir.path()).expect("checks");
    assert!(!report.diagnostics.is_empty());
    assert_eq!(
        std::fs::read(out.join(".signal/manifest.json")).expect("manifest"),
        manifest
    );
    assert_eq!(
        std::fs::read(out.join("posts/big/index.html")).expect("page"),
        page
    );

    // A second build reuses everything, exactly as before A6.
    let summary = build_site_from_disk(dir.path(), &out).expect("second build");
    assert_eq!(summary.rebuilt, 0, "diagnostics must not invalidate reuse");
}

#[test]
fn missing_and_unsafe_references_remain_hard_errors() {
    // Diagnostics never soften validation: a missing asset still fails the
    // build and `check`, with no report produced.
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Broken\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n",
            ),
            (
                "content/posts/broken.md",
                "---\ntitle: Broken\n---\n\n![gone](/images/gone.png).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    let out = out_dir(dir.path());
    let err = build_site_from_disk(dir.path(), &out).expect_err("must fail");
    assert!(err.to_string().contains("gone.png"), "got: {err}");
    let err = check_site_from_disk(dir.path()).expect_err("must fail");
    assert!(err.to_string().contains("gone.png"), "got: {err}");

    // Escaping references stay invalid, not diagnosed.
    std::fs::write(
        dir.path().join("content/posts/broken.md"),
        "---\ntitle: Broken\n---\n\n![evil](../../../etc/passwd).\n",
    )
    .expect("edit");
    let err = check_site_from_disk(dir.path()).expect_err("must fail");
    assert!(
        err.to_string().contains("invalid internal reference"),
        "got: {err}"
    );
}

#[test]
fn clean_site_reports_no_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Clean\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [640]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/clean.md",
                "---\ntitle: Clean\nimage: images/hero.png\nimage_alt: A described hero\n---\n\nA described ![image](/images/hero.png) here.\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
        ],
    );
    write_bytes(
        dir.path(),
        "static/images/hero.png",
        &gradient_png(800, 600),
    );
    let report = check_site_from_disk(dir.path()).expect("checks");
    assert!(
        report.diagnostics.is_empty(),
        "clean site must be concise, got: {:?}",
        report.diagnostics
    );
    assert_eq!(report.assets.derivatives, 1);
}

#[test]
fn cli_check_and_explain_render_diagnostics() {
    use std::process::Command;
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_signal"));
    let dir = tempfile::tempdir().expect("tempdir");
    diagnostics_site(dir.path());
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    let check = Command::new(&bin)
        .arg("check")
        .arg("--root")
        .arg(dir.path())
        .output()
        .expect("run signal check");
    assert!(check.status.success());
    let stdout = String::from_utf8(check.stdout).expect("utf8");
    assert!(
        stdout.contains("diagnostics: 2 warnings, 3 info\n"),
        "got:\n{stdout}"
    );
    assert!(
        stdout.contains(
            "  warning: source is 4000×3000; the largest generated representation is 1280px wide\n    images/oversized.png\n"
        ),
        "got:\n{stdout}"
    );

    let explain = Command::new(&bin)
        .arg("explain")
        .arg("--root")
        .arg(dir.path())
        .arg("--out")
        .arg(&out)
        .arg("images/oversized.png")
        .output()
        .expect("run signal explain");
    assert!(explain.status.success());
    let explain_stdout = String::from_utf8(explain.stdout).expect("utf8");
    assert!(
        explain_stdout.contains("\nDiagnostics:\n"),
        "got:\n{explain_stdout}"
    );
    assert!(
        explain_stdout.contains("warning: source is 4000×3000"),
        "got:\n{explain_stdout}"
    );

    // Deterministic CLI output across runs.
    let again = Command::new(&bin)
        .arg("check")
        .arg("--root")
        .arg(dir.path())
        .output()
        .expect("run signal check");
    assert_eq!(String::from_utf8(again.stdout).expect("utf8"), stdout);
}
