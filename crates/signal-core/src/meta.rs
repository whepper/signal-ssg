//! Presentation metadata helpers: dates, canonical URLs, image URLs.
//!
//! All helpers are pure, deterministic, and locale-free: no system timezone,
//! no machine locale, no I/O. They sit in `signal-core` because projections
//! in every crate need identical semantics.

use crate::ids::Route;

/// Default date format (strftime-style subset, see [`format_date`]).
/// Echoes the stored form.
pub const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// Parse a strict calendar date `YYYY-MM-DD`.
///
/// Returns `(year, month, day)` or `None` for wrong shapes, out-of-range
/// months, or impossible days (including February 29 on non-leap years).
/// Day-precision dates carry no timezone; none is assumed.
pub fn parse_ymd(value: &str) -> Option<(i32, u32, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = |lo: usize, hi: usize| -> Option<i32> { value.get(lo..hi)?.parse::<i32>().ok() };
    let year = digits(0, 4)?;
    let month = digits(5, 7)?;
    let day = digits(8, 10)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    // Reject non-digit smuggling: `parse` accepts `+`/whitespace.
    if !value
        .chars()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => unreachable!(),
    };
    if !(1..=max_day).contains(&day) {
        return None;
    }
    #[allow(clippy::cast_sign_loss)]
    let (month, day) = (month as u32, day as u32);
    Some((year, month, day))
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Format a stored `YYYY-MM-DD` date for presentation.
///
/// Supported verbs: `%Y` (2026), `%m` (09), `%d` (02), `%-d` (2),
/// `%B` (September), `%b` (Sep), `%%` (literal `%`). Anything else is
/// emitted literally (e.g. `%Q` stays `%Q`). Returns `None` for invalid
/// input dates. Month names are fixed English: formatting never depends on
/// machine locale.
pub fn format_date(ymd: &str, format: &str) -> Option<String> {
    let (year, month, day) = parse_ymd(ymd)?;
    let mut out = String::with_capacity(format.len() + 8);
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{year:04}")),
            Some('m') => out.push_str(&format!("{month:02}")),
            Some('d') => out.push_str(&format!("{day:02}")),
            Some('b') => out.push_str(&MONTHS[(month - 1) as usize][..3]),
            Some('B') => out.push_str(MONTHS[(month - 1) as usize]),
            Some('%') => out.push('%'),
            Some('-') => {
                if chars.next() == Some('d') {
                    out.push_str(&format!("{day}"));
                } else {
                    out.push_str("%-");
                }
            }
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    Some(out)
}

/// Format a stored `YYYY-MM-DD` date as an RFC 2822 timestamp for feeds
/// (`Thu, 03 Sep 2026 00:00:00 +0000`).
///
/// Day-precision dates have no time: midnight UTC is fixed, never the
/// machine timezone. Weekday names are computed calendrically (Zeller) in
/// fixed English — no locale. Returns `None` for invalid input dates.
pub fn rfc2822_date(ymd: &str) -> Option<String> {
    const WEEKDAYS: [&str; 7] = ["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"];
    const MONTHS_ABBR: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (mut year, mut month, day) = parse_ymd(ymd)?;
    if month <= 2 {
        month += 12;
        year -= 1;
    }
    let (k, j) = (year % 100, year.div_euclid(100));
    #[allow(clippy::cast_possible_wrap)]
    let m = month as i32;
    let h = (day as i32 + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    let (year, month, day) = parse_ymd(ymd)?;
    Some(format!(
        "{}, {:02} {} {:04} 00:00:00 +0000",
        WEEKDAYS[h as usize],
        day,
        MONTHS_ABBR[(month - 1) as usize],
        year
    ))
}
/// Join a site base URL and a route into a canonical URL.
///
/// `base_url` may carry a trailing slash or not; routes always start with
/// `/`. The route is serialized with [`encode_route_path`], so logical
/// routes containing URL-significant characters (`?`, `#`, `%`, …) still
/// produce syntactically correct URLs. Route and canonical URL stay distinct
/// concepts: the route addresses the page inside the site, the canonical
/// addresses it on the web.
pub fn canonical_url(base_url: &str, route: &Route) -> String {
    format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        encode_route_path(route)
    )
}

