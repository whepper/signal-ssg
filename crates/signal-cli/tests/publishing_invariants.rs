//! Publishing-pipeline invariants (A1–A5 architecture review).
//!
//! These are architectural assertions, not behaviour snapshots: they hold
//! for any site configuration and would catch a divergence between what
//! planning declares and what rendering emits. Each one is stated as a
//! single property over one built site.
//!
//! 1. Every URL rendered into HTML resolves to a planned artifact.
//! 2. Every `srcset` width descriptor equals the generated file's width.
//! 3. Social metadata matches the generated card.
//! 4. `check`, `--explain`, and the build agree on the artifact plan.

use image::ImageEncoder as _;
use signal_cli::build::build_site_from_disk;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn write_site(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }
}

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

const TEMPLATE: &str = concat!(
    "<html><head>",
    "{% if og_image %}<meta property=\"og:image\" content=\"{{ og_image }}\">{% endif %}",
    "{% if twitter_image %}<meta name=\"twitter:image\" content=\"{{ twitter_image }}\">{% endif %}",
    "{% if social_image %}<meta property=\"og:image:width\" content=\"{{ social_image.width }}\">",
    "<meta property=\"og:image:height\" content=\"{{ social_image.height }}\">{% endif %}",
    "</head><body>",
    "{% if responsive_image %}<picture>",
    "{% for source in responsive_image.sources %}",
    "<source type=\"{{ source.mime }}\" srcset=\"{{ source.srcset }}\" />",
    "{% endfor %}",
    "<img src=\"{{ responsive_image.src }}\" srcset=\"{{ responsive_image.srcset }}\"",
    " sizes=\"{{ responsive_image.sizes }}\" width=\"{{ responsive_image.width }}\"",
    " height=\"{{ responsive_image.height }}\" alt=\"{{ responsive_image.alt }}\" />",
    "</picture>{% endif %}",
    "{{ content | safe }}</body></html>",
);

/// One site exercising every generated artifact family at once: source
/// assets, WebP + AVIF derivatives, responsive body/hero markup, and a
/// social card. The hero source is deliberately larger than the requested
/// widths so clamping is in play.
fn publishing_site(dir: &Path) {
    write_site(
        dir,
        &[
            (
                "signal.toml",
                "[site]\ntitle = \"Signal\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[images]\nwidths = [80, 320]\nformats = [\"avif\", \"webp\"]\n[social]\n",
            ),
            (
                "content/posts/hero.md",
                "---\ntitle: Hero story\ndescription: A description.\nimage: images/hero.png\n---\n\nBody ![inline](/images/hero.png) image.\n",
            ),
            ("templates/post.html", TEMPLATE),
            (
                "templates/section.html",
                "<html><body>section {{ content | safe }}</body></html>",
            ),
        ],
    );
    let path = dir.join("static/images/hero.png");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, gradient_png(160, 90)).expect("write");
}

fn build(dir: &Path) -> (PathBuf, signal_cli::build::BuildSummary) {
    let out = dir.join("out");
    let summary = build_site_from_disk(dir, &out).expect("builds");
    (out, summary)
}

/// MiniJinja escapes `/` in interpolated values; entity-decode so the
/// invariants compare real URLs.
fn decode(html: &str) -> String {
    html.replace("&#x2f;", "/")
}

/// Every root-relative URL in `attribute="…"` values, in document order.
///
/// `srcset` entries are `<url> <descriptor>` pairs separated by commas;
/// plain attributes hold one URL. Only whole attribute names match (so
/// `data-src` is not read as `src`).
fn urls_in(html: &str, attribute: &str) -> Vec<String> {
    let needle = format!("{attribute}=\"");
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(index) = rest.find(&needle) {
        // Reject partial matches such as `data-src="` for `src="`.
        let boundary = rest[..index].chars().next_back();
        rest = &rest[index + needle.len()..];
        if !matches!(boundary, None | Some(' ') | Some('<')) {
            continue;
        }
        let Some(end) = rest.find('"') else { break };
        let value = &rest[..end];
        rest = &rest[end..];
        for candidate in value.split(',') {
            let url = candidate.split_whitespace().next().unwrap_or("");
            if url.starts_with('/') && !url.starts_with("//") {
                urls.push(url.to_string());
            }
        }
    }
    urls
}

/// The `content` of the `<meta>` tag carrying `key` (`og:image`,
/// `twitter:image`), entity-decoded by the caller.
fn meta_url(html: &str, key: &str) -> Option<String> {
    let needle = format!("{key}\" content=\"");
    let index = html.find(&needle)?;
    let rest = &html[index + needle.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Read an AVIF's primary image dimensions from its `ispe` box.
///
/// Test-only: the workspace's `image` dependency deliberately enables no
/// AVIF decoder (nothing reads generated AVIF back), so the container is
/// parsed directly. An `ispe` box is 20 bytes; its payload is the
/// big-endian width and height.
fn avif_dimensions(bytes: &[u8]) -> (u32, u32) {
    let mut index = 0;
    while index + 16 <= bytes.len() {
        if &bytes[index..index + 4] == b"ispe"
            && index >= 4
            && u32::from_be_bytes(bytes[index - 4..index].try_into().expect("4 bytes")) == 20
        {
            let width = u32::from_be_bytes(bytes[index + 8..index + 12].try_into().expect("4"));
            let height = u32::from_be_bytes(bytes[index + 12..index + 16].try_into().expect("4"));
            return (width, height);
        }
        index += 1;
    }
    panic!("no ispe box in AVIF payload");
}

/// Every HTML file under `out`, in sorted order.
fn html_files(out: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walk(out)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "html"))
        .collect();
    files.sort();
    files
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return out,
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            out.extend(walk(&child));
        } else {
            out.push(child);
        }
    }
    out
}

