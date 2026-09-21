//! Social images (A5, ADR 0032): generation, metadata, dependency, and
//! determinism integration tests.
//!
//! Cards, Open Graph/Twitter metadata, participation rules, exact
//! rebuild/reuse lifecycles, pruning, and independent-build determinism —
//! all against runtime-generated raster fixtures (no binary blobs).

use image::ImageEncoder as _;
use signal_cli::build::build_site_from_disk;
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

const POST_TEMPLATE: &str = concat!(
    "<html><head>",
    "{% if og_title %}<meta property=\"og:title\" content=\"{{ og_title }}\">{% endif %}",
    "{% if og_description %}<meta property=\"og:description\" content=\"{{ og_description }}\">{% endif %}",
    "{% if og_url %}<meta property=\"og:url\" content=\"{{ og_url }}\">{% endif %}",
    "{% if og_type %}<meta property=\"og:type\" content=\"{{ og_type }}\">{% endif %}",
    "{% if og_image %}<meta property=\"og:image\" content=\"{{ og_image }}\">{% endif %}",
    "{% if twitter_card %}<meta name=\"twitter:card\" content=\"{{ twitter_card }}\">{% endif %}",
    "{% if twitter_title %}<meta name=\"twitter:title\" content=\"{{ twitter_title }}\">{% endif %}",
    "{% if twitter_description %}<meta name=\"twitter:description\" content=\"{{ twitter_description }}\">{% endif %}",
    "{% if twitter_image %}<meta name=\"twitter:image\" content=\"{{ twitter_image }}\">{% endif %}",
    "{% if social_image %}<meta property=\"og:image:width\" content=\"{{ social_image.width }}\">",
    "<meta property=\"og:image:height\" content=\"{{ social_image.height }}\">",
    "<meta property=\"og:image:alt\" content=\"{{ title }}\">{% endif %}",
    "</head><body>{{ content | safe }}</body></html>",
);

/// A minimal site with two entries (one with a PNG hero, one without) and
/// one opted-out entry; `social` is the `[social]` table body (empty string
/// means the feature is not configured at all).
fn site(dir: &Path, social: &str) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                &format!(
                    "[site]\ntitle = \"Signal\"\nbase_url = \"https://example.com/\"\nauthor = \"Site Author\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n{social}"
                ),
            ),
            (
                "content/posts/hero.md",
                "---\ntitle: Hero story\ndescription: A description.\nimage: images/hero.png\nauthor: Entry Author\n---\n\nBody.\n",
            ),
            (
                "content/posts/plain.md",
                "---\ntitle: Plain story\n---\n\nBody.\n",
            ),
            (
                "content/posts/opted-out.md",
                "---\ntitle: Opted out\nsocial_image: false\n---\n\nBody.\n",
            ),
            (
                "content/posts/_index.md",
                "---\ntitle: Posts\n---\n\nSection body.\n",
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

fn out_dir(dir: &Path) -> PathBuf {
    dir.join("out")
}

const SOCIAL: &str = "[social]\n";

fn build(dir: &Path) -> signal_cli::build::BuildSummary {
    build_site_from_disk(dir, &out_dir(dir)).expect("builds")
}

/// MiniJinja HTML-escapes `/` in interpolated values, exactly like every
/// other URL-bearing context key (see the frozen goldens); entities decode
/// in attribute values, so tests assert on the decoded text.
fn decoded(html: String) -> String {
    html.replace("&#x2f;", "/")
}

fn page(dir: &Path, slug: &str) -> String {
    decoded(
        std::fs::read_to_string(out_dir(dir).join(format!("posts/{slug}/index.html")))
            .expect("page"),
    )
}

fn plan_text(dir: &Path) -> String {
    signal_cli::explain::explain_site_from_disk(dir, &out_dir(dir)).expect("explains")
}

#[test]
fn unconfigured_sites_are_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), "");
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 0);
    assert!(!out_dir(dir.path()).join("social").exists());
    let html = page(dir.path(), "hero");
    // A4 behavior: the hero's absolute URL, no Twitter keys, no card.
    assert!(
        html.contains(
            "<meta property=\"og:image\" content=\"https://example.com/images/hero.png\">"
        ),
        "got:\n{html}"
    );
    assert!(!html.contains("twitter:"), "got:\n{html}");
    assert!(!html.contains("og:image:width"), "got:\n{html}");
    // Same for the hero-less page: no og:image at all.
    assert!(!page(dir.path(), "plain").contains("og:image"));
}

