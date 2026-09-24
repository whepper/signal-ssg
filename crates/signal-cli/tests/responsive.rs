//! Responsive images (A3, ADR 0030; `<picture>`/AVIF since A4,
//! ADR 0031): rendering integration tests.
//!
//! Body-image rewriting, hero contexts, dependency lifecycles (pages and
//! sections), format lifecycles, backward compatibility, and determinism —
//! all against runtime-generated raster fixtures (no binary blobs).

use image::ImageEncoder as _;
use signal_cli::{build::build_site_from_disk, manifest::InputRef};
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

/// Deterministic W × H RGB gradient WebP, encoded in-test.
fn gradient_webp(width: u32, height: u32) -> Vec<u8> {
    let mut image = image::RgbImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        pixel.0 = [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8];
    }
    let mut bytes = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("fixture encodes");
    bytes
}

const HOME_TEMPLATE: &str = concat!(
    "{% if featured %}{% if featured.responsive_image %}",
    "{% if featured.responsive_image.has_picture %}<picture>",
    "{% for source in featured.responsive_image.sources %}",
    "<source type=\"{{ source.mime }}\" srcset=\"{{ source.srcset }}\" />",
    "{% endfor %}",
    "<img src=\"{{ featured.responsive_image.src }}\"",
    " srcset=\"{{ featured.responsive_image.srcset }}\"",
    " sizes=\"{{ featured.responsive_image.sizes }}\"",
    " width=\"{{ featured.responsive_image.width }}\"",
    " height=\"{{ featured.responsive_image.height }}\"",
    " alt=\"{{ featured.responsive_image.alt }}\" />",
    "</picture>",
    "{% else %}",
    "<img src=\"{{ featured.responsive_image.src }}\"",
    " srcset=\"{{ featured.responsive_image.srcset }}\"",
    " sizes=\"{{ featured.responsive_image.sizes }}\"",
    " width=\"{{ featured.responsive_image.width }}\"",
    " height=\"{{ featured.responsive_image.height }}\"",
    " alt=\"{{ featured.responsive_image.alt }}\" />",
    "{% endif %}",
    "{% elif featured.image %}",
    "<img src=\"{{ featured.image }}\" alt=\"{{ featured.image_alt | default('') }}\" />",
    "{% endif %}{% endif %}",
);

const POST_TEMPLATE: &str = concat!(
    "<html><body>",
    "{% if responsive_image %}",
    "{% if responsive_image.has_picture %}<picture>",
    "{% for source in responsive_image.sources %}",
    "<source type=\"{{ source.mime }}\" srcset=\"{{ source.srcset }}\" />",
    "{% endfor %}",
    "<img src=\"{{ responsive_image.src }}\" srcset=\"{{ responsive_image.srcset }}\"",
    " sizes=\"{{ responsive_image.sizes }}\" width=\"{{ responsive_image.width }}\"",
    " height=\"{{ responsive_image.height }}\" alt=\"{{ responsive_image.alt }}\" />",
    "</picture>",
    "{% else %}",
    "<img src=\"{{ responsive_image.src }}\" srcset=\"{{ responsive_image.srcset }}\"",
    " sizes=\"{{ responsive_image.sizes }}\" width=\"{{ responsive_image.width }}\"",
    " height=\"{{ responsive_image.height }}\" alt=\"{{ responsive_image.alt }}\" />",
    "{% endif %}",
    "{% elif image %}<img src=\"{{ image }}\" />{% endif %}",
    "{{ content | safe }}</body></html>",
);

fn site_with_hero(dir: &Path, config_extra: &str, body: &str) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                &format!(
                    "[site]\ntitle = \"Responsive\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n{config_extra}"
                ),
            ),
            (
                "content/posts/example.md",
                &format!("---\ntitle: Example\nimage: images/hero.png\nimage_alt: Hero alt\n---\n\n{body}\n"),
            ),
            ("templates/post.html", POST_TEMPLATE),
            (
                "templates/section.html",
                "<html><body>section {{ content | safe }}</body></html>",
            ),
        ],
    );
    write_bytes(dir, "static/images/hero.png", &gradient_png(160, 90));
}

