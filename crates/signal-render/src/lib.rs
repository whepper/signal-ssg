//! `signal-render`: renderer abstraction and MiniJinja boundary.
//!
//! - [`Renderer`] is the only rendering surface used by generators/CLI.
//! - [`RenderContext`] is explicit: a named value bag, no ambient state.
//! - [`MiniJinjaRenderer`] loads templates from in-memory strings only:
//!   no filesystem access, no network access.
//! - HTML auto-escaping is enabled for `.html` templates.
//! - Template dependency tracking is deliberately coarse: the rendered
//!   template set is exposed so template-rendered artifacts can depend on it
//!   as a whole (`InputRef::TemplateSet`, ADR 0020). There is no per-render or
//!   field-level access tracking — MiniJinja 2.x exposes no per-render hook.
//!
//! MiniJinja types never leak through this API; failures surface as
//! [`RenderError`] (owned strings).
//!
//! Post-render HTML minification ([`minify_html`]) lives here too: it runs on
//! the finished rendered string — after contexts, escaping, and serialization
//! — so template concerns never leak into output optimization and the
//! third-party minifier stays behind this crate boundary like MiniJinja.

#![forbid(unsafe_code)]

use minijinja::{AutoEscape, Environment, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Rendering failure (owns its message; no engine types leak).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    /// Requested template is not loaded.
    #[error("unknown template: {0}")]
    UnknownTemplate(String),
    /// Template is loaded but invalid.
    #[error("invalid template {name}: {message}")]
    InvalidTemplate {
        /// Template name.
        name: String,
        /// Engine message (owned).
        message: String,
    },
    /// Rendering a valid template failed.
    #[error("render failed for template {name}: {message}")]
    RenderFailed {
        /// Template name.
        name: String,
        /// Engine message (owned).
        message: String,
    },
}

/// Explicit rendering context: named values for one render call.
///
/// Deterministic (`BTreeMap`) so output ordering is stable.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderContext {
    values: BTreeMap<String, serde_json::Value>,
}

impl RenderContext {
    /// Create an empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a serializable value under `key`, replacing any previous value.
    pub fn insert<T: Serialize>(&mut self, key: impl Into<String>, value: T) -> &mut Self {
        let v = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
        self.values.insert(key.into(), v);
        self
    }

    /// Number of bound values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether no values are bound.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn as_map(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.values
    }
}

/// Renderer abstraction. Synchronous by design; no async, no filesystem.
pub trait Renderer {
    /// Render a loaded template with an explicit context.
    fn render(&self, template_name: &str, ctx: &RenderContext) -> Result<String, RenderError>;
    /// Names of loaded templates (the template dependency closure seed).
    fn template_names(&self) -> Vec<String>;
}

/// MiniJinja implementation of [`Renderer`].
///
/// Templates are added from strings via [`Self::add_template`]. There is
/// deliberately no `from_directory` / filesystem loader and no network
/// loading.
pub struct MiniJinjaRenderer {
    env: Environment<'static>,
    sources: BTreeMap<String, String>,
}