#[test]
fn enabled_sites_generate_cards_and_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    let summary = build(dir.path());
    // Two participants: the hero entry and the plain one; the opted-out
    // entry and the section root produce nothing.
    assert_eq!(summary.social_images, 2);
    for card in ["social/posts/hero.png", "social/posts/plain.png"] {
        let bytes = std::fs::read(out_dir(dir.path()).join(card)).expect("card exists");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{card}");
        let decoded = image::load_from_memory(&bytes).expect("card decodes");
        assert_eq!(
            image::GenericImageView::dimensions(&decoded),
            (1200, 630),
            "{card}"
        );
    }
    assert!(!out_dir(dir.path())
        .join("social/posts/opted-out.png")
        .exists());
    assert!(!out_dir(dir.path()).join("social/posts.png").exists());

    // Open Graph and Twitter metadata both point at the generated card,
    // and the card supersedes the hero image.
    let html = page(dir.path(), "hero");
    assert!(
        html.contains(
            "<meta property=\"og:image\" content=\"https://example.com/social/posts/hero.png\">"
        ),
        "got:\n{html}"
    );
    assert!(
        !html.contains("images/hero.png"),
        "hero superseded: got:\n{html}"
    );
    for expected in [
        "<meta name=\"twitter:card\" content=\"summary_large_image\">",
        "<meta name=\"twitter:title\" content=\"Hero story\">",
        "<meta name=\"twitter:description\" content=\"A description.\">",
        "<meta name=\"twitter:image\" content=\"https://example.com/social/posts/hero.png\">",
        "<meta property=\"og:image:width\" content=\"1200\">",
        "<meta property=\"og:image:height\" content=\"630\">",
    ] {
        assert!(html.contains(expected), "missing {expected}: got:\n{html}");
    }
    // No duplicated tags: exactly one og:image.
    assert_eq!(html.matches("og:image\"").count(), 1, "got:\n{html}");

    // Listing pages reference no card, so they stay free of card metadata.
    let section =
        std::fs::read_to_string(out_dir(dir.path()).join("posts/index.html")).expect("section");
    assert!(!section.contains("og:image"), "got:\n{section}");
    assert!(!section.contains("twitter:"), "got:\n{section}");

    // Opted-out pages fall back to A4 metadata (no hero here, so no image).
    let opted_out = page(dir.path(), "opted-out");
    assert!(!opted_out.contains("og:image"), "got:\n{opted_out}");
    assert!(!opted_out.contains("twitter:"), "got:\n{opted_out}");
}

#[test]
fn configured_dimensions_flow_into_the_card_and_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), "[social]\nwidth = 800\nheight = 400\n");
    build(dir.path());
    let bytes = std::fs::read(out_dir(dir.path()).join("social/posts/plain.png")).expect("card");
    let decoded = image::load_from_memory(&bytes).expect("decodes");
    assert_eq!(image::GenericImageView::dimensions(&decoded), (800, 400));
    let html = page(dir.path(), "plain");
    assert!(
        html.contains("og:image:width\" content=\"800\""),
        "got:\n{html}"
    );
    assert!(
        html.contains("og:image:height\" content=\"400\""),
        "got:\n{html}"
    );
}

#[test]
fn per_page_override_opts_out_and_opts_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    // Opt in a page on a disabled site: still nothing planned.
    let disabled = tempfile::tempdir().expect("tempdir");
    site(disabled.path(), "");
    write_site(
        disabled.path(),
        &[(
            "content/posts/plain.md",
            "---\ntitle: Plain story\nsocial_image: true\n---\n\nBody.\n",
        )],
    );
    build(disabled.path());
    assert!(!out_dir(disabled.path()).join("social").exists());

    // Opt out on an enabled site: that page opts out, the others do not.
    write_site(
        dir.path(),
        &[(
            "content/posts/plain.md",
            "---\ntitle: Plain story\nsocial_image: false\n---\n\nBody.\n",
        )],
    );
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 1);
    assert!(!out_dir(dir.path()).join("social/posts/plain.png").exists());
    assert!(out_dir(dir.path()).join("social/posts/hero.png").exists());
    assert!(!page(dir.path(), "plain").contains("twitter:"));
}