fn home_site_with_hero(dir: &Path, config_extra: &str, image_front_matter: &str) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                &format!(
                    "[site]\ntitle = \"Responsive home\"\nhome_collection = \"posts\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n{config_extra}"
                ),
            ),
            (
                "content/posts/example.md",
                &format!(
                    "---\ntitle: Example\ndate: 2026-01-01\nfeatured: true\n{image_front_matter}---\n\nBody text.\n"
                ),
            ),
            ("templates/post.html", "<html><body>{{ content | safe }}</body></html>"),
            ("templates/section.html", "<html><body>section</body></html>"),
            ("templates/home.html", HOME_TEMPLATE),
        ],
    );
    write_bytes(dir, "static/images/hero.png", &gradient_png(160, 90));
}

fn decode_template_slashes(html: &str) -> String {
    html.replace("&#x2f;", "/").replace("&#47;", "/")
}

fn out_dir(dir: &Path) -> PathBuf {
    dir.join("out")
}

const IMAGES_CONFIG: &str = "[images]\nwidths = [80, 320]\nformat = \"webp\"\n";

const IMAGES_BOTH: &str = "[images]\nwidths = [80, 320]\nformats = [\"avif\", \"webp\"]\n";

#[test]
fn body_images_render_responsive_markup_with_actual_dimensions() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    // The 320w request clamps to the 160px source: srcset advertises
    // actual widths (80w, 160w), never the requested label.
    assert!(
        html.contains("<img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Peak\" />"),
        "got:\n{html}"
    );
}

#[test]
fn webp_sources_use_existing_responsive_and_picture_rendering() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Responsive WebP\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80, 320]\nformats = [\"avif\", \"webp\"]\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: WebP\nimage: images/hero.webp\nimage_alt: Hero alt\n---\n\nBody ![Body](/images/body.webp).\n",
            ),
            ("templates/post.html", POST_TEMPLATE),
            (
                "templates/section.html",
                "<html><body>section {{ content | safe }}</body></html>",
            ),
        ],
    );
    write_bytes(
        dir.path(),
        "static/images/hero.webp",
        &gradient_webp(160, 90),
    );
    write_bytes(
        dir.path(),
        "static/images/body.webp",
        &gradient_webp(160, 90),
    );
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");

    // Both the front-matter hero and the Markdown body source use the
    // existing two-format selector: AVIF first, WebP fallback.
    assert!(html.contains("<source type=\"image&#x2f;avif\" srcset=\"&#x2f;images&#x2f;hero-80.avif 80w, &#x2f;images&#x2f;hero-320.avif 160w\" />"), "got:\n{html}");
    assert!(html.contains("<source type=\"image&#x2f;webp\" srcset=\"&#x2f;images&#x2f;hero-80.webp 80w, &#x2f;images&#x2f;hero-320.webp 160w\" />"), "got:\n{html}");
    assert!(
        html.contains("<img src=\"&#x2f;images&#x2f;hero-320.webp\""),
        "got:\n{html}"
    );
    assert!(html.contains("<source type=\"image/avif\" srcset=\"/images/body-80.avif 80w, /images/body-320.avif 160w\" />"), "got:\n{html}");
    assert!(html.contains("<source type=\"image/webp\" srcset=\"/images/body-80.webp 80w, /images/body-320.webp 160w\" />"), "got:\n{html}");
    assert!(
        html.contains("<img src=\"/images/body-320.webp\""),
        "got:\n{html}"
    );
    assert_eq!(html.matches("<picture>").count(), 2, "got:\n{html}");
}

#[test]
fn webp_source_with_webp_only_config_uses_plain_responsive_img() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Responsive WebP only\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/example.md",
                "---\ntitle: WebP\nimage: images/hero.webp\nimage_alt: Hero alt\n---\n\nBody ![Body](/images/body.webp).\n",
            ),
            ("templates/post.html", POST_TEMPLATE),
            (
                "templates/section.html",
                "<html><body>section {{ content | safe }}</body></html>",
            ),
        ],
    );
    write_bytes(
        dir.path(),
        "static/images/hero.webp",
        &gradient_webp(160, 90),
    );
    write_bytes(
        dir.path(),
        "static/images/body.webp",
        &gradient_webp(160, 90),
    );
    let out = dir.path().join("out");
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(html.contains("<img src=\"&#x2f;images&#x2f;hero-80.webp\" srcset=\"&#x2f;images&#x2f;hero-80.webp 80w\" sizes=\"100vw\" width=\"80\" height=\"45\" alt=\"Hero alt\" />"), "got:\n{html}");
    assert!(html.contains("<img src=\"/images/body-80.webp\" srcset=\"/images/body-80.webp 80w\" sizes=\"100vw\" width=\"80\" height=\"45\" alt=\"Body\" />"), "got:\n{html}");
    assert!(!html.contains("<picture>"), "got:\n{html}");
}