/// Serialize a logical route as a URL path (`/posts/a%3Fx%3D1/`).
///
/// This is the single route-to-URL boundary: every subsystem that turns a
/// [`Route`] into a URL (canonical URLs, feeds, sitemap locations, search
/// index paths, listing links) goes through here — directly or via
/// [`canonical_url`] — so one logical route always serializes to one URL
/// path. The [`Route`] itself is untouched: identity, validation, manifest
/// keys, and filesystem output paths all keep the raw logical value.
///
/// Policy (deliberate, tested): the route is split on `/` and each segment
/// is encoded independently, so separators stay structural and trailing
/// slashes are preserved. Within a segment, RFC 3986 unreserved bytes
/// (`A-Z a-z 0-9 - . _ ~`) stay literal; every other byte — including `%`,
/// `?`, `#`, `&`, `=`, `+`, `:`, `@`, and the UTF-8 bytes of non-ASCII
/// characters — is percent-encoded with uppercase hex. The set is
/// conservative on purpose: a route path is opaque site data, not a
/// hand-authored URI, so no delimiter semantics may leak (`;` parameters,
/// `&`/`=` query confusion, `+`-as-space misreading).
///
/// The route is logical data, never pre-encoded URL text: a literal `%` is
/// always encoded (`100%` → `100%25`, `100%25` → `100%2525`), so decoding
/// the output always recovers the route exactly and no `%xx`-looking input
/// is ever misread as an escape. `?` and `#` can therefore never introduce
/// a query string or fragment. Unicode is preserved semantically via UTF-8
/// percent-encoding (`café` → `caf%C3%A9`); no normalization is applied.
///
/// Deterministic and locale-free: pure byte operations, no environment data.
pub fn encode_route_path(route: &Route) -> String {
    encode_url_path(&route.0)
}

/// Percent-encode a `/`-separated path for URL use, preserving separators.
///
/// Same segment policy as [`encode_route_path`], for paths that are not
/// [`Route`] values (feed `self` links are artifact output paths, not
/// routes). Empty segments — leading, trailing, or doubled slashes — pass
/// through untouched: shape is preserved, only segment bytes are encoded.
pub fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.is_empty() {
                String::new()
            } else {
                encode_segment(segment)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encode one path segment: unreserved bytes stay literal, every
/// other byte becomes `%XX` (uppercase hex). Operates on UTF-8 bytes, so
/// non-ASCII characters encode as their UTF-8 sequence and malformed output
/// is impossible by construction.
fn encode_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(segment.len());
    for &byte in segment.as_bytes() {
        if matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
        ) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

/// Intended use of an author-controlled URL.
///
/// Signal applies one allow-list policy to author URLs so front-matter
/// images and Markdown link/image destinations cannot drift apart:
/// relative/root-relative/protocol-relative destinations, `http`, and
/// `https` are always allowed; `mailto:` is allowed for navigational links
/// but rejected as an image source (it is not a meaningful image).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlUse {
    /// A navigational destination rendered as an `href`.
    Link,
    /// An image source rendered as a `src`.
    Image,
}

/// Whether an author-controlled URL is safe to emit as an `href`/`src`.
///
/// The value is trimmed, then rejected when empty or when it contains
/// whitespace/control characters (which browsers strip or reinterpret).
/// An explicit scheme is accepted only from a small allow-list; every other
/// scheme — `javascript:`, `data:`, `vbscript:`, and anything unknown — is
/// rejected. Values with no scheme (relative paths, site-root-relative
/// `/…`, protocol-relative `//…`, fragments, queries) are accepted.
///
/// Scheme detection is case-insensitive and works on the parsed
/// `scheme:rest` shape ([`split_scheme`]). Markdown destinations reach this
/// already entity-decoded by Comrak, so `javascript&#58;`,
/// `&#106;avascript:`, and `data&colon;…` are seen as their literal schemes
/// and rejected. A percent-encoded sequence such as `%6aavascript:` is not a
/// valid scheme start (URL parsers require an ASCII letter first) and stays a
/// relative destination.
pub fn is_safe_author_url(value: &str, usage: UrlUse) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.contains(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return false;
    }
    match split_scheme(value) {
        None => true,
        Some((scheme, _)) => {
            let scheme = scheme.to_ascii_lowercase();
            match usage {
                UrlUse::Link => matches!(scheme.as_str(), "http" | "https" | "mailto"),
                UrlUse::Image => matches!(scheme.as_str(), "http" | "https"),
            }
        }
    }
}