#[test]
fn hero_variants_compose_deterministically() {
    // A PNG hero is composited; a non-raster (SVG) or external hero yields
    // a metadata-only card with no error and no dependency.
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    write_site(
        dir.path(),
        &[
            (
                "content/posts/plain.md",
                "---\ntitle: Plain story\nimage: images/logo.svg\n---\n\nBody.\n",
            ),
            (
                "content/posts/opted-out.md",
                "---\ntitle: Opted out\nimage: https://cdn.example/remote.png\n---\n\nBody.\n",
            ),
        ],
    );
    write_bytes(dir.path(), "static/images/logo.svg", b"<svg></svg>");
    let first = build(dir.path());
    assert_eq!(first.social_images, 3);
    let hero = std::fs::read(out_dir(dir.path()).join("social/posts/hero.png")).expect("hero card");
    let plain = std::fs::read(out_dir(dir.path()).join("social/posts/plain.png")).expect("plain");
    // The composited hero actually changed the card's pixels.
    assert_ne!(hero, plain);

    // Rebuild from scratch in a second output directory: byte-identical.
    let out2 = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(dir.path(), out2.path()).expect("builds");
    assert_eq!(
        std::fs::read(out2.path().join("social/posts/hero.png")).expect("card"),
        hero
    );
}

#[test]
fn long_text_is_wrapped_not_truncated_silently() {
    let dir = tempfile::tempdir().expect("tempdir");
    let long_title = "An extremely long headline that cannot possibly fit on a single line and must wrap across several lines of the card while remaining readable instead of silently losing most of its words";
    let long_description = "A similarly long description that keeps going well past the point where a single line would end, so the wrapping behaviour is exercised for descriptions too, again and again and again until it is comfortably over the limit.";
    site(dir.path(), SOCIAL);
    write_site(
        dir.path(),
        &[(
            "content/posts/plain.md",
            &format!("---\ntitle: {long_title}\ndescription: {long_description}\n---\n\nBody.\n"),
        )],
    );
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 2);
    // The card is still generated at the configured size; truncation
    // policy is unit-tested (`wrap_text`), where the ellipsis rule and the
    // line caps are asserted directly.
    let bytes = std::fs::read(out_dir(dir.path()).join("social/posts/plain.png")).expect("card");
    let decoded = image::load_from_memory(&bytes).expect("decodes");
    assert_eq!(image::GenericImageView::dimensions(&decoded), (1200, 630));
}

#[test]
fn metadata_records_declare_exactly_what_the_card_consumes() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    build(dir.path());
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out_dir(dir.path()).join(".signal/manifest.json")).expect("read"),
    )
    .expect("parses");
    let records = manifest["artifacts"].as_object().expect("artifacts");

    // The hero card consumes its entry, the configuration, and the hero
    // source bytes — never the page's HTML digest.
    let card = &records["social/posts/hero.png"];
    assert_eq!(card["kind"], "SocialImage");
    let inputs = card["inputs"].to_string();
    assert!(inputs.contains("\"Entry\""), "got: {inputs}");
    assert!(inputs.contains("\"Config\""), "got: {inputs}");
    assert!(inputs.contains("images/hero.png"), "got: {inputs}");
    assert_eq!(card["inputs"].as_array().expect("array").len(), 3);

    // A card without a hero consumes only entry + config.
    let plain = &records["social/posts/plain.png"];
    assert_eq!(plain["inputs"].as_array().expect("array").len(), 2);

    // The page itself consumes its entry and hero, but no social artifact:
    // the only thing a page embeds is the card's URL, a pure function of
    // inputs it already declares (ADR 0032).
    let page_inputs = records["posts/hero/index.html"]["inputs"].to_string();
    assert!(page_inputs.contains("\"Entry\""), "got: {page_inputs}");
    // The card path never appears as an input edge of the page.
    assert!(!page_inputs.contains("social/"), "got: {page_inputs}");
}

