//! `signal-core`: canonical immutable site model for Signal SSG.
//!
//! This crate owns the normalized, owned representation of site content.
//! It performs no filesystem I/O, no template rendering, and no CLI work.
//!
//! Design notes:
//! - Content identity is `(collection, source-relative-path)` by default.
//! - [`ContentId`], [`Slug`], and [`Route`] are distinct types on purpose.
//! - [`SiteModel`] is immutable after construction; build it via
//!   [`SiteModelBuilder`] and query it through `&SiteModel`.
//! - Semantic relationships (references, translations, tags) are indexes,
//!   not a generic graph engine and not build dependencies.

pub mod artifact;
pub mod body;
pub mod config;
pub mod error;
pub mod ids;
pub mod menu;
pub mod meta;
pub mod model;

pub use artifact::{ArtifactKind, ArtifactSpec};
pub use body::{CodeBlock, Heading, RenderedBody, Toc, TocItem};
pub use config::{
    CollectionConfig, FeedConfig, MenuConfig, MenuItemConfig, SignalConfig, SiteConfig,
    TaxonomyConfig, DEFAULT_FEED_LIMIT,
};
pub use error::CoreError;
pub use ids::{
    slugify, validate_route, validate_route_segment, CollectionId, ContentId, Route, Slug,
    SourceRef,
};
pub use menu::{resolve_main_menu, MenuError, MenuItem};
pub use meta::{
    absolute_url, canonical_url, encode_route_path, encode_url_path, format_date,
    is_safe_author_url, parse_ymd, resolve_image_url, rfc2822_date, UrlUse, DEFAULT_DATE_FORMAT,
};
pub use model::{ContentEntry, SiteModel, SiteModelBuilder};