#[test]
fn hero_context_renders_responsive_hero() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path(), IMAGES_CONFIG, "Body text.");
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    // Template interpolation escapes `/` exactly like the existing
    // `image` key (see the frozen goldens); entities decode in attribute
    // values, so the markup stays correct.
    assert!(
        html.contains("<img src=\"&#x2f;images&#x2f;hero-320.webp\" srcset=\"&#x2f;images&#x2f;hero-80.webp 80w, &#x2f;images&#x2f;hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Hero alt\" />"),
        "got:\n{html}"
    );
}

#[test]
fn clamped_duplicates_dedupe_to_one_candidate() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Both widths exceed the 160px source: one 160w candidate survives,
    // keeping the tighter (smaller requested) label.
    site_with_hero(
        dir.path(),
        "[images]\nwidths = [320, 640]\nformat = \"webp\"\n",
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(
        html.contains("srcset=\"/images/hero-320.webp 160w\""),
        "got:\n{html}"
    );
    assert!(
        !html.contains("1920w") && !html.contains(" 320w, "),
        "got:\n{html}"
    );
}

#[test]
fn unconfigured_sites_render_byte_identical_a1_markup() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path(), "", "Body ![Peak](/images/hero.png) text.");
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    // Comrak output untouched, hero falls back to the plain image key
    // (template-escaped, like every A1 `image` rendering).
    assert!(
        html.contains("<img src=\"/images/hero.png\" alt=\"Peak\" />"),
        "got:\n{html}"
    );
    assert!(
        html.contains("<img src=\"&#x2f;images&#x2f;hero.png\" />"),
        "got:\n{html}"
    );
    assert!(!html.contains("srcset"), "got:\n{html}");
}

#[test]
fn responsive_pages_track_source_and_config_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(text.contains("rebuild:   0\n"), "got:\n{text}");

    // New source dimensions flow into the markup: 120 × 60 hero, 80w
    // request still exact, 320w clamp now 120 × 68… (120 × 60 → 80 × 40).
    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(120, 60));
    build_site_from_disk(dir.path(), &out).expect("rebuilds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(
        html.contains("srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 120w\""),
        "got:\n{html}"
    );
    assert!(html.contains("width=\"120\" height=\"60\""), "got:\n{html}");
}

#[test]
fn section_bodies_render_responsively_and_track_inputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_site(
        dir.path(),
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Responsive\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80]\nformat = \"webp\"\n",
            ),
            (
                "content/posts/_index.md",
                "---\ntitle: Posts\n---\n\nSection ![Root](/images/hero.png) body.\n",
            ),
            (
                "content/posts/a.md",
                "---\ntitle: A\n---\n\nEntry body.\n",
            ),
            ("templates/post.html", "<html><body>{{ content | safe }}</body></html>"),
            (
                "templates/section.html",
                "<html><body>section {{ content | safe }}</body></html>",
            ),
        ],
    );
    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(160, 90));
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/index.html")).expect("section");
    assert!(
        html.contains("srcset=\"/images/hero-80.webp 80w\""),
        "got:\n{html}"
    );

    // The section reuses until its root's source changes…
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(text.contains("rebuild:   0\n"), "got:\n{text}");
    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(120, 60));
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(
        text.contains("  posts/index.html\n    reason: "),
        "section must rebuild with its root source: got:\n{text}"
    );
}

#[test]
fn listings_without_responsive_markup_keep_query_only_coverage() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    // The home page is not planned here (no home_collection), but the
    // section listing renders summaries only: no srcset anywhere.
    let html = std::fs::read_to_string(out.join("posts/index.html")).expect("section");
    assert!(!html.contains("srcset"), "got:\n{html}");
    assert!(
        summary
            .specs
            .iter()
            .all(|s| s.kind != signal_core::ArtifactKind::Home),
        "no home artifact in this fixture"
    );
}