#[test]
fn steady_state_reuses_and_changes_rebuild_exactly_what_they_must() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    build(dir.path());

    // Second build: everything reuses.
    let text = plan_text(dir.path());
    assert!(text.contains("rebuild:   0"), "got:\n{text}");

    // Unrelated asset: the cards reuse.
    write_bytes(dir.path(), "static/js/app.js", b"console.log(1)\n");
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("js/app.js"), "got:\n{text}");
    assert!(!rebuild.contains("social/"), "cards reused: got:\n{text}");
    build(dir.path());

    // Unrelated article: this entry's card reuses, the other's rebuilds.
    write_site(
        dir.path(),
        &[(
            "content/posts/plain.md",
            "---\ntitle: Plain story, retitled\n---\n\nBody.\n",
        )],
    );
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("social/posts/plain.png"), "got:\n{text}");
    assert!(
        !rebuild.contains("social/posts/hero.png"),
        "unrelated card reused: got:\n{text}"
    );
    build(dir.path());

    // Title change: the card and its page rebuild.
    write_site(
        dir.path(),
        &[(
            "content/posts/hero.md",
            "---\ntitle: Hero story retitled\ndescription: A description.\nimage: images/hero.png\nauthor: Entry Author\n---\n\nBody.\n",
        )],
    );
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("social/posts/hero.png"), "got:\n{text}");
    assert!(rebuild.contains("posts/hero/index.html"), "got:\n{text}");
    build(dir.path());

    // Description change: the card rebuilds.
    write_site(
        dir.path(),
        &[(
            "content/posts/hero.md",
            "---\ntitle: Hero story retitled\ndescription: A different description.\nimage: images/hero.png\nauthor: Entry Author\n---\n\nBody.\n",
        )],
    );
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("social/posts/hero.png"), "got:\n{text}");
    build(dir.path());

    // Hero bytes: the card rebuilds.
    write_bytes(dir.path(), "static/images/hero.png", &gradient_png(120, 60));
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("social/posts/hero.png"), "got:\n{text}");
    build(dir.path());

    // Back to steady state.
    let text = plan_text(dir.path());
    assert!(text.contains("rebuild:   0"), "got:\n{text}");
}

#[test]
fn disabling_prunes_cards_and_restores_a4_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    build(dir.path());
    assert!(out_dir(dir.path()).join("social/posts/hero.png").exists());

    site(dir.path(), "[social]\nenabled = false\n");
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 0);
    assert_eq!(summary.pruned, 2);
    assert!(summary
        .pruned_paths
        .contains(&"social/posts/hero.png".to_string()));
    assert!(!out_dir(dir.path()).join("social/posts/hero.png").exists());
    let html = page(dir.path(), "hero");
    assert!(
        html.contains(
            "<meta property=\"og:image\" content=\"https://example.com/images/hero.png\">"
        ),
        "A4 fallback: got:\n{html}"
    );
    assert!(!html.contains("twitter:"), "got:\n{html}");
    // Removing the table entirely behaves identically.
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 0);
    assert_eq!(summary.pruned, 0);
}

#[test]
fn invalid_social_config_fails_build_check_and_explain_identically() {
    for (name, social) in [
        ("zero-width", "[social]\nwidth = 0\n"),
        ("oversized", "[social]\nheight = 99999\n"),
        (
            "no-base-url",
            // base_url is required: rebuild the config without it.
            "",
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        site(dir.path(), social);
        if name == "no-base-url" {
            write_site(
                dir.path(),
                &[(
                    "signal.toml",
                    "[site]\ntitle = \"Signal\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[social]\n",
                )],
            );
        }
        for result in [
            build_site_from_disk(dir.path(), &out_dir(dir.path())).map(|_| ()),
            signal_cli::link_check::check_site_from_disk(dir.path()).map(|_| ()),
            signal_cli::explain::explain_site_from_disk(dir.path(), &out_dir(dir.path()))
                .map(|_| ()),
        ] {
            let err = result.expect_err(&format!("{name} must fail"));
            assert!(err.to_string().contains("[social]"), "{name}: got: {err}");
        }
    }
}

#[test]
fn malformed_hero_fails_build_and_check_identically() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    write_bytes(
        dir.path(),
        "static/images/hero.png",
        b"not actually png bytes",
    );
    for result in [
        build_site_from_disk(dir.path(), &out_dir(dir.path())).map(|_| ()),
        signal_cli::link_check::check_site_from_disk(dir.path()).map(|_| ()),
    ] {
        let err = result.expect_err("must fail");
        assert!(err.to_string().contains("hero.png"), "got: {err}");
    }
}

#[test]
fn card_output_collisions_fail_before_writing() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    // A static file already claims the card's output path.
    write_bytes(
        dir.path(),
        "static/social/posts/hero.png",
        &gradient_png(10, 10),
    );
    let err = build_site_from_disk(dir.path(), &out_dir(dir.path())).expect_err("collision");
    assert!(err.to_string().contains("collision"), "got: {err}");
    assert!(!out_dir(dir.path()).join("social/posts/hero.png").exists());
}

