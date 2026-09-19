//! Front-matter parsing for Signal content files.
//!
//! The Hugo migration proved both delimiters are in live use (`---` YAML and
//! `+++` TOML), so both are supported. Parsing goes through an intermediate
//! `serde_json::Value` so TOML datetimes (e.g. bare `date = 2026-09-03`) and
//! YAML scalars normalize to the same shapes before field extraction.
//!
//! Unknown fields are preserved verbatim in [`FrontMatter::extra`] so later
//! vertical slices (images, TOC, math, author overrides) can consume them
//! without re-parsing.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

/// Front-matter failure (owned message; no parser types leak).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrontMatterError {
    /// Neither a `---` nor a `+++` block could be split off.
    #[error("missing front matter (expected leading --- or +++)")]
    Missing,
    /// The block was found but did not parse.
    #[error("invalid front matter ({format}): {message}")]
    Invalid {
        /// `yaml` or `toml`.
        format: String,
        /// Parser message (owned).
        message: String,
    },
}

/// Supported front-matter delimiter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delimiter {
    /// `---` lines: YAML.
    Yaml,
    /// `+++` lines: TOML.
    Toml,
}

/// Normalized front matter: the vertical-slice subset plus verbatim extras.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrontMatter {
    /// Entry title (`title`, required at ingestion).
    #[serde(default)]
    pub title: Option<String>,
    /// Slug override (`slug`); else the source filename stem.
    #[serde(default)]
    pub slug: Option<String>,
    /// Short summary (`description`).
    #[serde(default)]
    pub description: Option<String>,
    /// Publication date as string (`date`, e.g. `2026-09-02`).
    #[serde(default)]
    pub date: Option<String>,
    /// Taxonomy terms: `topics` (Hugo) and `tags` merged, first-occurrence
    /// deduplicated, in authored order (`topics` first, then `tags`, each as
    /// encountered). Display order for templates; canonical sorted order for
    /// indexing is derived downstream (`ContentEntry.tags`).
    #[serde(default)]
    pub tags: Vec<String>,
    /// `draft = true` excludes the page from the build.
    #[serde(default)]
    pub draft: bool,
    /// `featured = true` marks the entry for the home hero projection.
    #[serde(default)]
    pub featured: bool,
    /// Last-modification date (`lastmod`, same form as `date`).
    #[serde(default)]
    pub lastmod: Option<String>,
    /// Per-entry author override (`author`).
    #[serde(default)]
    pub author: Option<String>,
    /// Hero image reference (`image`).
    #[serde(default)]
    pub image: Option<String>,
    /// Hero image alt text (`image_alt`).
    #[serde(default)]
    pub image_alt: Option<String>,
    /// All other fields, verbatim (e.g. `toc`, `math`, `repo`).
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Split `---`/`+++` front matter off `text`, returning the parsed matter and
/// the remaining Markdown body.
pub fn split_front_matter(text: &str) -> Result<(FrontMatter, &str), FrontMatterError> {
    let delimiter = detect_delimiter(text).ok_or(FrontMatterError::Missing)?;
    let (raw, body) = split_block(text, delimiter).ok_or(FrontMatterError::Missing)?;
    let value = match delimiter {
        Delimiter::Yaml => serde_yaml::from_str::<serde_yaml::Value>(raw)
            .map_err(|e| FrontMatterError::Invalid {
                format: "yaml".to_string(),
                message: e.to_string(),
            })
            .and_then(|v| {
                serde_json::to_value(v).map_err(|e| FrontMatterError::Invalid {
                    format: "yaml".to_string(),
                    message: e.to_string(),
                })
            })?,
        Delimiter::Toml => toml::from_str::<toml::Value>(raw)
            .map_err(|e| FrontMatterError::Invalid {
                format: "toml".to_string(),
                message: e.to_string(),
            })
            .and_then(|v| {
                serde_json::to_value(v).map_err(|e| FrontMatterError::Invalid {
                    format: "toml".to_string(),
                    message: e.to_string(),
                })
            })?,
    };
    let fm = front_matter_from_value(&value).map_err(|message| FrontMatterError::Invalid {
        format: match delimiter {
            Delimiter::Yaml => "yaml".to_string(),
            Delimiter::Toml => "toml".to_string(),
        },
        message,
    })?;
    Ok((fm, body))
}

