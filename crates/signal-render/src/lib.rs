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

use minijinja::{AutoEscape, Environment};
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
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|name| {
            if name.ends_with(".html") {
                AutoEscape::Html
            } else {
                AutoEscape::None
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
}
