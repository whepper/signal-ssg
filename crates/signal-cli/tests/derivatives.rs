//! Image derivatives (A2, ADR 0029): cross-crate integration tests.
//!
//! Planning, incremental rebuilds, validation agreement, `check`,
//! `explain`, collisions, and unsupported sources — all against
//! runtime-generated raster fixtures (no binary blobs).

use image::ImageEncoder as _;
use signal_cli::{
    build::build_site_from_disk, explain::explain_derivative_from_disk,
    link_check::check_site_from_disk, manifest::InputRef,
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

/// Deterministic W × H RGB gradient PNG, encoded in-test.
fn gradient_png(width: u32, height: u32) -> Vec<u8> {
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

/// Deterministic W × H RGB gradient JPEG, encoded in-test.
fn gradient_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        pixel.0 = [(y % 256) as u8, (x % 256) as u8, ((x * y) % 256) as u8];
    }
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("fixture encodes");
    bytes
}

fn site_with_images(dir: &Path, config_extra: &str) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                &format!(
                    "[site]\ntitle = \"Derivatives\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n{config_extra}"
                ),
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\nimage: images/hero.png\n---\n\nBody with ![alt](/images/hero.png) and ![photo](/images/photo.jpg).\n",
            ),
            (
                "content/posts/plain.md",
                "---\ntitle: Plain\n---\n\nNo images here.\n",
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
    write_bytes(dir, "static/images/hero.png", &gradient_png(160, 90));
    write_bytes(dir, "static/images/photo.jpg", &gradient_jpeg(200, 100));
}

fn out_dir(dir: &Path) -> PathBuf {
    dir.join("out")
}

const IMAGES_CONFIG: &str = "[images]\nwidths = [80, 320]\nformat = \"webp\"\n";

#[test]
fn derivatives_are_planned_resolved_and_valid_webp() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");

    let mut derived: Vec<&str> = summary
        .specs
        .iter()
        .filter(|s| s.kind == signal_core::ArtifactKind::DerivedImage)
        .map(|s| s.path.as_str())
        .collect();
    derived.sort();
    // Two referenced rasters × two widths. The 320w requests exceed both
    // sources (160w, 200w) and clamp instead of upscaling.
    assert_eq!(
        derived,
        vec![
            "images/hero-320.webp",
            "images/hero-80.webp",
            "images/photo-320.webp",
            "images/photo-80.webp",
        ]
    );

    for (path, (width, height)) in [
        ("images/hero-80.webp", (80, 45)),
        ("images/hero-320.webp", (160, 90)),
        ("images/photo-80.webp", (80, 40)),
        ("images/photo-320.webp", (200, 100)),
    ] {
        let bytes = std::fs::read(out.join(path)).expect("derivative output");
        assert!(bytes.starts_with(b"RIFF"), "{path} is WebP");
        assert_eq!(&bytes[8..12], b"WEBP", "{path} is WebP");
        let back = image::load_from_memory(&bytes).expect("webp decodes");
        assert_eq!((back.width(), back.height()), (width, height), "{path}");
    }

    // The manifest distinguishes source digests, derivative inputs, and
    // derivative output digests (never page HTML digests).
    let manifest = signal_cli::manifest::parse_manifest(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("parses");
    let record = manifest
        .artifacts
        .get("images/hero-80.webp")
        .expect("derivative record");
    assert_eq!(record.kind, signal_core::ArtifactKind::DerivedImage);
    assert_eq!(
        record.inputs,
        vec![InputRef::DerivedImage {
            source: "images/hero.png".to_string(),
            width: 80,
            format: "webp".to_string(),
        }]
    );
    assert!(manifest.assets.contains_key("images/hero.png"));
    let source_digest = manifest
        .assets
        .get("images/hero.png")
        .expect("source digest");
    assert_ne!(
        &record.output_digest, source_digest,
        "derivative output must differ from its source"
    );
}

#[test]
fn unconfigured_sites_plan_no_derivatives() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), "");
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    assert!(
        summary
            .specs
            .iter()
            .all(|s| s.kind != signal_core::ArtifactKind::DerivedImage),
        "no [images] table means no derivatives"
    );
}