/// Normalize a front-matter image reference to its site-root URL form.
///
/// Accepted: `https://…` / `http://…` absolute URLs (passed through),
/// protocol-relative `//…` (passed through), root-relative `/…` (as-is),
/// and bare relative paths, which are interpreted as site-root-relative
/// (`images/a.webp` → `/images/a.webp`). Rejected (`None`): other schemes
/// (`javascript:`, `data:`, …), whitespace/control characters, empty input.
/// Callers decide whether rejection is an error; ingestion fails the build.
///
/// Scheme/whitespace safety is delegated to [`is_safe_author_url`] with
/// [`UrlUse::Image`], so front-matter images and Markdown image sources
/// share exactly one definition of a safe URL.
pub fn resolve_image_url(value: &str) -> Option<String> {
    let value = value.trim();
    if !is_safe_author_url(value, UrlUse::Image) {
        return None;
    }
    if split_scheme(value).is_some() {
        // Only `http`/`https` can reach here (enforced by the policy above).
        return Some(value.to_string());
    }
    if value.starts_with("//") || value.starts_with('/') {
        return Some(value.to_string());
    }
    Some(format!("/{value}"))
}

fn split_scheme(value: &str) -> Option<(&str, &str)> {
    let colon = value.find(':')?;
    let scheme = &value[..colon];
    if scheme.is_empty()
        || !scheme.chars().enumerate().all(|(i, c)| {
            c.is_ascii_alphabetic()
                || (i > 0 && (c.is_ascii_digit() || c == '+' || c == '-' || c == '.'))
        })
    {
        return None;
    }
    // A colon before any `/` introduces a scheme; `/` first means a path
    // (e.g. relative `a:b`? no — `a:b` has colon first, so it IS scheme-like
    // and gets rejected unless http(s), which is the safe direction).
    Some((scheme, &value[colon + 1..]))
}

/// Upgrade a resolved image path to an absolute URL for metadata
/// (`og:image`, JSON-LD). Absolute and protocol-relative inputs pass
/// through; root-relative and bare paths join onto `base_url`.
pub fn absolute_url(base_url: &str, resolved: &str) -> String {
    if resolved.starts_with("http://")
        || resolved.starts_with("https://")
        || resolved.starts_with("//")
    {
        return resolved.to_string();
    }
    canonical_url(base_url, &Route::new(resolved.to_string()))
}