#[test]
fn responsive_output_is_deterministic_across_builds() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(dir.path(), first.path()).expect("first builds");
    build_site_from_disk(dir.path(), second.path()).expect("second builds");
    let a = std::fs::read(first.path().join("posts/example/index.html")).expect("a");
    let b = std::fs::read(second.path().join("posts/example/index.html")).expect("b");
    assert_eq!(a, b);
}

#[test]
fn explain_derivative_shows_responsive_representation() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let text = signal_cli::explain::explain_derivative_from_disk(
        dir.path(),
        &out,
        "images/hero.png",
        80,
        None,
    )
    .expect("explains");
    assert!(
        text.contains("Responsive:\n  fallback: /images/hero-320.webp\n  sizes: 100vw\n  webp:\n    /images/hero-80.webp 80w\n    /images/hero-320.webp 160w\n"),
        "got:\n{text}"
    );
}

#[test]
fn multi_format_body_images_render_picture_avif_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_BOTH,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    // Two formats × two widths: four derivatives plus the source.
    assert_eq!(summary.derived_images, 4);
    for output in [
        "images/hero-80.avif",
        "images/hero-320.avif",
        "images/hero-80.webp",
        "images/hero-320.webp",
    ] {
        assert!(out.join(output).exists(), "missing {output}");
    }
    // AVIF bytes are a real AVIF container, distinct from the WebP bytes.
    let avif = std::fs::read(out.join("images/hero-80.avif")).expect("avif");
    assert_eq!(&avif[4..12], b"ftypavif");
    let webp = std::fs::read(out.join("images/hero-80.webp")).expect("webp");
    assert_ne!(avif, webp);
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(
        html.contains("<picture><source type=\"image/avif\" srcset=\"/images/hero-80.avif 80w, /images/hero-320.avif 160w\" /><source type=\"image/webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" /><img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Peak\" /></picture>"),
        "got:\n{html}"
    );
}

#[test]
fn multi_format_hero_renders_picture_from_template_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(dir.path(), IMAGES_BOTH, "Body text.");
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    // Template-escaped slashes decode identically in attribute values;
    // AVIF source precedes WebP, WebP fallback closes the picture.
    assert!(html.contains("<picture>"), "got:\n{html}");
    // Template interpolation escapes `/` (like every A1 `image`
    // rendering); entities decode in attribute values.
    let avif_pos = html.find("image&#x2f;avif").expect("avif source");
    let webp_pos = html.find("image&#x2f;webp").expect("webp source");
    assert!(avif_pos < webp_pos, "AVIF source first: got:\n{html}");
    assert!(
        html.contains("hero-320.avif 160w") && html.contains("hero-320.webp 160w"),
        "both groups present: got:\n{html}"
    );
}

#[test]
fn avif_only_config_renders_plain_responsive_img() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        "[images]\nwidths = [80]\nformat = \"avif\"\n",
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    assert_eq!(summary.derived_images, 1);
    assert!(out.join("images/hero-80.avif").exists());
    assert!(!out.join("images/hero-80.webp").exists());
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(
        !html.contains("<picture>"),
        "single format, no picture: got:\n{html}"
    );
    assert!(
        html.contains("<img src=\"/images/hero-80.avif\" srcset=\"/images/hero-80.avif 80w\" sizes=\"100vw\" width=\"80\" height=\"45\" alt=\"Peak\" />"),
        "got:\n{html}"
    );
}

#[test]
fn webp_only_config_is_unchanged_by_a4() {
    // A site that never opts into AVIF renders exactly the A3 markup:
    // plain responsive `<img>`, no `<picture>`, no `.avif` outputs.
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(!html.contains("<picture>"), "got:\n{html}");
    assert!(!html.contains(".avif"), "got:\n{html}");
    assert!(
        html.contains("<img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Peak\" />"),
        "got:\n{html}"
    );
}