#[test]
fn non_raster_references_produce_no_derivatives() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Derivatives\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\nimage: images/logo.svg\n---\n\nBody ![alt](/images/logo.svg).\n",
            ),
            (
                "templates/post.html",
                "<html><body>{{ content | safe }}</body></html>",
            ),
            (
                "templates/section.html",
                "<html><body>section</body></html>",
            ),
            ("static/images/logo.svg", "<svg></svg>"),
        ],
    );
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    assert!(
        summary
            .specs
            .iter()
            .all(|s| s.kind != signal_core::ArtifactKind::DerivedImage),
        "SVG is never rasterized"
    );
    assert!(out.join("images/logo.svg").exists());
}

#[test]
fn incremental_derivative_lifecycle() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    let out = out_dir(dir.path());

    // Initial build: everything rebuilds.
    let first = build_site_from_disk(dir.path(), &out).expect("first builds");
    assert!(first
        .rebuilt_paths
        .contains(&"images/hero-80.webp".to_string()));

    // Second build: full reuse, derivatives included.
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(text.contains("rebuild:   0\n"), "got:\n{text}");

    // Source change: the source, its derivatives, and the consuming page
    // rebuild; the imageless page reuses.
    std::fs::write(
        dir.path().join("static/images/hero.png"),
        gradient_png(120, 60),
    )
    .expect("rewrite");
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    for path in [
        "images/hero.png",
        "images/hero-80.webp",
        "images/hero-320.webp",
        "posts/example/index.html",
    ] {
        assert!(
            text.contains(&format!("  {path}\n    reason: ")),
            "got:\n{text}"
        );
    }
    let reuse = text.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  posts/plain/index.html\n"), "got:\n{text}");
    assert!(reuse.contains("  images/photo-80.webp\n"), "got:\n{text}");
    build_site_from_disk(dir.path(), &out).expect("rebuilds");

    // Width change: a new derivative appears (missing record), the page
    // inputs change, and the dropped width goes stale.
    write_site(
        dir.path(),
        &[(
            "signal.toml",
            "[site]\ntitle = \"Derivatives\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80]\nformat = \"webp\"\n",
        )],
    );
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    // The surviving derivative's triple is unchanged, so it reuses…
    let reuse = text.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  images/hero-80.webp\n"), "got:\n{text}");
    // …while the page's input list lost the 320w edges.
    assert!(
        text.contains("  posts/example/index.html\n    reason: inputs changed\n"),
        "got:\n{text}"
    );
    assert!(
        text.contains("Prune:\n  images/hero-320.webp\n") || text.contains("images/hero-320.webp"),
        "dropped widths prune: got:\n{text}"
    );
    let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
    assert!(!out.join("images/hero-320.webp").exists());
    assert!(summary
        .pruned_paths
        .contains(&"images/hero-320.webp".to_string()));

    // Unrelated content change: derivatives reuse.
    write_site(
        dir.path(),
        &[(
            "content/posts/plain.md",
            "---\ntitle: Plain changed\n---\n\nNo images here.\n",
        )],
    );
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    let reuse = text.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  images/hero-80.webp\n"), "got:\n{text}");
    assert!(reuse.contains("  images/photo-80.webp\n"), "got:\n{text}");
}

#[test]
fn invalid_derivative_config_fails_build_and_check_identically() {
    for (name, config_extra) in [
        ("bad-format", "[images]\nwidths = [640]\nformat = \"jxl\"\n"),
        (
            "bad-plural-format",
            "[images]\nwidths = [640]\nformats = [\"webp\", \"jxl\"]\n",
        ),
        (
            "both-format-keys",
            "[images]\nwidths = [640]\nformat = \"webp\"\nformats = [\"avif\"]\n",
        ),
        ("zero-width", "[images]\nwidths = [0]\n"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        site_with_images(dir.path(), config_extra);
        let out = out_dir(dir.path());
        for result in [
            build_site_from_disk(dir.path(), &out).map(|_| ()),
            check_site_from_disk(dir.path()).map(|_| ()),
            signal_cli::explain::explain_site_from_disk(dir.path(), &out).map(|_| ()),
        ] {
            let err = result.expect_err(&format!("{name} must fail"));
            assert!(err.to_string().contains("[images]"), "{name}: got: {err}");
        }
    }
}

#[test]
fn malformed_source_fails_build_and_check_identically() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    write_bytes(
        dir.path(),
        "static/images/hero.png",
        b"not actually png bytes",
    );
    let out = out_dir(dir.path());
    for result in [
        build_site_from_disk(dir.path(), &out).map(|_| ()),
        check_site_from_disk(dir.path()).map(|_| ()),
        signal_cli::explain::explain_site_from_disk(dir.path(), &out).map(|_| ()),
    ] {
        let err = result.expect_err("malformed source must fail");
        assert!(
            err.to_string().contains("hero.png"),
            "diagnostic must name the source: {err}"
        );
    }
    assert!(!out.join("images/hero-80.webp").exists());
}