fn detect_delimiter(text: &str) -> Option<Delimiter> {
    let first = text.lines().next()?.trim();
    match first {
        "---" => Some(Delimiter::Yaml),
        "+++" => Some(Delimiter::Toml),
        _ => None,
    }
}

fn split_block(text: &str, delimiter: Delimiter) -> Option<(&str, &str)> {
    let marker = match delimiter {
        Delimiter::Yaml => "---",
        Delimiter::Toml => "+++",
    };
    let mut lines = text.split_inclusive('\n');
    let first = lines.next()?;
    debug_assert!(first.trim() == marker);
    let mut raw_end = first.len();
    for line in lines {
        raw_end += line.len();
        if line.trim() == marker {
            let raw = &text[first.len()..raw_end - line.len()];
            let body = &text[raw_end..];
            return Some((raw, body));
        }
    }
    None
}

/// Recursively collapse TOML-datetime marker maps to plain strings so
/// `extra` values are directly usable downstream.
fn normalize_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            if map.len() == 1 {
                if let Some(serde_json::Value::String(s)) = map.get("$__toml_private_datetime") {
                    return serde_json::Value::String(s.clone());
                }
            }
            serde_json::Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, normalize_value(v)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(normalize_value).collect())
        }
        scalar => scalar,
    }
}