#[test]
fn removing_a_format_prunes_its_derivatives_and_reuses_the_rest() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_BOTH,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");
    // Drop AVIF: its artifacts prune, WebP reuses, the page rebuilds
    // (picture → img) exactly once.
    site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "Body ![Peak](/images/hero.png) text.",
    );
    let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
    assert!(!out.join("images/hero-80.avif").exists());
    assert!(!out.join("images/hero-320.avif").exists());
    assert!(out.join("images/hero-80.webp").exists());
    assert!(
        summary
            .pruned_paths
            .contains(&"images/hero-80.avif".to_string()),
        "got: {:?}",
        summary.pruned_paths
    );
    assert!(
        summary
            .pruned_paths
            .contains(&"images/hero-320.avif".to_string()),
        "got: {:?}",
        summary.pruned_paths
    );
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    let reuse = text.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  images/hero-80.webp\n"), "got:\n{text}");
    let html = std::fs::read_to_string(out.join("posts/example/index.html")).expect("page");
    assert!(
        !html.contains("<picture>"),
        "back to plain img: got:\n{html}"
    );
}

#[test]
fn source_change_rebuilds_both_formats_and_consumers() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_BOTH,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");
    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(120, 60));
    let text = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    for rebuilt in [
        "  images/hero-80.avif\n",
        "  images/hero-320.avif\n",
        "  images/hero-80.webp\n",
        "  images/hero-320.webp\n",
        "  posts/example/index.html\n",
    ] {
        let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild section");
        assert!(
            rebuild.contains(rebuilt),
            "missing {rebuilt:?}: got:\n{text}"
        );
    }
}

#[test]
fn multi_format_output_is_deterministic_across_builds() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_BOTH,
        "Body ![Peak](/images/hero.png) text.",
    );
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(dir.path(), first.path()).expect("first builds");
    build_site_from_disk(dir.path(), second.path()).expect("second builds");
    for rel in [
        "posts/example/index.html",
        "images/hero-80.avif",
        "images/hero-320.avif",
        "images/hero-80.webp",
        "images/hero-320.webp",
        ".signal/manifest.json",
    ] {
        let a = std::fs::read(first.path().join(rel)).expect(rel);
        let b = std::fs::read(second.path().join(rel)).expect(rel);
        assert_eq!(a, b, "{rel} differs across builds");
    }
}

