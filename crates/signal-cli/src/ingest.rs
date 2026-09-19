//! Content ingestion: discovery → front matter → normalize → freeze.
//!
//! Filesystem access lives here, never in `signal-core`. Ingestion walks each
//! configured collection source directory, parses front matter (YAML `---` or
//! TOML `+++`, via `signal-markdown`), renders the Markdown body to owned
//! data, and stages normalized [`ContentEntry`] values with deterministic
//! [`ContentId`] assignment (sorted `(collection, relative-path)` order, from
//! 1). Drafts are skipped. [`SiteModelBuilder`] validates and freezes.
//!
//! Identity rules: slug = front-matter `slug`, else the source filename
//! stem verbatim (date prefixes are kept); route = collection `route_prefix`
//! (else `/<collection>/`) + slug + `/`; front-matter `topics` merge into
//! `tags`.

use signal_core::{
    CollectionId, ContentEntry, ContentId, Route, SignalConfig, SiteModel, SiteModelBuilder, Slug,
    SourceRef,
};
use signal_markdown::{parse_markdown, split_front_matter};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::discover::discover_markdown_sources;
use crate::errors::{read_file, BuildError};

/// One discovered source file bound to its collection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFile {
    collection: CollectionId,
    /// Path relative to the site root, with `/` separators.
    root_relative: String,
    /// Absolute path on disk.
    absolute: PathBuf,
    /// Path relative to the collection source dir, with `/` separators.
    collection_relative: String,
}

/// Discover all Markdown sources for every configured collection.
///
/// Sources are sorted by `(collection, collection-relative-path)` so
/// [`ContentId`] assignment is deterministic.
fn discover_collection_sources(
    root: &Path,
    config: &SignalConfig,
) -> Result<Vec<SourceFile>, BuildError> {
    if config.collections.is_empty() {
        // Unconfigured fallback: the default `content/` layout with a single
        // `posts` collection. Keeps the minimal fixture working without
        // requiring a `[collections.*]` table.
        return Ok(discover_markdown_sources(&root.join("content"))?
            .into_iter()
            .map(|absolute| SourceFile {
                collection: CollectionId::new("posts"),
                root_relative: path_relative_unix(root, &absolute),
                collection_relative: path_relative_unix(&root.join("content"), &absolute),
                absolute,
            })
            .collect());
    }

    let mut out = Vec::new();
    let mut names: Vec<&String> = config.collections.keys().collect();
    names.sort();
    for name in names {
        let collection = CollectionId::new(name.clone());
        let dir = root.join(config.source_dir_for(name));
        for absolute in discover_markdown_sources(&dir)? {
            out.push(SourceFile {
                collection: collection.clone(),
                root_relative: path_relative_unix(root, &absolute),
                collection_relative: path_relative_unix(&dir, &absolute),
                absolute,
            });
        }
    }
    out.sort_by(|a, b| {
        (&a.collection, &a.collection_relative).cmp(&(&b.collection, &b.collection_relative))
    });
    Ok(out)
}

pub(crate) fn path_relative_unix(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_else(|_| path.display().to_string())
}