impl MiniJinjaRenderer {
    /// Create an empty renderer with HTML auto-escaping for `.html` templates.
    ///
    /// Registers the `date_format` filter: `{{ date | date_format("%-d %B %Y") }}`
    /// formats a stored `YYYY-MM-DD` value with the site's strftime-style
    /// subset (see `signal_core::format_date`). Missing, non-string, and
    /// invalid dates render as the empty string — never an error — so
    /// templates can apply it unconditionally to optional dates.
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|name| {
            if name.ends_with(".html") {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        env.add_filter("date_format", |value: Value, format: String| -> String {
            match value.as_str() {
                Some(ymd) => signal_core::format_date(ymd, &format).unwrap_or_default(),
                None => String::new(),
            }
        });
        Self {
            env,
            sources: BTreeMap::new(),
        }
    }

    /// Add (or replace) a template from an in-memory string.
    pub fn add_template(
        &mut self,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<(), RenderError> {
        let name = name.into();
        let source = source.into();
        self.env
            .add_template_owned(name.clone(), source.clone())
            .map_err(|e| RenderError::InvalidTemplate {
                name: name.clone(),
                message: e.to_string(),
            })?;
        self.sources.insert(name, source);
        Ok(())
    }

    /// Retrieve the recorded source for dependency discovery.
    pub fn template_source(&self, name: &str) -> Option<&str> {
        self.sources.get(name).map(String::as_str)
    }
}

impl Default for MiniJinjaRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for MiniJinjaRenderer {
    fn render(&self, template_name: &str, ctx: &RenderContext) -> Result<String, RenderError> {
        let template = self
            .env
            .get_template(template_name)
            .map_err(|_| RenderError::UnknownTemplate(template_name.to_string()))?;
        template
            .render(ctx.as_map())
            .map_err(|e| RenderError::RenderFailed {
                name: template_name.to_string(),
                message: e.to_string(),
            })
    }

    fn template_names(&self) -> Vec<String> {
        self.sources.keys().cloned().collect()
    }
}

/// Minify finished rendered HTML, deterministically.
///
/// This is the opt-in `[output] minify_html` step. It runs on the complete
/// rendered page string — never on template sources, contexts, or Markdown —
/// following the `template rendering → rendered HTML → HTML minification →
/// artifact/output` pipeline order. Input is `&str`, output is `String`;
/// no filesystem or network access is involved.
///
/// Safety contract (pinned by the tests below):
///
/// - HTML-aware parsing, never regexes: element structure, attributes
///   (quoted, safely-unquoted, or boolean), and entity decoding are handled
///   by the parser.
/// - Whitespace collapses only where HTML rendering already collapses it.
///   `<pre>` subtrees (which is where Signal's highlighted code blocks and
///   Mermaid sources live), `<textarea>`, `<script>`, and `<style>` contents
///   pass through byte-identical apart from leading/trailing trimming of the
///   script/style element bodies.
/// - Inline scripts and styles are trimmed, never reinterpreted: the
///   JavaScript/CSS minification features of the underlying library stay
///   disabled, so JSON-LD, Mermaid sources, KaTeX/MathML markup, and client
///   scripts keep their exact semantics.
/// - Comments are kept verbatim. Conditional, SSI, license, and template
///   debug comments are not provably safe to remove, and they cost little.
/// - Entity re-encoding is decoding-identical: `&quot;` may serialize as a
///   literal `"` and `&#x2f;` as `/`, but a browser decodes both forms to
///   the same text (what Mermaid renderers, copy buttons, and search
///   indexing read).
/// - Deterministic: identical input bytes always produce identical output
///   bytes (no maps with unspecified order, no timestamps, no randomness).
pub fn minify_html(html: &str) -> String {
    let mut cfg = simple_minify_html::Cfg::new();
    // See the safety contract above: comments stay.
    cfg.keep_comments = true;
    let bytes = simple_minify_html::minify(html.as_bytes(), Some(cfg));
    // The minifier copies input byte ranges and emits ASCII markup around
    // them; valid UTF-8 in means valid UTF-8 out.
    String::from_utf8(bytes).expect("HTML minifier preserves UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_explicit_context() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template("post.html", "<h1>{{ title }}</h1>").unwrap();
        let mut ctx = RenderContext::new();
        ctx.insert("title", "Hello");
        assert_eq!(r.render("post.html", &ctx).unwrap(), "<h1>Hello</h1>");
    }

    #[test]
    fn html_templates_autoescape() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template("post.html", "{{ user }}").unwrap();
        let mut ctx = RenderContext::new();
        ctx.insert("user", "<b>hi</b>");
        let out = r.render("post.html", &ctx).unwrap();
        assert!(
            out.contains("&lt;b&gt;"),
            "expected HTML escaping, got: {out}"
        );
        assert!(!out.contains("<b>hi</b>"));
    }

    #[test]
    fn unknown_template_is_an_error() {
        let r = MiniJinjaRenderer::new();
        let ctx = RenderContext::new();
        assert!(matches!(
            r.render("missing.html", &ctx),
            Err(RenderError::UnknownTemplate(_))
        ));
    }

    #[test]
    fn template_set_is_recorded_for_dependency_closure() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template("a.html", "a").unwrap();
        r.add_template("b.html", "b").unwrap();
        assert_eq!(r.template_names(), vec!["a.html", "b.html"]);
    }

