//! Read-only, page-scoped inspection for agent/tool consumers.
//!
//! This is a public projection over the already-validated [`signal_core::SiteModel`].
//! It does not parse content independently, scan arbitrary files, render HTML,
//! perform network I/O, or create a second relationship/diagnostic model.

use std::path::Path;

use serde::Serialize;
use signal_core::{
    canonical_url, encode_route_path, image_src_url, ContentEntry, Route, SiteModel,
};
use signal_generators::{related_entries, EntrySummary};

use crate::errors::BuildError;

/// Stable schema identifier for the page inspection JSON contract.
pub const INSPECTION_SCHEMA: &str = "signal.inspect/v1";

/// Maximum number of entries returned in each bounded collection.
pub const MAX_INSPECTION_ITEMS: usize = 100;

/// A versioned, page-scoped view of Signal's resolved understanding.
#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    /// Stable schema identifier.
    pub schema: &'static str,
    /// The selected published page.
    pub page: InspectedPage,
}

/// A deterministic bounded collection with an explicit truncation contract.
#[derive(Clone, Debug, Serialize)]
pub struct Bounded<T> {
    /// Returned items, in canonical order.
    pub items: Vec<T>,
    /// Number of items before bounding.
    pub total: usize,
    /// Whether `items` was truncated at [`MAX_INSPECTION_ITEMS`].
    pub truncated: bool,
}

impl<T> Bounded<T> {
    fn from_items(items: Vec<T>) -> Self {
        let total = items.len();
        let mut items = items;
        items.truncate(MAX_INSPECTION_ITEMS);
        Self {
            truncated: total > items.len(),
            items,
            total,
        }
    }
}

/// Stable `(collection, path)` source reference.
#[derive(Clone, Debug, Serialize)]
pub struct SourceReference {
    /// Owning collection.
    pub collection: String,
    /// Path relative to that collection's configured source directory.
    pub path: String,
}

/// Publication state for a selected model entry.
#[derive(Clone, Debug, Serialize)]
pub struct Publication {
    /// `published` means the entry passed ingest and is present in the model.
    pub state: &'static str,
}

/// Normalized heading with its published fragment anchor.
#[derive(Clone, Debug, Serialize)]
pub struct HeadingReference {
    /// Heading level, 1 through 6.
    pub level: u8,
    /// Plain heading text.
    pub text: String,
    /// Signal's deterministic fragment id.
    pub id: String,
}

/// One normalized outbound Markdown link.
#[derive(Clone, Debug, Serialize)]
pub struct LinkReference {
    /// Destination as extracted from the normalized body.
    pub raw: String,
    /// `internal` or `external`.
    pub kind: &'static str,
    /// Resolved logical route or output-relative generated file, if internal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Fragment without `#`, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment: Option<String>,
}

/// One known inbound internal link.
#[derive(Clone, Debug, Serialize)]
pub struct InboundLinkReference {
    /// Source page that contains the link.
    pub source: SourceReference,
    /// Destination as authored in that source page.
    pub raw: String,
}

/// One page in Signal's existing related-entry projection.
#[derive(Clone, Debug, Serialize)]
pub struct RelatedReference {
    /// Stable source reference for the related page.
    pub source: SourceReference,
    /// URL-path form of the related page route.
    pub route: String,
    /// Related page title.
    pub title: String,
    /// Shared taxonomy terms that caused the page to be a candidate.
    pub shared_tags: Vec<String>,
}

/// One existing advisory diagnostic, without Signal's internal enum.
#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticReference {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Advisory severity: `warning` or `info`.
    pub severity: &'static str,
    /// Diagnostic subject, normally the selected route.
    pub subject: String,
    /// Measured fact, not editorial advice.
    pub message: String,
}

/// Resolved metadata and bounded context for one published page.
#[derive(Clone, Debug, Serialize)]
pub struct InspectedPage {
    /// Stable source identity, not the in-memory `ContentId`.
    pub source: SourceReference,
    /// Owning collection.
    pub collection: String,
    /// Logical route used by Signal's model.
    pub route: String,
    /// URL-path form used in rendered links and search documents.
    pub url: String,
    /// Canonical absolute URL, when `site.base_url` is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    /// Publication state.
    pub publication: Publication,
    /// Effective title.
    pub title: String,
    /// Author description, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Machine-readable publication date, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Machine-readable last-modification date, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Effective author: entry override, then configured site author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Effective hero image URL, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Hero alternative text, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_alt: Option<String>,
    /// Front-matter featured flag; actual home-hero eligibility also depends
    /// on the configured home collection.
    pub featured: bool,
    /// Taxonomy terms in effective authored display order.
    pub tags: Vec<String>,
    /// Markdown-derived word count used by Signal's reading-time projection.
    pub word_count: usize,
    /// Normalized headings and fragment ids.
    pub headings: Bounded<HeadingReference>,
    /// Resolved outbound links from the normalized body.
    pub outbound_links: Bounded<LinkReference>,
    /// Model-represented internal links pointing back to this page.
    pub inbound_links: Bounded<InboundLinkReference>,
    /// Referenced source assets, sorted and deduplicated by Signal.
    pub assets: Bounded<String>,
    /// Signal's existing page-local shared-tag related projection.
    pub related: Bounded<RelatedReference>,
    /// Existing advisory diagnostics whose subject is this route.
    pub diagnostics: Bounded<DiagnosticReference>,
}