/// Serialize a normalized front-matter image reference for template `src`
/// attributes. A resolved site-root image path is a path, never an authored
/// URL, so `#`/`?` encode here exactly as they do in metadata (which flows
/// through [`absolute_url`] → [`canonical_url`]). Absolute
/// (`http(s)://…`) and protocol-relative (`//…`) values pass through
/// untouched — encoding them would destroy the scheme.
pub fn image_src_url(image: &str) -> String {
    if image.starts_with('/') && !image.starts_with("//") {
        encode_url_path(image)
    } else {
        image.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_calendar_dates() {
        assert_eq!(parse_ymd("2026-09-02"), Some((2026, 9, 2)));
        assert_eq!(parse_ymd("2024-02-29"), Some((2024, 2, 29)));
        assert_eq!(parse_ymd("2000-02-29"), Some((2000, 2, 29)));
    }

    #[test]
    fn rejects_impossible_dates() {
        for bad in [
            "",
            "2026-9-2",
            "2026/09/02",
            "not-a-date",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2023-02-29",
            "1900-02-29",
            "2026-09-32",
            " 2026-09-02",
            "2026-09-02 ",
            "+2026-09-02",
        ] {
            assert_eq!(parse_ymd(bad), None, "input {bad:?}");
        }
    }

    #[test]
    fn formats_deterministically() {
        assert_eq!(
            format_date("2026-09-02", "%-d %B %Y"),
            Some("2 September 2026".to_string())
        );
        assert_eq!(
            format_date("2026-01-05", "%Y-%m-%d"),
            Some("2026-01-05".to_string())
        );
        assert_eq!(
            format_date("2026-01-05", "%d %b %Y"),
            Some("05 Jan 2026".to_string())
        );
        assert_eq!(format_date("2026-01-05", "100%%"), Some("100%".to_string()));
        assert_eq!(format_date("2026-01-05", "%Q"), Some("%Q".to_string()));
        assert_eq!(format_date("nope", "%Y"), None);
        assert_eq!(format_date("2026-02-30", "%Y"), None);
    }

    #[test]
    fn rfc2822_uses_utc_midnight_and_computed_weekdays() {
        // Weekdays verified against the reference calendar: 2026-09-02 is a
        // Wednesday, 2026-09-03 a Thursday.
        assert_eq!(
            rfc2822_date("2026-09-02"),
            Some("Wed, 02 Sep 2026 00:00:00 +0000".to_string())
        );
        assert_eq!(
            rfc2822_date("2026-09-03"),
            Some("Thu, 03 Sep 2026 00:00:00 +0000".to_string())
        );
        assert_eq!(
            rfc2822_date("2024-02-29"),
            Some("Thu, 29 Feb 2024 00:00:00 +0000".to_string())
        );
        assert_eq!(rfc2822_date("2026-02-30"), None);
        assert_eq!(rfc2822_date("not-a-date"), None);
    }

    #[test]
    fn canonical_joins_base_and_route() {
        let route = Route::new("/posts/a/");
        assert_eq!(
            canonical_url("https://example.com", &route),
            "https://example.com/posts/a/"
        );
        assert_eq!(
            canonical_url("https://example.com/", &route),
            "https://example.com/posts/a/"
        );
        assert_eq!(
            canonical_url("https://example.com/", &Route::new("/")),
            "https://example.com/"
        );
    }

    // --- Slice 15C: route-to-URL path encoding (R14-3) ---

    #[test]
    fn route_paths_encode_delimiters_and_percent() {
        // `?` and `#` must never become a query string or fragment; a
        // literal `%` is always encoded, never treated as a pre-existing
        // escape (so `100%25` encodes its `%` too: decode inverts exactly).
        for (route, expected) in [
            ("/posts/issue#12/", "/posts/issue%2312/"),
            ("/posts/100%-done/", "/posts/100%25-done/"),
            ("/posts/a?x=1/", "/posts/a%3Fx%3D1/"),
            ("/posts/100%25/", "/posts/100%2525/"),
            ("/posts/100%done/", "/posts/100%25done/"),
            ("/posts/a&b/", "/posts/a%26b/"),
            ("/posts/a:b/", "/posts/a%3Ab/"),
            ("/posts/a=b+c/", "/posts/a%3Db%2Bc/"),
            ("/posts/a@b,c;d/", "/posts/a%40b%2Cc%3Bd/"),
            ("/posts/a'b/", "/posts/a%27b/"),
            ("/posts/a b/", "/posts/a%20b/"),
            ("/posts/a$b/", "/posts/a%24b/"),
            ("/posts/a!b/", "/posts/a%21b/"),
            ("/posts/a(b)/", "/posts/a%28b%29/"),
            ("/posts/a*b/", "/posts/a%2Ab/"),
        ] {
            assert_eq!(
                encode_route_path(&Route::new(route)),
                expected,
                "route {route:?}"
            );
        }
    }

    #[test]
    fn route_paths_preserve_structure_and_safe_characters() {
        // Separators stay structural, trailing slashes survive, and ordinary
        // slugs (plus RFC 3986 unreserved marks) pass through byte-identical.
        for (route, expected) in [
            ("/", "/"),
            ("/posts/", "/posts/"),
            ("/posts/my-project/", "/posts/my-project/"),
            ("/a/b/c/", "/a/b/c/"),
            ("/posts/hello_world~v2/", "/posts/hello_world~v2/"),
            ("/posts/v1.2/", "/posts/v1.2/"),
            ("/posts/.../", "/posts/.../"),
        ] {
            assert_eq!(
                encode_route_path(&Route::new(route)),
                expected,
                "route {route:?}"
            );
        }
    }

    #[test]
    fn route_paths_encode_unicode_as_utf8() {
        // Non-ASCII routes stay addressable via UTF-8 percent-encoding; no
        // normalization is applied (NFC and NFD remain distinct inputs).
        assert_eq!(
            encode_route_path(&Route::new("/posts/caf\u{e9}/")),
            "/posts/caf%C3%A9/"
        );
        assert_eq!(
            encode_route_path(&Route::new("/posts/cafe\u{301}/")),
            "/posts/cafe%CC%81/"
        );
        assert_eq!(
            encode_route_path(&Route::new("/posts/日本語/")),
            "/posts/%E6%97%A5%E6%9C%AC%E8%AA%9E/"
        );
        // Mixed: delimiters and Unicode in one segment.
        assert_eq!(
            encode_route_path(&Route::new("/posts/a#b\u{e9}?/")),
            "/posts/a%23b%C3%A9%3F/"
        );
    }

    #[test]
    fn encoded_routes_carry_no_query_or_fragment() {
        // Structural proof for the browser-semantics invariant: everything
        // stays one path — no literal `?` query introducer and no literal
        // `#` fragment introducer. (`%` appears only inside well-formed
        // `%XX` escapes, verified byte-exact by the tables above.)
        for route in [
            "/posts/issue#12/",
            "/posts/a?x=1/",
            "/posts/100%-done/",
            "/posts/caf\u{e9}/",
            "/posts/a&b=c+d/",
        ] {
            let encoded = encode_route_path(&Route::new(route));
            assert!(
                !encoded.contains('?'),
                "route {route:?} encoded to {encoded:?}"
            );
            assert!(
                !encoded.contains('#'),
                "route {route:?} encoded to {encoded:?}"
            );
        }
    }

    #[test]
    fn canonical_url_encodes_routes_but_not_the_base() {
        assert_eq!(
            canonical_url("https://example.com", &Route::new("/posts/issue#12/")),
            "https://example.com/posts/issue%2312/"
        );
        assert_eq!(
            canonical_url("https://example.com/", &Route::new("/posts/a?x=1/")),
            "https://example.com/posts/a%3Fx%3D1/"
        );
        // The base URL is concatenated, never encoded.
        assert_eq!(
            canonical_url("https://example.com/sub/dir/", &Route::new("/posts/a/")),
            "https://example.com/sub/dir/posts/a/"
        );
    }

    #[test]
    fn url_path_helper_encodes_segments_and_preserves_shape() {
        assert_eq!(
            encode_url_path("topics/rust/index.xml"),
            "topics/rust/index.xml"
        );
        assert_eq!(encode_url_path("index.xml"), "index.xml");
        assert_eq!(encode_url_path("t#/rust/index.xml"), "t%23/rust/index.xml");
        assert_eq!(encode_url_path("/posts/a/"), "/posts/a/");
    }

    #[test]
    fn image_urls_resolve_and_reject_safely() {
        assert_eq!(
            resolve_image_url("/images/a.webp"),
            Some("/images/a.webp".to_string())
        );
        assert_eq!(
            resolve_image_url("images/a.webp"),
            Some("/images/a.webp".to_string())
        );
        assert_eq!(
            resolve_image_url("https://cdn.example/a.png"),
            Some("https://cdn.example/a.png".to_string())
        );
        assert_eq!(
            resolve_image_url("//cdn.example/a.png"),
            Some("//cdn.example/a.png".to_string())
        );
        for bad in [
            "",
            "   ",
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:image/png;base64,AAA",
            "vbscript:x",
            "ima ges/a.png",
            "a\nb",
        ] {
            assert_eq!(resolve_image_url(bad), None, "input {bad:?}");
        }
    }

    #[test]
    fn absolute_url_upgrades_paths() {
        assert_eq!(
            absolute_url("https://example.com", "/images/a.webp"),
            "https://example.com/images/a.webp"
        );
        assert_eq!(
            absolute_url("https://example.com/", "https://cdn.example/a.png"),
            "https://cdn.example/a.png"
        );
        assert_eq!(
            absolute_url("https://example.com", "//cdn.example/a.png"),
            "//cdn.example/a.png"
        );
    }

    // --- Slice 14A: shared author-URL safety policy ---

    #[test]
    fn url_safety_allows_relative_and_http_family() {
        for ok in [
            "relative/path",
            "/root/path",
            "../up",
            "//cdn.example/x",
            "#frag",
            "?q=1",
            "/page?x=1#s",
            "http://example.com/a",
            "https://example.com/a?b=1#c",
        ] {
            assert!(is_safe_author_url(ok, UrlUse::Link), "link {ok:?}");
            assert!(is_safe_author_url(ok, UrlUse::Image), "image {ok:?}");
        }
    }

    #[test]
    fn url_safety_allows_mailto_for_links_only() {
        assert!(is_safe_author_url("mailto:a@example.com", UrlUse::Link));
        assert!(!is_safe_author_url("mailto:a@example.com", UrlUse::Image));
    }

    #[test]
    fn url_safety_rejects_executable_and_unknown_schemes() {
        for bad in [
            "javascript:alert(1)",
            "JAVASCRIPT:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,x",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
            "ftp://example.com/x",
        ] {
            assert!(!is_safe_author_url(bad, UrlUse::Link), "link {bad:?}");
            assert!(!is_safe_author_url(bad, UrlUse::Image), "image {bad:?}");
        }
    }

    #[test]
    fn url_safety_rejects_empty_and_control_characters() {
        for bad in [
            "",
            "   ",
            "java script:x",
            "java\tscript:x",
            "\u{0}javascript:x",
        ] {
            assert!(!is_safe_author_url(bad, UrlUse::Link), "link {bad:?}");
            assert!(!is_safe_author_url(bad, UrlUse::Image), "image {bad:?}");
        }
    }

    #[test]
    fn image_policy_is_shared_with_url_safety() {
        // Every accepted front-matter image is accepted by the shared
        // policy, and every rejected form stays rejected: one definition,
        // two callers (front-matter images and Markdown image sources).
        for ok in [
            "images/a.webp",
            "/images/a.webp",
            "https://cdn.example/a.png",
            "//cdn.example/a.png",
        ] {
            assert!(resolve_image_url(ok).is_some(), "{ok:?}");
            assert!(is_safe_author_url(ok, UrlUse::Image), "{ok:?}");
        }
        for bad in ["javascript:x", "data:x", "mailto:a@b.c", "a b"] {
            assert!(resolve_image_url(bad).is_none(), "{bad:?}");
            assert!(!is_safe_author_url(bad, UrlUse::Image), "{bad:?}");
        }
    }

    #[test]
    fn image_src_url_encodes_paths_but_preserves_urls() {
        // Site-root paths encode URL-significant bytes per segment.
        assert_eq!(image_src_url("/images/a b.png"), "/images/a%20b.png");
        assert_eq!(image_src_url("/img/a.png"), "/img/a.png");
        // Absolute and protocol-relative values pass through untouched.
        for url in [
            "https://cdn.example/a b.png",
            "http://example.com/a.png",
            "//cdn.example/a.png",
            "images/relative.png",
        ] {
            assert_eq!(image_src_url(url), url, "input {url:?}");
        }
    }
}
