//! Body HTML `<img>` scanning (shared).
//!
//! Signal's content model carries rendered body HTML (`ContentEntry.body.html`)
//! plus extracted structure, but not every `<img>` detail. Two consumers need
//! to read `<img>` tags out of that HTML:
//!
//! - responsive rewriting (A3, `signal-cli::responsive`) rewrites resolvable
//!   tags to `srcset`/`<picture>` markup;
//! - diagnostics (A6, `signal-cli::diagnostics`) reads `alt` to report images
//!   without alternative text.
//!
//! Both use this one quote-aware scanner, so "what is an `<img>` here" has a
//! single answer. It is a byte-level scanner, not an HTML parser: it only ever
//! stops at ASCII `<`, whitespace, quotes, or `>`, so every slice lands on a
//! UTF-8 boundary. Comrak emits lowercase `<img` and well-formed attributes.

/// One `<img>` element found in body HTML, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BodyImage {
    /// The `src` attribute value as authored (never re-encoded).
    pub(crate) src: String,
    /// The `alt` attribute value, or `None` when the attribute is absent.
    /// Comrak always emits `alt` for Markdown images (empty when the author
    /// wrote none), so `None` only appears for markup Signal did not render.
    pub(crate) alt: Option<String>,
}

/// Every `<img>` with a `src` attribute in `html`, in document order.
pub(crate) fn images(html: &str) -> Vec<BodyImage> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(start) = find_img(bytes, cursor) {
        let (tag, end) = scan_tag(bytes, start);
        let attributes = parse_attributes(tag);
        let value = |name: &str| {
            attributes
                .iter()
                .find(|(attribute, _)| attribute == name)
                .map(|(_, value)| value.clone())
        };
        if let Some(src) = value("src") {
            out.push(BodyImage {
                src,
                alt: value("alt"),
            });
        }
        cursor = end;
    }
    out
}

/// Locate the next `<img` tag open (case-sensitive: Comrak emits
/// lowercase), where the character after `img` starts attributes or ends
/// the tag. Byte offsets; `haystack` is scanned from `from`.
pub(crate) fn find_img(haystack: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 4 <= haystack.len() {
        if &haystack[i..i + 4] == b"<img"
            && haystack
                .get(i + 4)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'/' || *b == b'>')
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Scan one tag: `(tag_text, end_offset)` where `end_offset` is just past
/// the closing `>` (quote-aware, so `>` inside values never ends the
/// tag). Unterminated input yields the remainder without failing.
pub(crate) fn scan_tag(bytes: &[u8], start: usize) -> (&str, usize) {
    let mut i = start;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(open) = quote {
            if byte == open {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            i += 1;
            break;
        }
        i += 1;
    }
    // Tags come from Comrak-rendered HTML: scanning only ever stops at
    // ASCII `<`, whitespace, quotes, or `>`, so the byte range always
    // lands on UTF-8 boundaries.
    (slice_str(bytes, start, i), i)
}

/// Borrow a byte range as `&str`, degrading to empty on a non-boundary.
pub(crate) fn slice_str(bytes: &[u8], start: usize, end: usize) -> &str {
    std::str::from_utf8(&bytes[start..end]).unwrap_or("")
}

/// Parsed attributes of one `<img>` tag: `(name, raw_value)` in document
/// order, where `raw_value` excludes surrounding quotes. Valueless
/// attributes carry an empty value. Attribute names are lowercased.
pub(crate) fn parse_attributes(tag: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let bytes = tag.as_bytes();
    // Skip `<img`.
    let mut i = 4;
    while i < bytes.len() {
        while i < bytes.len()
            && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/' || bytes[i] == b'>')
        {
            if bytes[i] == b'>' {
                return attributes;
            }
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' {
            break;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
            && bytes[i] != b'>'
        {
            i += 1;
        }
        let name = slice_str(bytes, name_start, i).to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if let Some(&quote) = bytes.get(i) {
                if quote == b'"' || quote == b'\'' {
                    i += 1;
                    let value_start = i;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    value = slice_str(bytes, value_start, i).to_string();
                    i += 1;
                } else {
                    let value_start = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    value = slice_str(bytes, value_start, i).to_string();
                }
            }
        }
        if name.is_empty() {
            break;
        }
        attributes.push((name, value));
    }
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_images_in_document_order() {
        let html = "<p><img src=\"/a.png\" alt=\"A\" /> and <img src=\"/b.png\" /></p>";
        let images = images(html);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].src, "/a.png");
        assert_eq!(images[0].alt.as_deref(), Some("A"));
        assert_eq!(images[1].src, "/b.png");
        assert_eq!(images[1].alt, None);
    }

    #[test]
    fn ignores_tags_without_src_and_partial_names() {
        // `data-src` is not `src`; a tag without `src` is not an image.
        let html = "<img alt=\"no src\" /><img data-src=\"/x.png\" />";
        assert!(images(html).is_empty());
    }

    #[test]
    fn distinguishes_empty_from_missing_alt() {
        let images = images("<img src=\"/a.png\" alt=\"\" /><img src=\"/b.png\" alt=\" \" />");
        assert_eq!(images[0].alt.as_deref(), Some(""));
        assert_eq!(images[1].alt.as_deref(), Some(" "));
    }

    #[test]
    fn quote_aware_scanning_keeps_gt_inside_values() {
        let images = images("<img src=\"/a.png\" title=\"a > b\" alt=\"A\" />");
        assert_eq!(images[0].src, "/a.png");
        assert_eq!(images[0].alt.as_deref(), Some("A"));
    }
}