fn front_matter_from_value(value: &serde_json::Value) -> Result<FrontMatter, String> {
    let get = |key: &str| value.get(key);
    let opt_string = |key: &str| -> Result<Option<String>, String> {
        match get(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
            // TOML datetimes (e.g. bare `date = 2026-09-03`) serialize to
            // `{"$__toml_private_datetime": "..."}` via `serde_json::to_value`.
            Some(serde_json::Value::Object(map)) => {
                if map.len() == 1 {
                    if let Some(serde_json::Value::String(s)) = map.get("$__toml_private_datetime")
                    {
                        return Ok(Some(s.clone()));
                    }
                }
                Err(format!("field {key:?} must be a string"))
            }
            Some(_) => Err(format!("field {key:?} must be a string")),
        }
    };
    let string_list = |key: &str| -> Result<Vec<String>, String> {
        match get(key) {
            None | Some(serde_json::Value::Null) => Ok(Vec::new()),
            Some(serde_json::Value::String(s)) => Ok(vec![s.clone()]),
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("field {key:?} must be a list of strings"))
                })
                .collect(),
            Some(_) => Err(format!("field {key:?} must be a list of strings")),
        }
    };

    let opt_bool = |key: &str| -> Result<bool, String> {
        match get(key) {
            None | Some(serde_json::Value::Null) => Ok(false),
            Some(serde_json::Value::Bool(b)) => Ok(*b),
            Some(_) => Err(format!("field {key:?} must be a boolean")),
        }
    };

    let mut tags = string_list("topics")?;
    tags.extend(string_list("tags")?);
    // First-occurrence dedup preserving authored order: templates render
    // eyebrows and "first topic" picks from this order. Sorted order for
    // taxonomy indexing is derived downstream (a `BTreeSet` sorts anyway).
    let mut seen = HashSet::new();
    tags.retain(|tag| seen.insert(tag.clone()));

    let draft = opt_bool("draft")?;
    let featured = opt_bool("featured")?;

    let mut extra = BTreeMap::new();
    if let Some(serde_json::Value::Object(map)) = value.as_object().map(|_| value) {
        for (key, val) in map {
            match key.as_str() {
                "title" | "slug" | "description" | "date" | "topics" | "tags" | "draft"
                | "featured" | "lastmod" | "author" | "image" | "image_alt" => {}
                _ => {
                    extra.insert(key.clone(), normalize_value(val.clone()));
                }
            }
        }
    }

    Ok(FrontMatter {
        title: opt_string("title")?,
        slug: opt_string("slug")?,
        description: opt_string("description")?,
        date: opt_string("date")?,
        tags,
        draft,
        featured,
        lastmod: opt_string("lastmod")?,
        author: opt_string("author")?,
        image: opt_string("image")?,
        image_alt: opt_string("image_alt")?,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_front_matter() {
        let (fm, body) =
            split_front_matter("---\ntitle: Hi\ntopics: [A, B]\n---\n\nBody.\n").expect("parses");
        assert_eq!(fm.title.as_deref(), Some("Hi"));
        assert_eq!(fm.tags, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(body, "\nBody.\n");
        assert!(!fm.draft);
    }

    #[test]
    fn parses_toml_front_matter_with_bare_date() {
        let text = "+++\ntitle = \"TOML\"\ndate = 2026-09-03\ntopics = [\"Rust\"]\n+++\n\nBody.\n";
        let (fm, body) = split_front_matter(text).expect("parses");
        assert_eq!(fm.title.as_deref(), Some("TOML"));
        assert_eq!(fm.date.as_deref(), Some("2026-09-03"));
        assert_eq!(fm.tags, vec!["Rust".to_string()]);
        assert!(body.contains("Body."));
    }

    #[test]
    fn merges_topics_and_tags_and_keeps_extras() {
        let text =
            "---\ntitle: T\ntopics: [B]\ntags: [A, B]\nrepo: https://example.com/r\ntoc: true\n---\nX\n";
        let (fm, _) = split_front_matter(text).expect("parses");
        assert_eq!(fm.tags, vec!["B".to_string(), "A".to_string()]);
        assert_eq!(
            fm.extra.get("repo").and_then(|v| v.as_str()),
            Some("https://example.com/r")
        );
        assert_eq!(fm.extra.get("toc"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn authored_tag_order_is_preserved_with_first_occurrence_dedup() {
        let text = "---\ntitle: T\ntopics: [Zulu, Alpha, Zulu]\ntags: [Mike, Alpha]\n---\nX\n";
        let (fm, _) = split_front_matter(text).expect("parses");
        // `topics` first as encountered, then `tags`; repeats collapse to
        // their first position, never sorted.
        assert_eq!(
            fm.tags,
            vec!["Zulu".to_string(), "Alpha".to_string(), "Mike".to_string()]
        );
    }

    #[test]
    fn missing_delimiter_is_an_error() {
        assert_eq!(
            split_front_matter("# No front matter\n"),
            Err(FrontMatterError::Missing)
        );
    }

    #[test]
    fn unterminated_block_is_missing() {
        assert_eq!(
            split_front_matter("---\ntitle: T\n"),
            Err(FrontMatterError::Missing)
        );
    }

    #[test]
    fn invalid_yaml_is_reported() {
        let err = split_front_matter("---\ntitle: [unclosed\n---\nBody\n").expect_err("fails");
        assert!(matches!(err, FrontMatterError::Invalid { .. }));
    }

    #[test]
    fn draft_flag_is_honored() {
        let (fm, _) = split_front_matter("---\ntitle: T\ndraft: true\n---\nX\n").expect("parses");
        assert!(fm.draft);
    }

    #[test]
    fn featured_flag_is_typed() {
        let (fm, _) =
            split_front_matter("---\ntitle: T\nfeatured: true\n---\nX\n").expect("parses");
        assert!(fm.featured);
        assert!(!fm.extra.contains_key("featured"));
        let (plain, _) = split_front_matter("---\ntitle: T\n---\nX\n").expect("parses");
        assert!(!plain.featured);
    }

    #[test]
    fn metadata_fields_are_typed_not_extras() {
        let text = "---\ntitle: T\nlastmod: 2026-04-01\nauthor: Guest Writer\nimage: /images/a.svg\nimage_alt: Alt text\n---\nX\n";
        let (fm, _) = split_front_matter(text).expect("parses");
        assert_eq!(fm.lastmod.as_deref(), Some("2026-04-01"));
        assert_eq!(fm.author.as_deref(), Some("Guest Writer"));
        assert_eq!(fm.image.as_deref(), Some("/images/a.svg"));
        assert_eq!(fm.image_alt.as_deref(), Some("Alt text"));
        for key in ["lastmod", "author", "image", "image_alt"] {
            assert!(!fm.extra.contains_key(key), "key {key} leaked to extra");
        }
    }
}