    #[test]
    fn date_format_filter_formats_stored_dates() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template(
            "post.html",
            "{{ date | date_format(\"%-d %B %Y\") }}|{{ date | date_format(\"%d %b %Y\") }}|{{ date | date_format(\"%Y-%m-%d\") }}",
        )
        .unwrap();
        let mut ctx = RenderContext::new();
        ctx.insert("date", "2026-09-02");
        assert_eq!(
            r.render("post.html", &ctx).unwrap(),
            "2 September 2026|02 Sep 2026|2026-09-02"
        );
    }

    #[test]
    fn date_format_filter_handles_absent_and_invalid_dates() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template(
            "post.html",
            "[{{ missing | date_format(\"%-d %B %Y\") }}][{{ date | date_format(\"%-d %B %Y\") }}]",
        )
        .unwrap();
        // Missing key: the filter sees no string and renders nothing.
        let ctx = RenderContext::new();
        assert_eq!(r.render("post.html", &ctx).unwrap(), "[][]");
        // Invalid date: likewise empty, never a render error.
        let mut ctx = RenderContext::new();
        ctx.insert("date", "not-a-date");
        assert_eq!(r.render("post.html", &ctx).unwrap(), "[][]");
        // Non-string values (numbers, booleans) are also empty, not errors.
        let mut ctx = RenderContext::new();
        ctx.insert("date", 2026);
        assert_eq!(r.render("post.html", &ctx).unwrap(), "[][]");
    }

    #[test]
    fn date_format_filter_is_deterministic_and_literal_safe() {
        let mut r = MiniJinjaRenderer::new();
        r.add_template("post.html", "{{ date | date_format(\"%Q %%-d\") }}")
            .unwrap();
        let mut ctx = RenderContext::new();
        ctx.insert("date", "2026-01-05");
        // Unknown verbs pass through literally (the `format_date`
        // contract); repeated renders agree byte-for-byte.
        let first = r.render("post.html", &ctx).unwrap();
        assert_eq!(first, "%Q %-d");
        assert_eq!(r.render("post.html", &ctx).unwrap(), first);
    }

    /// A realistic rendered page exercising the constructs the minifier
    /// must preserve: navigation, alerts, highlighted code, Mermaid with
    /// its no-JS fallback, JSON-LD, entities, and boolean attributes.
    fn realistic_page() -> String {
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="description" content="Fish &amp; ships &#x2f; notes">
    <script type="application/ld+json">{"@context":"https://schema.org","@type":"Article","headline":"Fish & ships","datePublished":"2026-09-02"}</script>
  </head>
  <body>
    <nav aria-label="Main">
      <a href="/articles/" class="active">Articles</a>
    </nav>
    <aside class="alert alert-warning" role="alert">
      <p>Watch <strong>out</strong> &amp; stay sharp.</p>
    </aside>
    <div class="code-block"><pre><code class="language-sh"><span class="variable function shell">sync</span><span class="meta function-call arguments shell"> notes.txt  backup</span>
</code></pre><button data-code-copy type="button" disabled>Copy</button></div>
    <figure class="mermaid-container">
      <pre class="mermaid">flowchart TD
    A[&quot;source&quot;] --&gt;|prompt| B[&quot;sink&quot;]</pre>
      <details class="mermaid-source"><summary>View diagram source</summary><pre>flowchart TD
    A[&quot;source&quot;] --&gt;|prompt| B[&quot;sink&quot;]</pre></details>
    </figure>
    <form><textarea name="notes">  keep
  these  spaces  </textarea></form>
    <script>if (a < b && c > d) { console.log("stay"); }</script>
    <style>
      p { color: red; }
    </style>
    <!-- a template comment worth keeping -->
    <p>Fish &amp; ships &#x2f; notes</p>
  </body>
</html>
"#
        .to_string()
    }

    /// Minimal browser-like entity decoder, so tests assert decoded
    /// semantics rather than one serializer's quoting choices.
    fn decode_entities(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('&') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let Some(semi) = tail.find(';').filter(|j| *j < 12) else {
                out.push('&');
                rest = &tail[1..];
                continue;
            };
            let entity = &tail[1..semi];
            let decoded = if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = entity.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                match entity {
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "amp" => Some('&'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => None,
                }
            };
            match decoded {
                Some(ch) => {
                    out.push(ch);
                    rest = &tail[semi + 1..];
                }
                None => {
                    out.push('&');
                    rest = &tail[1..];
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Visible text: tags stripped, entities decoded, whitespace runs
    /// collapsed — what a reader (or indexer) observes outside `pre`.
    /// Tag boundaries count as word separators on both sides, because
    /// formatting-only whitespace between block elements is insignificant
    /// and any consistent normalization must agree.
    fn visible_text(html: &str) -> String {
        let spaced = html.replace("><", "> <");
        decode_entities(&strip_tags(&spaced))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Tag-stripped text (entities left encoded; pair with
    /// [`decode_entities`] when comparing across serializers).
    fn strip_tags(s: &str) -> String {
        let mut text = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('<') {
            text.push_str(&rest[..i]);
            match rest[i..].find('>') {
                Some(j) => rest = &rest[i + j + 1..],
                None => break,
            }
        }
        text.push_str(rest);
        text
    }

    fn tag_contents(html: &str, tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = html;
        let close = format!("</{tag}>");
        while let Some(i) = rest.find(&format!("<{tag}")) {
            let after = &rest[i..];
            let Some(gt) = after.find('>') else { break };
            let body = &after[gt + 1..];
            let Some(j) = body.find(&close) else { break };
            out.push(body[..j].to_string());
            rest = &body[j + close.len()..];
        }
        out
    }

    #[test]
    fn minify_collapses_whitespace_but_keeps_text() {
        let page = realistic_page();
        let min = minify_html(&page);
        assert!(
            min.len() < page.len(),
            "expected shrinkage, got {} -> {}",
            page.len(),
            min.len()
        );
        assert_eq!(visible_text(&min), visible_text(&page));
    }

    #[test]
    fn minify_preserves_code_and_mermaid_sources() {
        let page = realistic_page();
        let min = minify_html(&page);
        let pres = tag_contents(&page, "pre");
        let min_pres = tag_contents(&min, "pre");
        assert_eq!(pres.len(), 3, "code + mermaid + fallback pres");
        assert_eq!(min_pres.len(), 3);
        for (a, b) in pres.iter().zip(min_pres.iter()) {
            // Tag-stripped, entity-decoded: identical diagram sources and
            // code text, whatever the serializer's quoting choices.
            assert_eq!(
                decode_entities(&strip_tags(a)),
                decode_entities(&strip_tags(b))
            );
        }
        // The double space inside the highlighted code survives verbatim.
        assert!(min.contains("notes.txt  backup"), "got: {min}");
    }

    #[test]
    fn minify_preserves_textarea_script_style_and_json_ld() {
        let page = realistic_page();
        let min = minify_html(&page);
        // Textarea content is byte-identical, newlines and runs included.
        assert!(
            min.contains(">  keep\n  these  spaces  </textarea>"),
            "got: {min}"
        );
        // Inline script survives byte-identical (only trimmable edges move).
        assert!(
            min.contains(r#"if (a < b && c > d) { console.log("stay"); }"#),
            "got: {min}"
        );
        // Inline style keeps its rules; only surrounding whitespace goes.
        assert!(min.contains("p { color: red; }"), "got: {min}");
        // JSON-LD still parses to the same value.
        let script_bodies = tag_contents(&min, "script");
        let payload: serde_json::Value = script_bodies
            .iter()
            .filter_map(|b| serde_json::from_str(b.trim()).ok())
            .next()
            .expect("one JSON-LD block survives");
        assert_eq!(payload["@type"], "Article");
        assert_eq!(payload["headline"], "Fish & ships");
        assert_eq!(payload["datePublished"], "2026-09-02");
    }

    #[test]
    fn minify_keeps_comments_entities_and_attributes() {
        let page = realistic_page();
        let min = minify_html(&page);
        // Comments are kept verbatim by policy.
        assert!(
            min.contains("<!-- a template comment worth keeping -->"),
            "got: {min}"
        );
        // Decoded text matches: entity re-encoding never changes meaning.
        assert!(visible_text(&min).contains("Fish & ships / notes"));
        // Boolean attributes survive; values needing quotes keep them.
        assert!(min.contains("disabled"), "got: {min}");
        assert!(
            min.contains("role=alert") || min.contains("role=\"alert\""),
            "got: {min}"
        );
        assert!(
            min.contains("class=\"alert alert-warning\""),
            "spaced values stay quoted, got: {min}"
        );
        // Navigation hrefs and classes survive.
        assert!(min.contains("/articles/"), "got: {min}");
    }

    #[test]
    fn minify_is_deterministic() {
        let page = realistic_page();
        let first = minify_html(&page);
        for _ in 0..3 {
            assert_eq!(minify_html(&page), first);
        }
        // Extra blank lines in, identical bytes out.
        let padded = page.replace("<nav", "\n\n\n<nav");
        assert_eq!(minify_html(&padded), first);
    }
}
