//! Navigation/menu resolution: validated configuration plus a route.
//!
//! Menus are configuration-driven, unlike taxonomy or search data, so they
//! deliberately do not touch [`crate::SiteModel`]. The projection is a pure
//! function:
//!
//! ```text
//! validated configuration + current route → navigation render model
//! ```
//!
//! No filesystem, no generated HTML, no network. Configuration order is the
//! display order; active state is exact route equality only.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use crate::config::MenuConfig;
use crate::ids::Route;
use crate::meta::encode_url_path;

/// One resolved menu item: what templates actually receive.
///
/// `active` is true only for internal items whose normalized route equals
/// the normalized current route. External items are never active.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// Display label.
    pub label: String,
    /// Destination URL: a normalized internal route in URL-path-encoded
    /// form (slice 15E — the same boundary as generated route links, so a
    /// menu can address routes containing `%`), or an absolute URL
    /// verbatim. Active matching always compares the raw logical routes,
    /// never the encoded output.
    pub url: String,
    /// Whether this item corresponds to the current page.
    pub active: bool,
}

/// Menu configuration or resolution failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MenuError {
    /// An item has an empty label.
    #[error("menu item {index} has an empty label")]
    EmptyLabel {
        /// Zero-based item index.
        index: usize,
    },

    /// An item URL is empty, malformed, or neither internal nor absolute.
    #[error("menu item {label:?} has an invalid url {url:?}: {reason}")]
    InvalidUrl {
        /// Item label.
        label: String,
        /// Offending URL.
        url: String,
        /// Why it was rejected.
        reason: String,
    },

    /// An item URL uses a rejected scheme.
    #[error("menu item {label:?} uses rejected URL scheme {scheme:?} in {url:?}")]
    RejectedScheme {
        /// Item label.
        label: String,
        /// Offending URL.
        url: String,
        /// Lowercased scheme.
        scheme: String,
    },
}

/// Normalize an internal menu URL to canonical route form: leading slash,
/// exactly one trailing slash (`/` stays `/`).
fn normalize_internal(url: &str) -> Result<String, String> {
    if !url.starts_with('/') || url.starts_with("//") {
        return Err("internal URLs must start with a single `/`".to_string());
    }
    if url.contains(char::is_whitespace) || url.chars().any(|c| c.is_control()) {
        return Err("URL must not contain whitespace or control characters".to_string());
    }
    if url.contains('?') || url.contains('#') {
        return Err("URLs must not contain `?` or `#`".to_string());
    }
    if url.contains('?') || url.contains('#') {
        return Err("URLs must not contain `?` or `#`".to_string());
    }
    let trimmed = url.trim_matches('/');
    if trimmed.is_empty() {
        return Ok("/".to_string());
    }
    for segment in trimmed.split('/') {
        if segment == "." || segment == ".." {
            return Err("URLs must not contain `.` or `..` segments".to_string());
        }
    }
    Ok(format!("/{trimmed}/"))
}

/// Split `scheme:rest`, mirroring the strictness of image-URL validation:
/// a colon before any `/` introduces a scheme; otherwise the value is a path.
fn split_scheme(value: &str) -> Option<(&str, &str)> {
    let colon = value.find(':')?;
    if value[..colon].contains('/') {
        return None;
    }
    let scheme = &value[..colon];
    if scheme.is_empty()
        || !scheme.chars().enumerate().all(|(i, c)| {
            c.is_ascii_alphabetic()
                || (i > 0 && (c.is_ascii_digit() || c == '+' || c == '-' || c == '.'))
        })
    {
        return None;
    }
    Some((scheme, &value[colon + 1..]))
}

/// Validate one external URL: absolute `http(s)` with a non-empty host.
fn validate_external(label: &str, url: &str, scheme: &str) -> Result<String, MenuError> {
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(MenuError::RejectedScheme {
            label: label.to_string(),
            url: url.to_string(),
            scheme: scheme.to_ascii_lowercase(),
        });
    }
    if url.contains(char::is_whitespace) || url.chars().any(|c| c.is_control()) {
        return Err(MenuError::InvalidUrl {
            label: label.to_string(),
            url: url.to_string(),
            reason: "URL must not contain whitespace or control characters".to_string(),
        });
    }
    let after_scheme = &url[scheme.len() + 1..];
    let host = after_scheme
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("");
    if !after_scheme.starts_with("//") || host.is_empty() {
        return Err(MenuError::InvalidUrl {
            label: label.to_string(),
            url: url.to_string(),
            reason: "absolute URLs must look like `https://host/path`".to_string(),
        });
    }
    Ok(url.to_string())
}