/// Normalize one source file into a [`ContentEntry`], or `None` for drafts.
fn normalize_source(
    id: ContentId,
    source: &SourceFile,
    config: &SignalConfig,
) -> Result<Option<ContentEntry>, BuildError> {
    let text = read_file(&source.absolute)?;
    let origin = source.absolute.display().to_string();
    let (fm, body_markdown) = split_front_matter(&text).map_err(|e| BuildError::FrontMatter {
        path: origin.clone(),
        src: miette::NamedSource::new(origin.clone(), text.clone()),
        message: e.to_string(),
    })?;
    if fm.draft {
        return Ok(None);
    }
    let title = fm
        .title
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| BuildError::Content {
            path: origin.clone(),
            src: miette::NamedSource::new(origin.clone(), text.clone()),
            message: "missing required front-matter field \"title\"".to_string(),
        })?;

    let stem = Path::new(&source.collection_relative)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string());
    // Branch bundles (`_index.md` / `index.md`) address the collection root
    // itself — the Hugo section behavior the migration relies on — rather
    // than a literal `_index` slug.
    let is_section_root = stem == "_index" || stem == "index";
    let slug = fm.slug.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        if is_section_root {
            String::new()
        } else {
            stem
        }
    });
    let prefix = config.route_prefix_for(source.collection.0.as_str());
    // Slugs are single route segments: reject traversal (`.`/`..`),
    // separators, and whitespace/control characters with a content
    // diagnostic instead of silently building an unsafe route.
    if !slug.is_empty() {
        if let Err(reason) = signal_core::validate_route_segment(&slug) {
            return Err(BuildError::Content {
                path: origin.clone(),
                src: miette::NamedSource::new(origin, text),
                message: format!("invalid slug {slug:?}: {reason}"),
            });
        }
    }
    let route = join_route(&prefix, &slug);

    let mut entry = ContentEntry::new(
        id,
        source.collection.clone(),
        SourceRef::new(
            source.collection.clone(),
            source.collection_relative.clone(),
        ),
        Slug::new(slug),
        Route::new(route),
        title,
    );
    entry.description = fm.description.filter(|d| !d.trim().is_empty());
    entry.date = normalize_date(fm.date.as_deref(), "date", &origin, &text)?;
    entry.last_modified = normalize_date(fm.lastmod.as_deref(), "lastmod", &origin, &text)?;
    entry.author = fm.author.filter(|a| !a.trim().is_empty());
    entry.image = normalize_image(fm.image.as_deref(), &origin, &text)?;
    entry.image_alt = fm.image_alt.filter(|a| !a.trim().is_empty());
    entry.body = parse_markdown(body_markdown);
    // Canonical sorted identity for indexing plus the authored display
    // order for templates; both derive from the same ordered, deduplicated
    // front-matter list.
    entry.tag_order = fm.tags.clone();
    entry.tags = fm.tags.into_iter().collect::<BTreeSet<_>>();
    entry.featured = fm.featured;
    entry.section_root = is_section_root;
    // Site-specific fields survive verbatim (e.g. `toc`, `repo`, `math`);
    // templates read them from the `extra` context key.
    entry.extra = fm.extra;
    Ok(Some(entry))
}

/// Normalize an optional `YYYY-MM-DD` front-matter date: empty stays absent,
/// valid dates pass through, anything else is a content error naming the
/// field. The model stores machine-readable dates only; presentation
/// formatting happens at the render boundary.
fn normalize_date(
    value: Option<&str>,
    field: &str,
    origin: &str,
    text: &str,
) -> Result<Option<String>, BuildError> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if signal_core::parse_ymd(v).is_some() => Ok(Some(v.to_string())),
        Some(v) => Err(BuildError::Content {
            path: origin.to_string(),
            src: miette::NamedSource::new(origin, text.to_string()),
            message: format!("invalid {field} {v:?}: expected YYYY-MM-DD"),
        }),
    }
}

/// Normalize an optional front-matter image reference to its site-root URL
/// form. Unsafe references (`javascript:`, `data:`, …) fail the build: image
/// URLs flow into markup and metadata unattended.
fn normalize_image(
    value: Option<&str>,
    origin: &str,
    text: &str,
) -> Result<Option<String>, BuildError> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => signal_core::resolve_image_url(v)
            .map(Some)
            .ok_or_else(|| BuildError::Content {
                path: origin.to_string(),
                src: miette::NamedSource::new(origin, text.to_string()),
                message: format!("unsafe image reference {v:?}"),
            }),
    }
}

fn join_route(prefix: &str, slug: &str) -> String {
    let mut route = String::from("/");
    route.push_str(prefix.trim_matches('/'));
    if !route.ends_with('/') {
        route.push('/');
    }
    let slug = slug.trim_matches('/');
    if !slug.is_empty() {
        route.push_str(slug);
        route.push('/');
    }
    route.replace("//", "/")
}

