//! Structured metadata projections (JSON-LD).
//!
//! Builders take only available values and omit everything else: no
//! fabricated authorship, dates, or images. Output is produced with
//! `serde_json` (never string concatenation) and `</` sequences are escaped
//! to `<\/` so the payload cannot break out of its `<script>` element.
//! Templates insert the result with `| safe`; the escaping is what makes
//! that sound.

use serde_json::{Map, Value};

fn escape_script(value: String) -> String {
    value.replace("</", "<\\/")
}

/// Article structured data.
///
/// `headline` is required; every other field is included only when `Some`.
/// `author` becomes `{"@type": "Person", "name": …}` when known.
#[allow(clippy::too_many_arguments)]
pub fn article_json_ld(
    headline: &str,
    description: Option<&str>,
    date_published: Option<&str>,
    date_modified: Option<&str>,
    url: Option<&str>,
    image: Option<&str>,
    author: Option<&str>,
) -> String {
    let mut obj = Map::with_capacity(8);
    obj.insert("@context".to_string(), Value::from("https://schema.org"));
    obj.insert("@type".to_string(), Value::from("Article"));
    obj.insert("headline".to_string(), Value::from(headline));
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        obj.insert("description".to_string(), Value::from(description));
    }
    if let Some(date) = date_published {
        obj.insert("datePublished".to_string(), Value::from(date));
    }
    if let Some(date) = date_modified {
        obj.insert("dateModified".to_string(), Value::from(date));
    }
    if let Some(url) = url {
        obj.insert("url".to_string(), Value::from(url));
    }
    if let Some(image) = image {
        obj.insert("image".to_string(), Value::from(image));
    }
    if let Some(author) = author.filter(|a| !a.trim().is_empty()) {
        let mut person = Map::with_capacity(2);
        person.insert("@type".to_string(), Value::from("Person"));
        person.insert("name".to_string(), Value::from(author));
        obj.insert("author".to_string(), Value::from(person));
    }
    escape_script(Value::from(obj).to_string())
}

/// Generic page structured data for section, term, index, and home pages.
pub fn webpage_json_ld(name: &str, url: Option<&str>, description: Option<&str>) -> String {
    let mut obj = Map::with_capacity(5);
    obj.insert("@context".to_string(), Value::from("https://schema.org"));
    obj.insert("@type".to_string(), Value::from("WebPage"));
    obj.insert("name".to_string(), Value::from(name));
    if let Some(url) = url {
        obj.insert("url".to_string(), Value::from(url));
    }
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        obj.insert("description".to_string(), Value::from(description));
    }
    escape_script(Value::from(obj).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_omits_missing_fields() {
        let json = article_json_ld("Hi", None, None, None, None, None, None);
        let value: Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["@type"], Value::from("Article"));
        assert_eq!(value["headline"], Value::from("Hi"));
        assert!(value.get("description").is_none());
        assert!(value.get("author").is_none());
        assert!(value.get("image").is_none());
    }

    #[test]
    fn article_includes_available_metadata() {
        let json = article_json_ld(
            "Hi",
            Some("Desc"),
            Some("2026-09-02"),
            Some("2026-09-10"),
            Some("https://example.com/a/"),
            Some("https://example.com/i.svg"),
            Some("Guest Writer"),
        );
        let value: Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["datePublished"], Value::from("2026-09-02"));
        assert_eq!(value["dateModified"], Value::from("2026-09-10"));
        assert_eq!(value["author"]["name"], Value::from("Guest Writer"));
        assert_eq!(value["author"]["@type"], Value::from("Person"));
    }

    #[test]
    fn script_breakout_is_escaped_but_json_stays_valid() {
        let json = article_json_ld(
            "A </script><script>alert(1)",
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(!json.contains("</script>"), "got: {json}");
        assert!(json.contains("<\\/script>"), "got: {json}");
        let value: Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            value["headline"],
            Value::from("A </script><script>alert(1)")
        );
    }

    #[test]
    fn webpage_carries_name_url_description() {
        let json = webpage_json_ld("Posts", Some("https://example.com/posts/"), Some("Desc"));
        let value: Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["@type"], Value::from("WebPage"));
        assert_eq!(value["url"], Value::from("https://example.com/posts/"));
    }
}