/// Resolve the `main` menu for one page.
///
/// Returns `None` when no `main` menu is configured (or it has no items),
/// so templates can omit navigation entirely. Otherwise validates every
/// item in configuration order and marks the item whose normalized route
/// equals the normalized current route as active.
///
/// Items are shape-validated here (syntactically valid routes, normalized
/// and encoded identically everywhere); existence against generated routes
/// is validated downstream by `signal-cli::link_check`, which fails a build
/// or `signal check` on an internal menu target nothing generates.
pub fn resolve_main_menu(
    menus: &BTreeMap<String, MenuConfig>,
    current: &Route,
) -> Result<Option<Vec<MenuItem>>, MenuError> {
    let Some(config) = menus.get("main") else {
        return Ok(None);
    };
    if config.items.is_empty() {
        return Ok(None);
    }
    let current = normalize_internal(&current.0).unwrap_or_else(|_| current.0.clone());
    let mut items = Vec::with_capacity(config.items.len());
    for (index, item) in config.items.iter().enumerate() {
        let label = item.label.trim();
        if label.is_empty() {
            return Err(MenuError::EmptyLabel { index });
        }
        let url = item.url.trim();
        if url.is_empty() {
            return Err(MenuError::InvalidUrl {
                label: label.to_string(),
                url: item.url.clone(),
                reason: "URL must not be empty".to_string(),
            });
        }
        let invalid = |reason: &str| MenuError::InvalidUrl {
            label: label.to_string(),
            url: item.url.clone(),
            reason: reason.to_string(),
        };
        if let Some((scheme, _)) = split_scheme(url) {
            items.push(MenuItem {
                label: label.to_string(),
                url: validate_external(label, url, scheme)?,
                active: false,
            });
        } else if url.starts_with('/') {
            let route = normalize_internal(url).map_err(|reason| invalid(reason.as_str()))?;
            // Active matching compares raw logical routes; only the emitted
            // URL is encoded. Authored values are literal route data (like
            // `Route` itself), so an authored `%` encodes rather than acting
            // as an escape — and external URLs never reach this branch.
            items.push(MenuItem {
                active: route == current,
                label: label.to_string(),
                url: encode_url_path(&route),
            });
        } else {
            return Err(invalid(
                "URL must be an internal route starting with `/` or an absolute `http(s)` URL",
            ));
        }
    }
    Ok(Some(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menus(items: &[(&str, &str)]) -> BTreeMap<String, MenuConfig> {
        BTreeMap::from([(
            "main".to_string(),
            MenuConfig {
                items: items
                    .iter()
                    .map(|(label, url)| crate::config::MenuItemConfig {
                        label: label.to_string(),
                        url: url.to_string(),
                    })
                    .collect(),
            },
        )])
    }

    #[test]
    fn resolves_internal_and_external_in_configuration_order() {
        let items = resolve_main_menu(
            &menus(&[
                ("Home", "/"),
                ("Posts", "/posts/"),
                ("Elsewhere", "https://example.org/"),
            ]),
            &Route::new("/posts/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "Home");
        assert!(!items[0].active);
        assert_eq!(items[1].url, "/posts/");
        assert!(items[1].active);
        assert_eq!(items[2].url, "https://example.org/");
        assert!(!items[2].active);
    }

    #[test]
    fn missing_or_empty_menu_resolves_to_none() {
        assert_eq!(
            resolve_main_menu(&BTreeMap::new(), &Route::new("/")),
            Ok(None)
        );
        assert_eq!(
            resolve_main_menu(
                &BTreeMap::from([("other".to_string(), MenuConfig { items: vec![] })]),
                &Route::new("/")
            ),
            Ok(None)
        );
        assert_eq!(resolve_main_menu(&menus(&[]), &Route::new("/")), Ok(None));
    }

    #[test]
    fn trailing_slash_variants_match() {
        let items = resolve_main_menu(&menus(&[("Posts", "/posts")]), &Route::new("/posts/"))
            .expect("valid")
            .expect("present");
        assert_eq!(items[0].url, "/posts/");
        assert!(items[0].active);
    }

    #[test]
    fn menu_urls_reject_traversal_segments() {
        for bad in ["/posts/../x", "/./x", "/../"] {
            assert!(
                resolve_main_menu(&menus(&[("X", bad)]), &Route::new("/")).is_err(),
                "menu URL {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn home_route_matches_only_home() {
        let items = resolve_main_menu(&menus(&[("Home", "/")]), &Route::new("/"))
            .expect("valid")
            .expect("present");
        assert!(items[0].active);
        let items = resolve_main_menu(&menus(&[("Home", "/")]), &Route::new("/posts/"))
            .expect("valid")
            .expect("present");
        assert!(!items[0].active);
    }

    #[test]
    fn rejects_malformed_items() {
        assert!(matches!(
            resolve_main_menu(&menus(&[("", "/")]), &Route::new("/")),
            Err(MenuError::EmptyLabel { index: 0 })
        ));
        for bad in [
            "",
            "   ",
            "posts/",
            "posts",
            "//cdn.example/x",
            "/a b/",
            "/a?x=1",
            "/a#b",
        ] {
            let err = resolve_main_menu(&menus(&[("X", bad)]), &Route::new("/"))
                .expect_err(&format!("must reject {bad:?}"));
            assert!(
                matches!(err, MenuError::InvalidUrl { .. }),
                "wrong variant for {bad:?}: {err}"
            );
        }
    }

    #[test]
    fn rejects_dangerous_schemes() {
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:void(0)",
            "data:text/html,<p>x</p>",
            "vbscript:msgbox(1)",
            "ftp://example.org/x",
            "mailto:someone@example.org",
        ] {
            let err = resolve_main_menu(&menus(&[("X", bad)]), &Route::new("/"))
                .expect_err(&format!("must reject {bad:?}"));
            assert!(
                matches!(err, MenuError::RejectedScheme { .. }),
                "wrong variant for {bad:?}: {err}"
            );
        }
    }

    #[test]
    fn accepts_http_and_rejects_hostless_urls() {
        let items = resolve_main_menu(
            &menus(&[("A", "http://example.org/x"), ("B", "HTTPS://EXAMPLE.ORG/")]),
            &Route::new("/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items[0].url, "http://example.org/x");
        assert!(!items[0].active);
        assert!(items[1].url.starts_with("HTTPS://"));
        for bad in ["https://", "https:///", "http://"] {
            assert!(
                resolve_main_menu(&menus(&[("X", bad)]), &Route::new("/")).is_err(),
                "must reject {bad:?}"
            );
        }
    }

    #[test]
    fn resolution_is_deterministic() {
        let cfg = menus(&[("B", "/b/"), ("A", "/a/")]);
        let route = Route::new("/a/");
        assert_eq!(
            resolve_main_menu(&cfg, &route),
            resolve_main_menu(&cfg, &route)
        );
    }

    // --- Slice 15E: internal menu URLs use the route URL encoding (15D-3) ---

    #[test]
    fn internal_percent_routes_emit_encoded_urls_and_stay_active() {
        // The emitted href must address the route exactly like generated
        // route links (`/posts/100%-done/` → `/posts/100%25-done/`), while
        // active matching still compares raw logical routes.
        let items = resolve_main_menu(
            &menus(&[("Pct", "/posts/100%-done/")]),
            &Route::new("/posts/100%-done/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items[0].url, "/posts/100%25-done/");
        assert!(items[0].active);
        // Another page: correctly encoded href, not active.
        let items = resolve_main_menu(
            &menus(&[("Pct", "/posts/100%-done/")]),
            &Route::new("/posts/other/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items[0].url, "/posts/100%25-done/");
        assert!(!items[0].active);
    }

    #[test]
    fn authored_encoded_looking_values_are_literal_data() {
        // Authored menu values are literal route data, like `Route` itself:
        // an authored `%` encodes rather than acting as an escape, so there
        // is no double-encoding ambiguity — but such an item addresses a
        // route containing a literal `%25`, not the `%` route.
        let items = resolve_main_menu(
            &menus(&[("Pct", "/posts/100%25-done/")]),
            &Route::new("/posts/100%-done/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items[0].url, "/posts/100%2525-done/");
        assert!(!items[0].active);
    }

    #[test]
    fn ordinary_and_external_menu_urls_are_unchanged() {
        let items = resolve_main_menu(
            &menus(&[
                ("Posts", "/posts/"),
                ("Café", "/posts/café/"),
                ("Ext", "https://example.org/a?x=1#f"),
            ]),
            &Route::new("/posts/"),
        )
        .expect("valid")
        .expect("present");
        assert_eq!(items[0].url, "/posts/");
        assert!(items[0].active);
        // Non-ASCII encodes as UTF-8, exactly like generated route links.
        assert_eq!(items[1].url, "/posts/caf%C3%A9/");
        assert!(!items[1].active);
        // External URLs are authored URLs, never route-encoded.
        assert_eq!(items[2].url, "https://example.org/a?x=1#f");
        assert!(!items[2].active);
    }
}