#[test]
fn every_rendered_reference_resolves_to_a_planned_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    publishing_site(dir.path());
    let (out, summary) = build(dir.path());
    let planned: BTreeSet<&str> = summary
        .specs
        .iter()
        .map(|spec| spec.path.as_str())
        .collect();

    let mut checked = 0;
    for file in html_files(&out) {
        let html = decode(&std::fs::read_to_string(&file).expect("html"));
        for attribute in ["src", "srcset", "content"] {
            for url in urls_in(&html, attribute) {
                let relative = url.trim_start_matches('/');
                checked += 1;
                assert!(
                    planned.contains(relative),
                    "{attribute} {url:?} in {} is not a planned artifact",
                    file.display()
                );
                assert!(
                    out.join(relative).is_file(),
                    "{attribute} {url:?} in {} was not written",
                    file.display()
                );
            }
        }
    }
    // The fixture must actually exercise generated references, or the
    // invariant proves nothing.
    assert!(
        checked >= 6,
        "expected several generated references, got {checked}"
    );
}

#[test]
fn advertised_srcset_widths_match_the_generated_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    publishing_site(dir.path());
    let (out, _) = build(dir.path());

    let mut descriptors = 0;
    let mut formats: BTreeSet<&str> = BTreeSet::new();
    for file in html_files(&out) {
        let html = decode(&std::fs::read_to_string(&file).expect("html"));
        let needle = "srcset=\"";
        let mut rest = html.as_str();
        while let Some(index) = rest.find(needle) {
            rest = &rest[index + needle.len()..];
            let Some(end) = rest.find('"') else { break };
            for candidate in rest[..end].split(',') {
                let mut parts = candidate.split_whitespace();
                let (Some(url), Some(descriptor)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let width: u32 = descriptor
                    .strip_suffix('w')
                    .expect("width descriptor")
                    .parse()
                    .expect("numeric width");
                let relative = url.trim_start_matches('/');
                let bytes = std::fs::read(out.join(relative)).expect("derivative exists");
                let actual = if relative.ends_with(".avif") {
                    formats.insert("avif");
                    avif_dimensions(&bytes)
                } else {
                    formats.insert("webp");
                    let decoded = image::load_from_memory(&bytes).expect("derivative decodes");
                    image::GenericImageView::dimensions(&decoded)
                };
                assert_eq!(
                    actual.0, width,
                    "{relative} advertises {width}w but is not that wide"
                );
                assert!(actual.1 > 0, "{relative} has no height");
                descriptors += 1;
            }
            rest = &rest[end..];
        }
    }
    // Hero + body image, each with two formats × two widths in the
    // `<source>`s plus the WebP fallback `<img>`: every candidate was
    // decoded and matched.
    assert!(
        descriptors >= 12,
        "expected candidates for both rendering paths, got {descriptors}"
    );
    assert_eq!(
        formats.iter().copied().collect::<Vec<_>>(),
        vec!["avif", "webp"],
        "both planned formats must be advertised and verified"
    );
}

#[test]
fn social_metadata_matches_the_generated_card() {
    let dir = tempfile::tempdir().expect("tempdir");
    publishing_site(dir.path());
    let (out, _) = build(dir.path());
    let html = decode(&std::fs::read_to_string(out.join("posts/hero/index.html")).expect("page"));
    let og_image = meta_url(&html, "og:image").expect("og:image present");
    let twitter_image = meta_url(&html, "twitter:image").expect("twitter:image present");
    assert_eq!(og_image, twitter_image, "one card, one URL");
    // Open Graph needs an absolute URL; the site's base URL supplies it.
    let og_image = og_image
        .strip_prefix("https://example.com")
        .expect("absolute card URL under the configured base")
        .to_string();
    assert_eq!(og_image, "/social/posts/hero.png");
    let bytes = std::fs::read(out.join(og_image.trim_start_matches('/'))).expect("card exists");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let decoded = image::load_from_memory(&bytes).expect("card decodes");
    let (width, height) = image::GenericImageView::dimensions(&decoded);
    // The metadata's declared dimensions are the file's real dimensions.
    assert!(html.contains(&format!("og:image:width\" content=\"{width}\"")));
    assert!(html.contains(&format!("og:image:height\" content=\"{height}\"")));
    assert_eq!((width, height), (1200, 630));
}

#[test]
fn check_explain_and_build_agree_on_the_artifact_plan() {
    let dir = tempfile::tempdir().expect("tempdir");
    publishing_site(dir.path());
    let (out, summary) = build(dir.path());
    let from_build: BTreeSet<String> = summary.specs.iter().map(|s| s.path.clone()).collect();

    // `check` validates the same plan without an output directory.
    let check = signal_cli::link_check::check_site_from_disk(dir.path()).expect("checks");
    let from_check: BTreeSet<String> = signal_cli::pipeline::load_validated_check(dir.path())
        .expect("checks")
        .validated
        .specs
        .iter()
        .map(|spec| spec.path.clone())
        .collect();
    assert_eq!(
        from_build, from_check,
        "check and build plan different artifact sets"
    );
    assert_eq!(check.assets.social_images, 1);
    assert_eq!(check.assets.derivatives, 4);

    // `--explain` reports one decision per planned artifact.
    let explained =
        signal_cli::explain::explain_site_from_disk(dir.path(), &out).expect("explains");
    let header = explained
        .lines()
        .find_map(|line| line.strip_prefix("  artifacts: "))
        .expect("artifact count")
        .trim()
        .parse::<usize>()
        .expect("numeric");
    assert_eq!(header, from_build.len());
}