/// Load, validate, and inspect one published page from disk.
///
/// The command runs the canonical check prefix: config loading, ingestion,
/// model freeze, structural validation, template loading, and reference
/// validation. It does not require an output directory and cannot write a
/// build artifact or manifest.
pub fn inspect_page_from_disk(root: &Path, page: &str) -> Result<Inspection, BuildError> {
    let loaded = crate::pipeline::load_validated_check(root)?;
    inspect_page(
        &loaded.config,
        &loaded.model,
        &loaded.validated.specs,
        root,
        page,
    )
}

/// Inspect one already-loaded and validated site state.
pub(crate) fn inspect_page(
    config: &signal_core::SignalConfig,
    model: &SiteModel,
    specs: &[signal_core::ArtifactSpec],
    root: &Path,
    page: &str,
) -> Result<Inspection, BuildError> {
    let entry = find_entry(model, config, page)?;
    let links = crate::link_check::resolve_page_links(model, specs, entry)?;
    let inbound = crate::link_check::inbound_page_links(model, specs, entry)?;
    let diagnostics = crate::diagnostics::analyze(root, config, model, specs)
        .into_iter()
        .filter(|diagnostic| diagnostic.subject() == entry.route.0)
        .map(|diagnostic| DiagnosticReference {
            code: diagnostic.code(),
            severity: diagnostic.severity().as_str(),
            subject: diagnostic.subject().to_string(),
            message: diagnostic.message(),
        })
        .collect();

    let headings = entry
        .body
        .headings
        .iter()
        .map(|heading| HeadingReference {
            level: heading.level,
            text: heading.text.clone(),
            id: heading.id.clone(),
        })
        .collect();
    let outbound = links
        .into_iter()
        .map(|link| LinkReference {
            raw: link.raw,
            kind: link.kind,
            target: link.target,
            fragment: link.fragment,
        })
        .collect();
    let inbound = inbound
        .into_iter()
        .map(|(source, raw)| InboundLinkReference {
            source: source_reference(&source),
            raw,
        })
        .collect();
    let related = related_references(config, model, entry)?;

    Ok(Inspection {
        schema: INSPECTION_SCHEMA,
        page: InspectedPage {
            source: source_reference(&entry.source),
            collection: entry.collection.0.clone(),
            route: entry.route.0.clone(),
            url: encode_route_path(&entry.route),
            canonical_url: config
                .site
                .base_url
                .as_deref()
                .map(|base| canonical_url(base, &entry.route)),
            publication: Publication { state: "published" },
            title: entry.title.clone(),
            description: entry.description.clone(),
            date: entry.date.clone(),
            last_modified: entry.last_modified.clone(),
            author: entry
                .author
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    config
                        .site
                        .author
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                }),
            image: entry.image.as_deref().map(image_src_url),
            image_alt: entry
                .image
                .as_deref()
                .and(entry.image_alt.as_deref())
                .map(str::to_string),
            featured: entry.featured,
            tags: entry.tag_order.clone(),
            word_count: entry.body.word_count,
            headings: Bounded::from_items(headings),
            outbound_links: Bounded::from_items(outbound),
            inbound_links: Bounded::from_items(inbound),
            assets: Bounded::from_items(signal_core::entry_asset_paths(entry)),
            related: Bounded::from_items(related),
            diagnostics: Bounded::from_items(diagnostics),
        },
    })
}

/// Serialize the inspection using the public schema's deterministic pretty JSON.
pub fn inspection_json(inspection: &Inspection) -> String {
    serde_json::to_string_pretty(inspection).expect("inspection contains owned serializable values")
}

fn find_entry<'a>(
    model: &'a SiteModel,
    config: &signal_core::SignalConfig,
    selector: &str,
) -> Result<&'a ContentEntry, BuildError> {
    let selector = selector.trim();
    if let Some(entry) = model.lookup_by_route(&Route::new(selector)) {
        return Ok(entry);
    }

    // A source reference is also accepted, but only as a model identity. No
    // filesystem lookup is performed here, so a selector cannot escape root
    // or disclose an unrelated file.
    for entry in model.entries() {
        let root_relative = format!(
            "{}/{}",
            config.source_dir_for(&entry.collection.0),
            entry.source.relative_path
        );
        if selector == root_relative {
            return Ok(entry);
        }
    }

    Err(BuildError::Model {
        message: format!("unknown or non-entry page {selector:?}"),
    })
}

fn source_reference(source: &signal_core::SourceRef) -> SourceReference {
    SourceReference {
        collection: source.collection.0.clone(),
        path: source.relative_path.clone(),
    }
}

fn related_references(
    config: &signal_core::SignalConfig,
    model: &SiteModel,
    entry: &ContentEntry,
) -> Result<Vec<RelatedReference>, BuildError> {
    let summaries: Vec<EntrySummary> = related_entries(
        model,
        entry,
        crate::manifest::related_limit(config),
        config.date_format_str(),
    );
    let mut out = Vec::new();
    for summary in summaries {
        let related_entry = model
            .entries()
            .find(|candidate| encode_route_path(&candidate.route) == summary.route)
            .ok_or_else(|| BuildError::Model {
                message: format!(
                    "related-entry projection returned unknown route {:?}",
                    summary.route
                ),
            })?;
        let shared_tags = entry
            .tags
            .intersection(&related_entry.tags)
            .cloned()
            .collect();
        out.push(RelatedReference {
            source: source_reference(&related_entry.source),
            route: summary.route,
            title: summary.title,
            shared_tags,
        });
    }
    Ok(out)
}