#[test]
fn explain_multi_format_derivative_shows_both_groups() {
    let dir = tempfile::tempdir().expect("tempdir");
    site_with_hero(
        dir.path(),
        IMAGES_BOTH,
        "Body ![Peak](/images/hero.png) text.",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");
    let text = signal_cli::explain::explain_derivative_from_disk(
        dir.path(),
        &out,
        "images/hero.png",
        80,
        Some("avif"),
    )
    .expect("explains");
    assert!(text.contains("format: avif"), "got:\n{text}");
    assert!(
        text.contains("Responsive:\n  fallback: /images/hero-320.webp\n  sizes: 100vw\n  avif:\n    /images/hero-80.avif 80w\n    /images/hero-320.avif 160w\n  webp:\n    /images/hero-80.webp 80w\n    /images/hero-320.webp 160w\n"),
        "got:\n{text}"
    );
}

#[test]
fn homepage_featured_hero_renders_picture_avif_first_without_alt() {
    let dir = tempfile::tempdir().expect("tempdir");
    home_site_with_hero(dir.path(), IMAGES_BOTH, "image: images/hero.png\n");
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(
        html.contains(
            "<picture><source type=\"image/avif\" srcset=\"/images/hero-80.avif 80w, /images/hero-320.avif 160w\" /><source type=\"image/webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" /><img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"\" /></picture>"
        ),
        "got:\n{html}"
    );

    let manifest = signal_cli::manifest::parse_manifest(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("manifest parses");
    let inputs = &manifest.artifacts["index.html"].inputs;
    assert!(inputs.contains(&InputRef::Entry {
        route: "/posts/example/".to_string()
    }));
    assert!(inputs.contains(&InputRef::Static {
        path: "images/hero.png".to_string()
    }));
    for format in ["avif", "webp"] {
        assert!(inputs.contains(&InputRef::DerivedImage {
            source: "images/hero.png".to_string(),
            width: 80,
            format: format.to_string(),
        }));
        assert!(inputs.contains(&InputRef::DerivedImage {
            source: "images/hero.png".to_string(),
            width: 320,
            format: format.to_string(),
        }));
    }
}

#[test]
fn homepage_featured_hero_single_format_is_responsive_img_with_clamped_widths() {
    let dir = tempfile::tempdir().expect("tempdir");
    home_site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "image: images/hero.png\nimage_alt: Homepage hero\n",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("builds");

    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(
        html.contains(
            "<img src=\"/images/hero-320.webp\" srcset=\"/images/hero-80.webp 80w, /images/hero-320.webp 160w\" sizes=\"100vw\" width=\"160\" height=\"90\" alt=\"Homepage hero\" />"
        ),
        "got:\n{html}"
    );
    assert!(!html.contains("<picture>"), "got:\n{html}");
}

#[test]
fn homepage_featured_hero_falls_back_to_plain_image_when_pipeline_is_unavailable() {
    for (name, config, front_matter, extra_file, expected) in [
        (
            "disabled",
            "",
            "image: images/hero.png\nimage_alt: Plain hero\n",
            None,
            "<img src=\"/images/hero.png\" alt=\"Plain hero\" />",
        ),
        (
            "external",
            IMAGES_CONFIG,
            "image: https://cdn.example.com/hero.png\nimage_alt: Remote hero\n",
            None,
            "<img src=\"https://cdn.example.com/hero.png\" alt=\"Remote hero\" />",
        ),
        (
            "non-raster",
            IMAGES_CONFIG,
            "image: images/logo.svg\nimage_alt: Logo\n",
            Some(("static/images/logo.svg", "<svg></svg>\n")),
            "<img src=\"/images/logo.svg\" alt=\"Logo\" />",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        home_site_with_hero(dir.path(), config, front_matter);
        if let Some((path, contents)) = extra_file {
            write_site(dir.path(), &[(path, contents)]);
        }
        let out = out_dir(dir.path());
        let summary = build_site_from_disk(dir.path(), &out).expect("builds");
        assert_eq!(summary.derived_images, 0, "{name} must not derive");
        let html = decode_template_slashes(
            &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
        );
        assert!(html.contains(expected), "{name}: got:\n{html}");
        assert!(
            !html.contains("srcset") && !html.contains("<picture>"),
            "{name}: got:\n{html}"
        );
    }
}

#[test]
fn homepage_without_featured_entry_exposes_no_hero_or_derivatives() {
    let dir = tempfile::tempdir().expect("tempdir");
    home_site_with_hero(dir.path(), IMAGES_CONFIG, "image: images/hero.png\n");
    write_site(
        dir.path(),
        &[(
            "content/posts/example.md",
            "---\ntitle: Example\ndate: 2026-01-01\n---\n\nNot featured.\n",
        )],
    );
    let out = out_dir(dir.path());
    let summary = build_site_from_disk(dir.path(), &out).expect("builds");
    assert_eq!(summary.derived_images, 0);
    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(html.is_empty(), "got:\n{html}");

    let manifest = signal_cli::manifest::parse_manifest(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("manifest parses");
    assert!(!manifest.artifacts["index.html"]
        .inputs
        .iter()
        .any(|input| matches!(
            input,
            InputRef::Entry { .. } | InputRef::Static { .. } | InputRef::DerivedImage { .. }
        )));
}

#[test]
fn homepage_hero_source_change_rebuilds_derivatives_and_home_then_fully_reuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let one_format = "[images]\nwidths = [80]\nformat = \"webp\"\n";
    home_site_with_hero(
        dir.path(),
        one_format,
        "image: images/hero.png\nimage_alt: Hero\n",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");

    let explain = signal_cli::explain::explain_asset_from_disk(dir.path(), &out, "index.html")
        .expect("homepage explains");
    assert!(explain.contains("  DerivedImage(images/hero.png, 80w, webp)\n"));
    assert!(explain.contains("Decision:\n  reuse\n"), "got:\n{explain}");

    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(120, 60));
    let plan = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    for path in [
        "  images/hero.png\n",
        "  images/hero-80.webp\n",
        "  index.html\n",
    ] {
        assert!(
            plan.split("Rebuild:\n")
                .nth(1)
                .expect("rebuilds")
                .contains(path),
            "missing {path:?}: got:\n{plan}"
        );
    }
    build_site_from_disk(dir.path(), &out).expect("rebuilds");

    let unchanged = build_site_from_disk(dir.path(), &out).expect("no-op builds");
    assert_eq!(unchanged.rebuilt, 0, "got: {unchanged:?}");
    assert!(unchanged.reused > 0);
    let plan = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(plan.contains("rebuild:   0\n"), "got:\n{plan}");
    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(html.contains("width=\"80\" height=\"40\""), "got:\n{html}");
}

#[test]
fn removing_homepage_avif_prunes_only_avif_and_changes_picture_to_img() {
    let dir = tempfile::tempdir().expect("tempdir");
    let one_format = "[images]\nwidths = [80]\nformat = \"webp\"\n";
    home_site_with_hero(
        dir.path(),
        "[images]\nwidths = [80]\nformats = [\"avif\", \"webp\"]\n",
        "image: images/hero.png\nimage_alt: Hero\n",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");
    assert!(out.join("images/hero-80.avif").exists());

    home_site_with_hero(
        dir.path(),
        one_format,
        "image: images/hero.png\nimage_alt: Hero\n",
    );
    let plan = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    let reuse = plan.split("Reuse:\n").nth(1).expect("reuse section");
    assert!(reuse.contains("  images/hero-80.webp\n"), "got:\n{plan}");
    assert!(
        plan.contains("Prune:\n  images/hero-80.avif\n"),
        "got:\n{plan}"
    );
    assert!(
        plan.contains("  index.html\n    reason: inputs changed\n"),
        "got:\n{plan}"
    );

    let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
    assert_eq!(
        summary.pruned_paths,
        vec!["images/hero-80.avif".to_string()]
    );
    assert!(!out.join("images/hero-80.avif").exists());
    assert!(out.join("images/hero-80.webp").exists());
    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(
        !html.contains("<picture>") && !html.contains("avif"),
        "got:\n{html}"
    );
    assert!(
        html.contains("src=\"/images/hero-80.webp\""),
        "got:\n{html}"
    );
}

#[test]
fn homepage_switches_responsive_hero_when_featured_selection_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    home_site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "image: images/hero.png\nimage_alt: First hero\n",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");

    write_site(
        dir.path(),
        &[(
            "content/posts/newer.md",
            "---\ntitle: Newer\ndate: 2026-02-01\nfeatured: true\nimage: images/newer.png\nimage_alt: Newer hero\n---\n\nNewer body.\n",
        )],
    );
    write_bytes(
        dir.path(),
        "static/images/newer.png",
        &gradient_png(200, 100),
    );
    let plan = signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    assert!(
        plan.contains("  index.html\n    reason: inputs changed\n"),
        "got:\n{plan}"
    );
    assert!(
        plan.contains("  images/newer-80.webp\n    reason: manifest record missing\n"),
        "got:\n{plan}"
    );
    build_site_from_disk(dir.path(), &out).expect("switch builds");

    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(
        html.contains("src=\"/images/newer-320.webp\""),
        "got:\n{html}"
    );
    assert!(html.contains("alt=\"Newer hero\""), "got:\n{html}");
    assert!(
        !html.contains("src=\"/images/hero-320.webp\""),
        "got:\n{html}"
    );
}

#[test]
fn removing_featured_image_removes_responsive_homepage_markup() {
    let dir = tempfile::tempdir().expect("tempdir");
    home_site_with_hero(
        dir.path(),
        IMAGES_CONFIG,
        "image: images/hero.png\nimage_alt: Hero\n",
    );
    let out = out_dir(dir.path());
    build_site_from_disk(dir.path(), &out).expect("first builds");

    write_site(
        dir.path(),
        &[(
            "content/posts/example.md",
            "---\ntitle: Example\ndate: 2026-01-01\nfeatured: true\n---\n\nNo hero.\n",
        )],
    );
    let summary = build_site_from_disk(dir.path(), &out).expect("rebuilds");
    assert!(summary
        .pruned_paths
        .contains(&"images/hero-80.webp".to_string()));
    assert!(summary
        .pruned_paths
        .contains(&"images/hero-320.webp".to_string()));
    let html = decode_template_slashes(
        &std::fs::read_to_string(out.join("index.html")).expect("homepage"),
    );
    assert!(html.is_empty(), "got:\n{html}");
    let manifest = signal_cli::manifest::parse_manifest(
        &std::fs::read_to_string(out.join(".signal/manifest.json")).expect("manifest"),
    )
    .expect("manifest parses");
    assert!(!manifest.artifacts["index.html"]
        .inputs
        .iter()
        .any(|input| matches!(input, InputRef::DerivedImage { .. })));
}