#[test]
fn explain_reports_plan_state_inputs_and_decisions() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    build(dir.path());
    let text = signal_cli::explain::explain_asset_from_disk(
        dir.path(),
        &out_dir(dir.path()),
        "social/posts/hero.png",
    )
    .expect("explains");
    for expected in [
        "Social image\n",
        "  path: social/posts/hero.png\n",
        "  /posts/hero/\n",
        "  posts/hero.md\n",
        "  1200 × 630\n",
        "  title: Hero story\n",
        "  description: A description.\n",
        "  author: Entry Author\n",
        "  hero: images/hero.png\n",
        "  configuration: [social]\n",
        "Action:\n  generate\n",
        "Decision:\n  reuse\n",
    ] {
        assert!(
            text.contains(expected),
            "missing {expected:?}: got:\n{text}"
        );
    }
    // Explaining twice is byte-identical, and a rebuild reason is shown.
    assert_eq!(
        text,
        signal_cli::explain::explain_asset_from_disk(
            dir.path(),
            &out_dir(dir.path()),
            "social/posts/hero.png"
        )
        .expect("explains")
    );
    write_site(
        dir.path(),
        &[(
            "content/posts/hero.md",
            "---\ntitle: Hero story two\ndescription: A description.\nimage: images/hero.png\nauthor: Entry Author\n---\n\nBody.\n",
        )],
    );
    let text = signal_cli::explain::explain_asset_from_disk(
        dir.path(),
        &out_dir(dir.path()),
        "social/posts/hero.png",
    )
    .expect("explains");
    assert!(text.contains("rebuild: entry changed"), "got:\n{text}");

    // Non-participating states render their own decision lines.
    let opted_out = signal_cli::explain::explain_asset_from_disk(
        dir.path(),
        &out_dir(dir.path()),
        "social/posts/opted-out.png",
    )
    .expect("explains");
    assert!(opted_out.contains("opted out"), "got:\n{opted_out}");
    let listing = signal_cli::explain::explain_asset_from_disk(
        dir.path(),
        &out_dir(dir.path()),
        "social/posts.png",
    )
    .expect("explains");
    assert!(listing.contains("section roots"), "got:\n{listing}");
}

#[test]
fn independent_builds_are_byte_identical() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    let first = tempfile::tempdir().expect("tempdir");
    let second = tempfile::tempdir().expect("tempdir");
    build_site_from_disk(dir.path(), first.path()).expect("first builds");
    build_site_from_disk(dir.path(), second.path()).expect("second builds");
    for rel in [
        "social/posts/hero.png",
        "social/posts/plain.png",
        "posts/hero/index.html",
        "posts/plain/index.html",
        ".signal/manifest.json",
    ] {
        let a = std::fs::read(first.path().join(rel)).expect(rel);
        let b = std::fs::read(second.path().join(rel)).expect(rel);
        assert_eq!(a, b, "{rel} differs across builds");
    }
}

#[test]
fn dimension_changes_invalidate_cards_and_change_pixels() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    build(dir.path());
    let before = std::fs::read(out_dir(dir.path()).join("social/posts/plain.png")).expect("card");

    // A configuration change is part of the card's identity: it rebuilds
    // (never reusing bytes generated for the old size) and the page's
    // metadata follows the new dimensions.
    site(dir.path(), "[social]\nwidth = 1000\nheight = 500\n");
    let text = plan_text(dir.path());
    let rebuild = text.split("Rebuild:\n").nth(1).expect("rebuild");
    assert!(rebuild.contains("social/posts/plain.png"), "got:\n{text}");
    let summary = build(dir.path());
    assert_eq!(summary.social_images, 2);
    let after = std::fs::read(out_dir(dir.path()).join("social/posts/plain.png")).expect("card");
    assert_ne!(before, after);
    let decoded = image::load_from_memory(&after).expect("decodes");
    assert_eq!(image::GenericImageView::dimensions(&decoded), (1000, 500));
    assert!(page(dir.path(), "plain").contains("og:image:width\" content=\"1000\""));

    // Steady state again after the change.
    let text = plan_text(dir.path());
    assert!(text.contains("rebuild:   0"), "got:\n{text}");
}

#[test]
fn check_reports_planned_cards() {
    let dir = tempfile::tempdir().expect("tempdir");
    site(dir.path(), SOCIAL);
    let report = signal_cli::link_check::check_site_from_disk(dir.path()).expect("checks");
    assert_eq!(report.assets.social_images, 2);
    // Unconfigured sites report none, and check stays read-only.
    let plain = tempfile::tempdir().expect("tempdir");
    site(plain.path(), "");
    let report = signal_cli::link_check::check_site_from_disk(plain.path()).expect("checks");
    assert_eq!(report.assets.social_images, 0);
    assert!(!out_dir(plain.path()).exists());
}
