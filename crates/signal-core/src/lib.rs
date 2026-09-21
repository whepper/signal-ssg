//! `signal-core`: canonical immutable site model for Signal SSG.
//!
//! This crate owns the normalized, owned representation of site content.
//! It performs no filesystem I/O, no template rendering, and no CLI work.
//!
#![forbid(unsafe_code)]
//! Design notes:
//! - Content identity is `(collection, source-relative-path)` by default.
//! - [`ContentId`], [`Slug`], and [`Route`] are distinct types on purpose.
//! - [`SiteModel`] is immutable after construction; build it via
//!   [`SiteModelBuilder`] and query it through `&SiteModel`.
//! - Semantic relationships (references, translations, tags) are indexes,
//!   not a generic graph engine and not build dependencies.

pub mod artifact;
pub mod asset;
pub mod body;
pub mod config;
pub mod error;
pub mod ids;
pub mod menu;
pub mod meta;
pub mod model;
pub mod social;

pub use artifact::{ArtifactKind, ArtifactSpec};
pub use asset::{
    asset_referrers, derivative_output_path, entry_asset_paths, is_derivable_source,
    is_external_reference, mime_for_path, output_path_for_source, percent_decode,
    resolve_body_image, resolve_front_matter_image, responsive_image, DerivativeFormat,
    DerivativeSpec, DerivativeView, ResponsiveCandidate, ResponsiveImage, ResponsiveSource,
    DEFAULT_SIZES,
};
pub use body::{CodeBlock, Heading, RenderedBody, Toc, TocItem};
pub use config::{
    CollectionConfig, FeedConfig, GitConfig, ImagesConfig, MenuConfig, MenuItemConfig,
    RelatedConfig, RobotsConfig, SignalConfig, SiteConfig, SocialConfig, TaxonomyConfig,
    DEFAULT_FEED_LIMIT,
};
pub use error::CoreError;
pub use ids::{
    slugify, validate_route, validate_route_segment, CollectionId, ContentId, Route, Slug,
    SourceRef,
};
pub use menu::{resolve_main_menu, MenuError, MenuItem};
pub use meta::{
    absolute_url, canonical_url, encode_route_path, encode_url_path, format_date, image_src_url,
    is_safe_author_url, parse_ymd, resolve_image_url, rfc2822_date, UrlUse, DEFAULT_DATE_FORMAT,
};
pub use model::{compare_by_date_desc, ContentEntry, SiteModel, SiteModelBuilder};
pub use social::{
    social_image_absolute_url, social_image_eligible, social_image_for_output, social_image_hero,
    social_image_override, social_image_path, social_image_route, social_image_url, PlannedSocial,
    DEFAULT_SOCIAL_HEIGHT, DEFAULT_SOCIAL_WIDTH, MAX_SOCIAL_DIMENSION, SOCIAL_OUTPUT_DIR,
};