/// Ingest a site root into a frozen [`SiteModel`].
///
/// Drafts are skipped. Returns the model plus the number of skipped drafts
/// (useful for build summaries).
pub fn ingest_site(root: &Path, config: &SignalConfig) -> Result<(SiteModel, usize), BuildError> {
    // Collection route prefixes become route (and output path) components;
    // validate them before any entry is normalized.
    for name in config.collection_ids() {
        let prefix = config.route_prefix_for(name.0.as_str());
        if let Err(reason) = signal_core::validate_route(&prefix) {
            return Err(BuildError::Model {
                message: format!(
                    "collection {:?} has an invalid route_prefix {prefix:?}: {reason}",
                    name.0
                ),
            });
        }
    }
    let sources = discover_collection_sources(root, config)?;
    // Git-derived last_modified is opt-in ([git] last_modified = true) and
    // advisory: explicit front-matter `lastmod` always wins, and Git
    // unavailability degrades to no derived dates.
    let git_dates = if config.git_last_modified() {
        crate::git::last_modified_dates(root)
    } else {
        std::collections::BTreeMap::new()
    };
    let mut builder = SiteModelBuilder::new();
    let mut skipped_drafts = 0;
    let mut next_id: u32 = 1;
    for source in &sources {
        #[allow(clippy::cast_possible_truncation)]
        let id = ContentId(next_id);
        match normalize_source(id, source, config)? {
            Some(mut entry) => {
                if entry.last_modified.is_none() && !git_dates.is_empty() {
                    if let Some(date) = git_dates.get(&source.root_relative) {
                        entry.last_modified = Some(date.clone());
                    }
                }
                builder.add_entry(entry);
                next_id += 1;
            }
            None => skipped_drafts += 1,
        }
    }
    let model = builder.build().map_err(|e| BuildError::Model {
        message: e.to_string(),
    })?;
    Ok((model, skipped_drafts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_site(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, content).expect("write");
        }
    }

    fn config_with(posts_prefix: &str) -> SignalConfig {
        let toml = format!(
            "[site]\ntitle = \"T\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"{posts_prefix}\"\n"
        );
        SignalConfig::from_toml_str(&toml).expect("config parses")
    }

    #[test]
    fn ingests_yaml_post_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/hello.md",
                "---\ntitle: Hello\nslug: hello\ndescription: Hi there.\ndate: 2026-09-01\ntopics: [B, A]\n---\n\n# Hello\n\nBody text.\n",
            )],
        );
        let (model, drafts) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        assert_eq!(drafts, 0);
        assert_eq!(model.len(), 1);
        let entry = model
            .lookup_by_route(&Route::new("/posts/hello/"))
            .expect("route");
        assert_eq!(entry.title, "Hello");
        assert_eq!(entry.description.as_deref(), Some("Hi there."));
        assert_eq!(entry.date.as_deref(), Some("2026-09-01"));
        assert!(entry.body.html.contains("Body text."));
        assert_eq!(entry.body.word_count, 4);
        let tagged: Vec<u32> = model.entries_tagged("A").iter().map(|e| e.id.0).collect();
        assert_eq!(tagged, vec![entry.id.0]);
    }

    #[test]
    fn authored_topic_order_survives_ingestion() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/ordered.md",
                "---\ntitle: Ordered\ntopics: [Zulu, Alpha]\ntags: [Mike, Alpha]\n---\n\nBody.\n",
            )],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        let entry = model
            .lookup_by_route(&Route::new("/posts/ordered/"))
            .expect("route");
        // Display order: topics first as encountered, then tags, repeats
        // collapsed to first position — never sorted.
        assert_eq!(
            entry.tag_order,
            vec!["Zulu".to_string(), "Alpha".to_string(), "Mike".to_string()]
        );
        // Canonical identity stays sorted for indexing and queries.
        assert_eq!(
            entry.tags.iter().cloned().collect::<Vec<_>>(),
            vec!["Alpha".to_string(), "Mike".to_string(), "Zulu".to_string()]
        );
        // Every term still indexes the entry.
        for term in ["Zulu", "Alpha", "Mike"] {
            assert_eq!(model.entries_tagged(term).len(), 1, "term {term}");
        }
    }

    #[test]
    fn arbitrary_front_matter_survives_ingestion() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "content/posts/a.md",
                    "---\ntitle: A\nrepo: https://example.com/r\ntoc: true\nnested:\n  depth: 2\n---\n\nA.\n",
                ),
                ("content/posts/b.md", "---\ntitle: B\ntopics: [Rust]\n---\n\nB.\n"),
            ],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        let a = model
            .lookup_by_route(&Route::new("/posts/a/"))
            .expect("entry");
        // Unknown fields arrive verbatim, whatever their shape.
        assert_eq!(
            a.extra.get("repo").and_then(|v| v.as_str()),
            Some("https://example.com/r")
        );
        assert_eq!(a.extra.get("toc"), Some(&serde_json::Value::Bool(true)));
        assert_eq!(
            a.extra.get("nested").and_then(|v| v.get("depth")),
            Some(&serde_json::Value::from(2))
        );
        // Typed/known fields never leak into `extra`.
        for key in [
            "title",
            "description",
            "date",
            "topics",
            "tags",
            "draft",
            "featured",
        ] {
            assert!(
                !a.extra.contains_key(key),
                "typed key {key} leaked to extra"
            );
        }
        // An entry without extras keeps the map empty.
        let b = model
            .lookup_by_route(&Route::new("/posts/b/"))
            .expect("entry");
        assert!(b.extra.is_empty(), "{:?}", b.extra);
    }

    #[test]
    fn git_last_modified_fills_gaps_and_front_matter_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_site(
            root,
            &[
                (
                    "content/posts/explicit.md",
                    "---\ntitle: Explicit\nlastmod: 2020-01-01\n---\n\nE.\n",
                ),
                (
                    "content/posts/derived.md",
                    "---\ntitle: Derived\n---\n\nD.\n",
                ),
            ],
        );
        let git = |args: &[&str], date: Option<&str>| {
            let mut cmd = std::process::Command::new("git");
            cmd.arg("-C")
                .arg(root)
                .arg("-c")
                .arg("user.name=Signal Test")
                .arg("-c")
                .arg("user.email=test@example.com")
                .args(args);
            if let Some(date) = date {
                cmd.env("GIT_AUTHOR_DATE", date);
                cmd.env("GIT_COMMITTER_DATE", date);
            }
            let output = cmd.output().expect("git runs");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"], None);
        git(&["add", "-A"], None);
        git(
            &["commit", "-m", "initial"],
            Some("2026-05-05T00:00:00 +0000"),
        );

        let toml = "[site]\ntitle = \"T\"\n[git]\nlast_modified = true\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n";
        let config = SignalConfig::from_toml_str(toml).expect("config parses");
        let (model, _) = ingest_site(root, &config).expect("ingests");
        // Explicit front-matter lastmod wins over the Git-derived date.
        let explicit = model
            .lookup_by_route(&Route::new("/posts/explicit/"))
            .expect("entry");
        assert_eq!(explicit.last_modified.as_deref(), Some("2020-01-01"));
        // Git fills the gap when no lastmod is given.
        let derived = model
            .lookup_by_route(&Route::new("/posts/derived/"))
            .expect("entry");
        assert_eq!(derived.last_modified.as_deref(), Some("2026-05-05"));

        // Without the [git] table, nothing is *derived*: the front-matter
        // lastmod still stands, the gap stays empty.
        let (plain, _) = ingest_site(root, &config_with("/posts/")).expect("ingests");
        let explicit_plain = plain
            .lookup_by_route(&Route::new("/posts/explicit/"))
            .expect("entry");
        assert_eq!(explicit_plain.last_modified.as_deref(), Some("2020-01-01"));
        let derived_plain = plain
            .lookup_by_route(&Route::new("/posts/derived/"))
            .expect("entry");
        assert!(
            derived_plain.last_modified.is_none(),
            "derived.md must stay Git-free without [git]"
        );
    }

    #[test]
    fn slug_defaults_to_filename_stem_and_keeps_date_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/2026-09-02-my-post.md",
                "---\ntitle: Dated\n---\n\nText.\n",
            )],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/articles/")).expect("ingests");
        assert!(model
            .lookup_by_route(&Route::new("/articles/2026-09-02-my-post/"))
            .is_some());
    }

    #[test]
    fn index_files_address_the_collection_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "content/posts/_index.md",
                    "---\ntitle: Posts\n---\n\nIndex.\n",
                ),
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n"),
            ],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        assert_eq!(model.len(), 2);
        let index = model
            .lookup_by_route(&Route::new("/posts/"))
            .expect("section root");
        assert_eq!(index.title, "Posts");
    }

    #[test]
    fn drafts_are_skipped_and_ids_stay_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                ("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n"),
                (
                    "content/posts/b.md",
                    "---\ntitle: B\ndraft: true\n---\n\nB.\n",
                ),
                ("content/posts/c.md", "---\ntitle: C\n---\n\nC.\n"),
            ],
        );
        let (model, drafts) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        assert_eq!(drafts, 1);
        let ids: Vec<u32> = model.entries().map(|e| e.id.0).collect();
        assert_eq!(ids, vec![1, 2]);
        assert!(model.lookup_by_route(&Route::new("/posts/b/")).is_none());
    }

    #[test]
    fn missing_title_is_a_content_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[("content/posts/x.md", "---\ntags: [a]\n---\n\nX.\n")],
        );
        let err = ingest_site(dir.path(), &config_with("/posts/")).expect_err("fails");
        assert!(matches!(err, BuildError::Content { .. }));
    }

    #[test]
    fn toml_front_matter_ingests() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/t.md",
                "+++\ntitle = \"TOML Post\"\ndate = 2026-09-03\n+++\n\nText.\n",
            )],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        assert_eq!(model.len(), 1);
    }

    #[test]
    fn route_collisions_surface_as_model_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[
                (
                    "content/posts/a.md",
                    "---\ntitle: A\nslug: same\n---\n\nA.\n",
                ),
                (
                    "content/posts/b.md",
                    "---\ntitle: B\nslug: same\n---\n\nB.\n",
                ),
            ],
        );
        let err = ingest_site(dir.path(), &config_with("/posts/")).expect_err("collides");
        assert!(matches!(err, BuildError::Model { .. }));
    }

    #[test]
    fn metadata_fields_promote_and_validate() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/a.md",
                "---\ntitle: A\ndate: 2026-09-02\nlastmod: 2026-09-10\nauthor: Guest\nimage: images/a.svg\nimage_alt: Alt\n---\n\nA.\n",
            )],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        let entry = model
            .lookup_by_route(&Route::new("/posts/a/"))
            .expect("route");
        assert_eq!(entry.date.as_deref(), Some("2026-09-02"));
        assert_eq!(entry.last_modified.as_deref(), Some("2026-09-10"));
        assert_eq!(entry.author.as_deref(), Some("Guest"));
        assert_eq!(entry.image.as_deref(), Some("/images/a.svg"));
        assert_eq!(entry.image_alt.as_deref(), Some("Alt"));
    }

    #[test]
    fn invalid_dates_are_content_errors() {
        for front_matter in [
            "---\ntitle: A\ndate: yesterday\n---\n\nA.\n",
            "---\ntitle: A\ndate: 2026-02-30\n---\n\nA.\n",
            "---\ntitle: A\nlastmod: 2026-13-01\n---\n\nA.\n",
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            write_site(dir.path(), &[("content/posts/a.md", front_matter)]);
            let err = ingest_site(dir.path(), &config_with("/posts/")).expect_err("fails");
            assert!(matches!(err, BuildError::Content { .. }), "got: {err:?}");
        }
    }

    #[test]
    fn unsafe_image_references_are_content_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/a.md",
                "---\ntitle: A\nimage: \"javascript:alert(1)\"\n---\n\nA.\n",
            )],
        );
        let err = ingest_site(dir.path(), &config_with("/posts/")).expect_err("fails");
        assert!(matches!(err, BuildError::Content { .. }), "got: {err:?}");
    }

    #[test]
    fn unsafe_slugs_are_rejected() {
        for bad_slug in ["../../evil", "..", ".", "a/b", "a\\b", "a b", "a/../.."] {
            let dir = tempfile::tempdir().expect("tempdir");
            write_site(
                dir.path(),
                &[(
                    "content/posts/x.md",
                    &format!("---\ntitle: X\nslug: \"{bad_slug}\"\n---\n\nX.\n"),
                )],
            );
            let err = ingest_site(dir.path(), &config_with("/posts/"))
                .expect_err(&format!("slug {bad_slug:?} must be rejected"));
            assert!(
                matches!(err, BuildError::Content { .. }),
                "slug {bad_slug:?}: got {err:?}"
            );
        }
    }

    #[test]
    fn traversal_via_filename_stem_is_rejected() {
        // A source named `...md` yields the filename stem `..`.
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[("content/posts/...md", "---\ntitle: X\n---\n\nX.\n")],
        );
        let err = ingest_site(dir.path(), &config_with("/posts/")).expect_err("fails");
        assert!(matches!(err, BuildError::Content { .. }), "got: {err:?}");
    }

    #[test]
    fn unsafe_route_prefixes_are_rejected() {
        for bad_prefix in ["/posts/../x/", "/a\\b/", "/posts a/"] {
            let dir = tempfile::tempdir().expect("tempdir");
            write_site(
                dir.path(),
                &[("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n")],
            );
            let err = ingest_site(dir.path(), &config_with(bad_prefix))
                .expect_err(&format!("prefix {bad_prefix:?} must be rejected"));
            assert!(
                matches!(err, BuildError::Model { .. }),
                "prefix {bad_prefix:?}: got {err:?}"
            );
        }
        // A missing leading slash is repaired by long-standing normalization
        // (route_prefix_for), so it is not rejected.
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[("content/posts/a.md", "---\ntitle: A\n---\n\nA.\n")],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("posts")).expect("ingests");
        assert!(model.lookup_by_route(&Route::new("/posts/a/")).is_some());
    }

    #[test]
    fn unicode_and_hyphenated_slugs_still_work() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_site(
            dir.path(),
            &[(
                "content/posts/über-alles.md",
                "---\ntitle: Ü\n---\n\nText.\n",
            )],
        );
        let (model, _) = ingest_site(dir.path(), &config_with("/posts/")).expect("ingests");
        assert!(model
            .lookup_by_route(&Route::new("/posts/über-alles/"))
            .is_some());
    }
}