#[test]
fn derivative_output_collisions_fail_before_writes() {
    // A committed static shadowing a derivative output.
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    write_bytes(dir.path(), "static/images/hero-80.webp", b"squat");
    let out = out_dir(dir.path());
    let err = build_site_from_disk(dir.path(), &out).expect_err("must collide");
    assert!(err.to_string().contains("collision"), "got: {err}");

    // Two sources (PNG + JPEG) mapping to one derivative output.
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Derivatives\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: Example\n---\n\n![a](/images/dupe.png) ![b](/images/dupe.jpg).\n",
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
    write_bytes(dir.path(), "static/images/dupe.png", &gradient_png(160, 90));
    write_bytes(
        dir.path(),
        "static/images/dupe.jpg",
        &gradient_jpeg(160, 90),
    );
    let out = out_dir(dir.path());
    let err = build_site_from_disk(dir.path(), &out).expect_err("must collide");
    assert!(err.to_string().contains("collision"), "got: {err}");
}

#[test]
fn check_reports_planned_derivatives() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    let report = check_site_from_disk(dir.path()).expect("check passes");
    assert_eq!(report.assets.derivatives, 4);
    assert_eq!(report.assets.missing, 0);
    assert_eq!(report.assets.unsafe_paths, 0);
}

#[test]
fn explain_derivative_matches_build_decision() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    // By source path with flags…
    let text = explain_derivative_from_disk(dir.path(), &out, "images/hero.png", 80, None)
        .expect("explains");
    assert!(
        text.contains("Source:\n  static/images/hero.png\n"),
        "got:\n{text}"
    );
    assert!(text.contains("Input:\n  160 × 90\n  PNG\n"), "got:\n{text}");
    assert!(
        text.contains("Derivative:\n  width: 80\n  height: 45\n  format: webp\n"),
        "got:\n{text}"
    );
    assert!(
        text.contains("Output:\n  images/hero-80.webp\n"),
        "got:\n{text}"
    );
    assert!(
        text.contains("Dependencies:\n  source: images/hero.png\n  page: /posts/example/\n"),
        "got:\n{text}"
    );
    assert!(text.contains("Action:\n  generate\n"), "got:\n{text}");
    assert!(text.contains("Decision:\n  reuse\n"), "got:\n{text}");
    // …and deterministically.
    let again = explain_derivative_from_disk(dir.path(), &out, "images/hero.png", 80, None)
        .expect("explains");
    assert_eq!(text, again);

    // Bare derivative output paths explain without flags.
    let via_output =
        signal_cli::explain::explain_asset_from_disk(dir.path(), &out, "images/hero-80.webp")
            .expect("explains output path");
    assert_eq!(via_output, text);

    // Clamped widths report source dimensions, never upscaled ones.
    let clamped = explain_derivative_from_disk(dir.path(), &out, "images/hero.png", 320, None)
        .expect("explains");
    assert!(
        clamped.contains("Derivative:\n  width: 160\n  height: 90\n  format: webp\n"),
        "got:\n{clamped}"
    );

    // Unplanned requests fail with the configured widths named.
    let err = explain_derivative_from_disk(dir.path(), &out, "images/hero.png", 1234, None)
        .expect_err("must fail");
    assert!(err.to_string().contains("not planned"), "got: {err}");

    // After touching the source, explain agrees with the build: rebuild.
    std::fs::write(
        dir.path().join("static/images/hero.png"),
        gradient_png(120, 60),
    )
    .expect("rewrite");
    let text = explain_derivative_from_disk(dir.path(), &out, "images/hero.png", 80, None)
        .expect("explains");
    assert!(
        text.contains("Decision:\n  rebuild: derivative source changed: images/hero.png\n"),
        "got:\n{text}"
    );
}

#[test]
fn explain_asset_lists_planned_derivatives() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_images(dir.path(), IMAGES_CONFIG);
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let text = signal_cli::explain::explain_asset_from_disk(dir.path(), &out, "images/hero.png")
        .expect("explains");
    assert!(
        text.contains("Derivatives:\n  images/hero-320.webp (320w → 160 × 90)\n  images/hero-80.webp (80w → 80 × 45)\n"),
        "got:\n{text}"
    );
}
