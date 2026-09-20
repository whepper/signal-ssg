//! Explicit build planning: what should be reused, rebuilt, and pruned.
//!
//! This module owns the pure planning concepts previously scattered across
//! `build.rs` (the reuse loop) and `manifest.rs` (input derivation, digest
//! comparison, stale subtraction). Planning determines **what should
//! happen**; `build_site` in `build.rs` executes those decisions (resolve,
//! write, prune, persist) without redefining the dependency contract.
//!
//! ```text
//! current site state (specs, config, model, renderer, previous manifest)
//!       │
//!       ▼
//!   plan() -> BuildPlan { decisions, stale }
//!       │
//!       ▼
//!  build execution (resolve rebuilds, skip reuses, prune stale, persist)
//! ```
//!
//! The planner performs no filesystem writes, deletions, manifest mutation,
//! or model mutation. It does perform the same bounded filesystem *reads*
//! the previous reuse predicate performed: static source bytes (to digest
//! current static inputs) and existing output bytes (to verify the recorded
//! output digest still holds). Those reads are inherent to content-hash
//! reuse — mtime is never consulted.
//!
//! # Dependency contract
//!
//! [`artifact_inputs`] is the single source of truth for "what one artifact
//! consumes". Both planning (reuse decisions) and recording (the new
//! manifest in `manifest::build_manifest`) call this one function, so the
//! two can never disagree.
//!
//! Resolution (`build::resolve_artifact`) remains a procedural mirror: it
//! implements *how* those inputs become bytes, while [`artifact_inputs`]
//! declares *which* inputs matter. The per-kind correspondence is:
//!
//! | `ArtifactKind` | `artifact_inputs` declares | `resolve_artifact` consumes |
//! |----------------|---------------------------|------------------------------|
//! | `Page` | `Entry{route}` + `Query{related:{route}}` + `TemplateSet` + `Config` | entry lookup + `entry_context` (incl. related) + selected template + config |
//! | `CollectionIndex` | `Query{summaries:{coll}}` (+ `Entry{route}` when a section-root entry exists) + `TemplateSet` + `Config` | `section_context` (summaries + optional section-root entry) + selected template + config |
//! | `Home` | `Query{home:{coll}}` + `TemplateSet` + `Config` | `home_context` (featured + recent) + selected template + config |
//! | `Taxonomy` (index) | `Query{topic_terms}` + `TemplateSet` + `Config` | `topic_terms` + index template + config |
//! | `Taxonomy` (term) | `Query{tagged:{label}}` + `TemplateSet` + `Config` | `topic_terms` term lookup + term template + config |
//! | `Rss` | `Query{feed:*}` (via shared `feed_identity`) + `Config` | `resolve_feed` via the same `feed_identity` + config |
//! | `Sitemap` | `Query{routes_inventory}` + `Config` | `resolve_sitemap` (same inventory + sections + taxonomy root) + config |
//! | `SearchIndex` | `Query{search_documents}` | `search_documents` (no config) |
//! | `Static` | `Static{path}` | `static/<path>` bytes verbatim |
//! | `Robots` | `Config` | `base_url` from config |
//! | `NotFound` | `TemplateSet` + `Config` | `resolve_not_found` (template set + config) |
//!
//! Shared derivations (`feed_identity`, `collection_for_section_route`,
//! `section_title`, template selection, `feed_limit`, `related_limit`,
//! taxonomy root/title) live in `manifest.rs` and are called verbatim by
//! both sides, so resolution and planning cannot disagree about *which*
//! collection, feed, term, or template an artifact names. Query *digests*
//! (`manifest::digest_query`) hash the exact consumed projections, so a
//! change in a consumed value invalidates even when the input list is
//! unchanged.
//!
//! Remaining mirror (documented, not eliminated): if a future resolver
//! starts consuming a new model/config value without adding the
//! corresponding `InputRef` here (or vice versa), reuse could go stale.
//! The `artifact_inputs_cover_resolution` test guards the structural part
//! of this (every resolvable spec yields inputs; every input kind resolves
//! against current state), and
//! `build::tests::unconsumed_entry_fields_do_not_affect_resolved_bytes`
//! guards the exclusion side (fields outside `EntryDigestInput` cannot
//! affect resolved bytes — consuming one fails the test and forces digest
//! + input coverage).
//!
//! Field-level drift inside a consumed projection must still be caught by
//! review: any change to `resolve_*` contexts or serializers must consider
//! `EntryDigestInput` / `digest_query` coverage.

use signal_core::{ArtifactSpec, Route, SignalConfig, SiteModel};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::errors::BuildError;
use crate::manifest::{
    collection_for_section_route, config_digest, digest_bytes, digest_json, digest_query,
    entry_digest, feed_identity, query_key, taxonomy_root, template_digests, Digest, FeedIdentity,
    InputRef, Manifest, PreviousManifest,
};

/// Why one artifact must be rebuilt.
///
/// These variants describe the existing reuse predicate's failure modes —
/// no new invalidation semantics. Variants are coarse where the predicate
/// is coarse (whole config digest, whole template set) and precise where
/// the predicate is precise (per-entry / per-query digests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebuildReason {
    /// No usable previous manifest (first build, unreadable, corrupt JSON,
    /// or unsupported schema). Full build by design.
    NoUsableManifest,
    /// The previous manifest's generation identity does not exactly match
    /// the running binary (engine version or behavior version).
    GenerationMismatch,
    /// No record exists under this output path in the previous manifest.
    MissingRecord,
    /// The recorded artifact kind differs from the planned kind.
    KindChanged,
    /// The canonical input-reference lists differ (dependency structure
    /// changed, e.g. a section gained/lost its section-root entry input).
    InputsChanged,
    /// The named entry's current digest differs from the recorded one.
    EntryChanged {
        /// Entry route whose digest changed.
        route: String,
    },
    /// The named query's current projection digest differs.
    QueryChanged {
        /// Query key whose digest changed.
        key: String,
    },
    /// The complete loaded template-set digest differs.
    TemplateSetChanged,
    /// A legacy per-template input digest differs (records written before
    /// slice 14C; current builds emit `TemplateSet` instead).
    TemplateChanged {
        /// Template name whose digest changed.
        name: String,
    },
    /// The whole-configuration digest differs.
    ConfigChanged,
    /// The static source's current bytes differ from the recorded output
    /// digest (verbatim passthrough).
    StaticChanged {
        /// Static path relative to `static/`.
        path: String,
    },
    /// The existing output file is missing, unreadable, or not a regular
    /// file (directories and symlinks never count as reusable outputs).
    OutputMissing,
    /// The existing output file's bytes do not hash to the recorded
    /// output digest (hand-edited, truncated, or replaced outputs rebuild).
    OutputChanged,
    /// Current inputs cannot be derived or digested (unknown query key,
    /// missing entry, unreadable static source, …). Any uncertainty
    /// rebuilds — never an error at plan time.
    InputError {
        /// What could not be proven unchanged.
        message: String,
    },
}

impl std::fmt::Display for RebuildReason {
    /// Human-readable reason text for diagnostics. Stable strings: no
    /// timestamps, no absolute paths, no formatting beyond the recorded
    /// route/key/name/path/message. Changing these strings changes
    /// `--explain` output only — never planning semantics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebuildReason::NoUsableManifest => write!(f, "no usable manifest"),
            RebuildReason::GenerationMismatch => write!(f, "generation changed"),
            RebuildReason::MissingRecord => write!(f, "manifest record missing"),
            RebuildReason::KindChanged => write!(f, "artifact kind changed"),
            RebuildReason::InputsChanged => write!(f, "inputs changed"),
            RebuildReason::EntryChanged { route } => write!(f, "entry changed: {route}"),
            RebuildReason::QueryChanged { key } => write!(f, "query changed: {key}"),
            RebuildReason::TemplateSetChanged => write!(f, "template set changed"),
            RebuildReason::TemplateChanged { name } => write!(f, "template changed: {name}"),
            RebuildReason::ConfigChanged => write!(f, "configuration changed"),
            RebuildReason::StaticChanged { path } => write!(f, "static file changed: {path}"),
            RebuildReason::OutputMissing => write!(f, "output missing"),
            RebuildReason::OutputChanged => write!(f, "output changed"),
            RebuildReason::InputError { message } => write!(f, "input error: {message}"),
        }
    }
}

/// One artifact's reuse/rebuild decision, with its spec inline so execution
/// never re-derives which artifact a decision belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildDecision {
    /// Skip resolve and write; carry the recorded output digest forward.
    Reuse {
        /// Planned artifact to reuse.
        spec: ArtifactSpec,
        /// Recorded output digest carried into the new manifest.
        output_digest: Digest,
    },
    /// Resolve and write; the reason names the failed predicate arm.
    Rebuild {
        /// Planned artifact to rebuild.
        spec: ArtifactSpec,
        /// Why reuse was refused.
        reason: RebuildReason,
    },
}

impl BuildDecision {
    /// The planned artifact this decision belongs to.
    pub fn spec(&self) -> &ArtifactSpec {
        match self {
            BuildDecision::Reuse { spec, .. } => spec,
            BuildDecision::Rebuild { spec, .. } => spec,
        }
    }

    /// Whether this decision reuses a previous output.
    pub fn is_reuse(&self) -> bool {
        matches!(self, BuildDecision::Reuse { .. })
    }

    /// The rebuild reason, if this decision rebuilds.
    pub fn reason(&self) -> Option<&RebuildReason> {
        match self {
            BuildDecision::Reuse { .. } => None,
            BuildDecision::Rebuild { reason, .. } => Some(reason),
        }
    }
}

/// The complete pure plan for one build: per-artifact decisions plus the
/// stale-output inventory. Execution consumes this without redefining it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildPlan {
    /// One decision per current spec, in plan (sorted-path) order.
    pub decisions: Vec<BuildDecision>,
    /// Previous-manifest paths absent from the current plan, sorted.
    /// Empty when there is no usable previous manifest (nothing may be
    /// pruned from corrupt metadata).
    pub stale: Vec<String>,
    /// Current template digests shared by all decisions (and recording).
    pub templates: BTreeMap<String, Digest>,
    /// Current whole-configuration digest shared by all decisions.
    pub config_digest: Digest,
}

impl BuildPlan {
    /// Decisions that reuse a previous output.
    pub fn reuses(&self) -> impl Iterator<Item = &BuildDecision> {
        self.decisions.iter().filter(|d| d.is_reuse())
    }

    /// Decisions that must rebuild.
    pub fn rebuilds(&self) -> impl Iterator<Item = &BuildDecision> {
        self.decisions.iter().filter(|d| !d.is_reuse())
    }

    /// Number of reused artifacts.
    pub fn reused_count(&self) -> usize {
        self.decisions.iter().filter(|d| d.is_reuse()).count()
    }

    /// Number of rebuilt artifacts.
    pub fn rebuilt_count(&self) -> usize {
        self.decisions.iter().filter(|d| !d.is_reuse()).count()
    }

    /// Paths of artifacts to rebuild, in plan order.
    pub fn rebuild_paths(&self) -> Vec<String> {
        self.decisions
            .iter()
            .filter(|d| !d.is_reuse())
            .map(|d| d.spec().path.clone())
            .collect()
    }
}

/// Derive one artifact's input references from its spec, config, and model.
///
/// This is the single source of truth for "what an artifact consumes".
/// Both [`plan`] (reuse decisions) and `manifest::build_manifest`
/// (recording) call this one function. It mirrors `resolve_artifact`'s
/// per-kind dispatch exactly — see the module-level contract table — so a
/// change to what resolution consumes must change what is declared here.
pub(crate) fn artifact_inputs(
    spec: &ArtifactSpec,
    config: &SignalConfig,
    model: &SiteModel,
) -> Result<Vec<InputRef>, BuildError> {
    use signal_core::ArtifactKind;
    match spec.kind {
        ArtifactKind::Page => {
            let route = spec.route.as_ref().ok_or_else(|| BuildError::Model {
                message: format!("artifact {:?} has no route", spec.path),
            })?;
            let entry = model
                .lookup_by_route(route)
                .ok_or_else(|| BuildError::Model {
                    message: format!("no entry for route {route}"),
                })?;
            Ok(vec![
                InputRef::Entry {
                    route: entry.route.0.clone(),
                },
                // Related summaries consume *other* entries (their titles,
                // dates, tags); the projection digest covers them so a
                // change in any related entry invalidates this page.
                InputRef::Query {
                    key: query_key::related(&entry.route.0),
                },
                InputRef::TemplateSet,
                InputRef::Config,
            ])
        }
        ArtifactKind::CollectionIndex => {
            let route = spec.route.as_ref().ok_or_else(|| BuildError::Model {
                message: format!("artifact {:?} has no route", spec.path),
            })?;
            let collection =
                collection_for_section_route(config, model, route).ok_or_else(|| {
                    BuildError::Model {
                        message: format!("no collection for section route {route}"),
                    }
                })?;
            let mut inputs = vec![
                InputRef::Query {
                    key: query_key::summaries(&collection),
                },
                InputRef::TemplateSet,
                InputRef::Config,
            ];
            if model.lookup_by_route(route).is_some() {
                inputs.insert(
                    0,
                    InputRef::Entry {
                        route: route.0.clone(),
                    },
                );
            }
            Ok(inputs)
        }
        ArtifactKind::Home => {
            let collection =
                config
                    .site
                    .home_collection
                    .clone()
                    .ok_or_else(|| BuildError::Model {
                        message: "home artifact without home configuration".to_string(),
                    })?;
            Ok(vec![
                InputRef::Query {
                    key: query_key::home(&collection),
                },
                InputRef::TemplateSet,
                InputRef::Config,
            ])
        }
        ArtifactKind::Taxonomy => {
            let route = spec.route.as_ref().ok_or_else(|| BuildError::Model {
                message: format!("artifact {:?} has no route", spec.path),
            })?;
            let tax_root = taxonomy_root(config).ok_or_else(|| BuildError::Model {
                message: "taxonomy artifact without taxonomy configuration".to_string(),
            })?;
            if route.0 == tax_root {
                Ok(vec![
                    InputRef::Query {
                        key: query_key::topic_terms(),
                    },
                    InputRef::TemplateSet,
                    InputRef::Config,
                ])
            } else {
                let terms =
                    signal_generators::topic_terms(model, &tax_root, config.date_format_str())
                        .map_err(|e| BuildError::Model {
                            message: e.to_string(),
                        })?;
                let term =
                    terms
                        .iter()
                        .find(|t| t.route == route.0)
                        .ok_or_else(|| BuildError::Model {
                            message: format!("no topic for route {route}"),
                        })?;
                Ok(vec![
                    InputRef::Query {
                        key: query_key::tagged(&term.label),
                    },
                    InputRef::TemplateSet,
                    InputRef::Config,
                ])
            }
        }
        ArtifactKind::Rss => {
            // One dispatch rule (shared with resolution): the path's feed
            // identity determines the query whose consumed projection is
            // the artifact's input.
            let key = match feed_identity(&spec.path, config) {
                Some(FeedIdentity::Main) => query_key::feed_main(),
                Some(FeedIdentity::Section { collection }) => query_key::feed_section(&collection),
                Some(FeedIdentity::TaxonomyLabels) => query_key::feed_labels(),
                Some(FeedIdentity::Term { slug }) => query_key::feed_term(&slug),
                None => {
                    return Err(BuildError::Model {
                        message: format!("unrecognized feed path {:?}", spec.path),
                    })
                }
            };
            Ok(vec![InputRef::Query { key }, InputRef::Config])
        }
        ArtifactKind::Sitemap => Ok(vec![
            InputRef::Query {
                key: query_key::routes_inventory(),
            },
            InputRef::Config,
        ]),
        ArtifactKind::SearchIndex => Ok(vec![InputRef::Query {
            key: query_key::search_documents(),
        }]),
        ArtifactKind::Static => Ok(vec![InputRef::Static {
            path: spec.path.clone(),
        }]),
        // robots.txt is a pure function of the base URL (config), and the
        // themed 404 page is rendered from the template set plus config;
        // neither consumes entries or queries.
        ArtifactKind::Robots => Ok(vec![InputRef::Config]),
        ArtifactKind::NotFound => Ok(vec![InputRef::TemplateSet, InputRef::Config]),
    }
}

/// Canonical form of an input list: each reference serialized, then sorted.
///
/// Both sides of a comparison go through this, so construction-order
/// differences can never cause false invalidations — only genuinely
/// different dependency sets mismatch.
pub(crate) fn canonical_inputs(inputs: &[InputRef]) -> Vec<String> {
    let mut keys: Vec<String> = inputs
        .iter()
        .map(|input| serde_json::to_string(input).expect("owned inputs always serialize"))
        .collect();
    keys.sort();
    keys
}

/// Digest one input reference against current build state.
///
/// `templates` and `config_digest` are precomputed once per build; entries
/// and queries digest on demand from the model. Static sources hash their
/// current bytes. Any failure (missing data, unreadable source) is an
/// error the caller treats as "cannot prove unchanged" — never as success.
fn current_input_digest(
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    templates: &BTreeMap<String, Digest>,
    config_digest: &Digest,
    input: &InputRef,
) -> Result<Digest, BuildError> {
    match input {
        InputRef::Entry { route } => {
            let entry = model
                .lookup_by_route(&Route::new(route.clone()))
                .ok_or_else(|| BuildError::Model {
                    message: format!("no entry for route {route}"),
                })?;
            Ok(entry_digest(entry))
        }
        InputRef::Query { key } => digest_query(config, model, key),
        InputRef::Template { name } => {
            templates
                .get(name)
                .cloned()
                .ok_or_else(|| BuildError::Model {
                    message: format!("template {name:?} not loaded"),
                })
        }
        InputRef::TemplateSet => Ok(digest_json(templates)),
        InputRef::Config => Ok(config_digest.clone()),
        InputRef::Static { path } => {
            // Static resolve reads `static/<path>` under the site root;
            // digest the same bytes. (Containment holds: spec paths passed
            // plan validation, and this join never escapes `static/` for
            // validated components.)
            let source = root.join("static").join(path);
            std::fs::read(&source).map_err(|e| BuildError::Read {
                path: source.display().to_string(),
                message: e.to_string(),
            })
        }
        .map(|bytes| digest_bytes(&bytes)),
    }
}

/// Digest the existing output file, if it is a regular file.
///
/// Directories, symlinks, and missing or unreadable files yield `None`
/// (→ rebuild). Never trusts mtime or size: only exact bytes.
fn existing_output_digest(out_dir: &Path, relative: &str) -> Option<Digest> {
    let path = out_dir.join(relative);
    let file_type = std::fs::symlink_metadata(&path).ok()?.file_type();
    if !file_type.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(digest_bytes(&bytes))
}

/// Compute stale artifact paths: previous manifest records absent from the
/// current plan, sorted.
///
/// Pure inventory subtraction on normalized artifact paths — no filesystem
/// access, no dependency analysis, no graph. Execution decides what to do
/// with the result (safety-screen, alias-guard, delete); this function only
/// identifies it deterministically.
pub(crate) fn stale_artifact_paths(prev: &Manifest, specs: &[ArtifactSpec]) -> Vec<String> {
    let current: BTreeSet<&str> = specs.iter().map(|spec| spec.path.as_str()).collect();
    prev.artifacts
        .keys()
        .filter(|path| !current.contains(path.as_str()))
        .cloned()
        .collect()
}

/// Decide one artifact's fate against a usable previous manifest.
///
/// Total function with the same semantics as the historical
/// `artifact_reusable` predicate, but returning the *reason* instead of a
/// bare boolean. Any uncertainty rebuilds — never an error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decide_artifact(
    spec: &ArtifactSpec,
    config: &SignalConfig,
    root: &Path,
    model: &SiteModel,
    templates: &BTreeMap<String, Digest>,
    config_digest: &Digest,
    prev: &Manifest,
    out_dir: &Path,
) -> BuildDecision {
    use RebuildReason as Reason;
    // Build-wide compatibility gate: generation behavior is not an artifact
    // dependency, so it is checked before any per-artifact input comparison.
    if !crate::manifest::generation_compatible(prev) {
        return BuildDecision::Rebuild {
            spec: spec.clone(),
            reason: Reason::GenerationMismatch,
        };
    }
    let record = match prev.artifacts.get(&spec.path) {
        Some(record) => record,
        None => {
            return BuildDecision::Rebuild {
                spec: spec.clone(),
                reason: Reason::MissingRecord,
            };
        }
    };
    if record.kind != spec.kind {
        return BuildDecision::Rebuild {
            spec: spec.clone(),
            reason: Reason::KindChanged,
        };
    }
    let current_inputs = match artifact_inputs(spec, config, model) {
        Ok(inputs) => inputs,
        Err(e) => {
            return BuildDecision::Rebuild {
                spec: spec.clone(),
                reason: Reason::InputError {
                    message: e.to_string(),
                },
            };
        }
    };
    if canonical_inputs(&current_inputs) != canonical_inputs(&record.inputs) {
        return BuildDecision::Rebuild {
            spec: spec.clone(),
            reason: Reason::InputsChanged,
        };
    }
    // The recorded complete template set, digested once for `TemplateSet`
    // inputs.
    let previous_template_set = digest_json(&prev.templates);
    for input in &current_inputs {
        let current =
            match current_input_digest(config, root, model, templates, config_digest, input) {
                Ok(digest) => digest,
                Err(e) => {
                    return BuildDecision::Rebuild {
                        spec: spec.clone(),
                        reason: Reason::InputError {
                            message: e.to_string(),
                        },
                    };
                }
            };
        let previous = match input {
            InputRef::Entry { route } => prev.entries.get(route),
            InputRef::Query { key } => prev.queries.get(key),
            InputRef::Template { name } => prev.templates.get(name),
            InputRef::TemplateSet => Some(&previous_template_set),
            InputRef::Config => Some(&prev.config_digest),
            // Static sources have no digest map: the recorded output
            // digest IS the source digest (verbatim passthrough).
            InputRef::Static { .. } => Some(&record.output_digest),
        };
        if previous != Some(&current) {
            let reason = match input {
                InputRef::Entry { route } => Reason::EntryChanged {
                    route: route.clone(),
                },
                InputRef::Query { key } => Reason::QueryChanged { key: key.clone() },
                InputRef::Template { name } => Reason::TemplateChanged { name: name.clone() },
                InputRef::TemplateSet => Reason::TemplateSetChanged,
                InputRef::Config => Reason::ConfigChanged,
                InputRef::Static { path } => Reason::StaticChanged { path: path.clone() },
            };
            return BuildDecision::Rebuild {
                spec: spec.clone(),
                reason,
            };
        }
    }
    match existing_output_digest(out_dir, &spec.path) {
        Some(digest) if digest == record.output_digest => BuildDecision::Reuse {
            spec: spec.clone(),
            output_digest: record.output_digest.clone(),
        },
        Some(_) => BuildDecision::Rebuild {
            spec: spec.clone(),
            reason: Reason::OutputChanged,
        },
        None => BuildDecision::Rebuild {
            spec: spec.clone(),
            reason: Reason::OutputMissing,
        },
    }
}

/// Plan one build: per-artifact reuse/rebuild decisions plus the stale
/// inventory.
///
/// `specs` must already be validated (output paths, routes) and in
/// deterministic order. The previous manifest is consulted but never
/// mutated. Template and config digests are computed once here and carried
/// on the plan for execution/recording. Lookup keys are always current spec
/// paths, so corrupt manifest paths can never redirect a read or a write.
pub(crate) fn plan(
    specs: &[ArtifactSpec],
    config: &SignalConfig,
    model: &SiteModel,
    renderer: &signal_render::MiniJinjaRenderer,
    root: &Path,
    out_dir: &Path,
    previous: &PreviousManifest,
) -> Result<BuildPlan, BuildError> {
    let templates = template_digests(renderer)?;
    let config_digest = config_digest(config);
    let usable_prev = match previous {
        PreviousManifest::Usable(manifest) => Some(manifest),
        _ => None,
    };
    let mut decisions = Vec::with_capacity(specs.len());
    for spec in specs {
        let decision = match usable_prev {
            Some(prev) => decide_artifact(
                spec,
                config,
                root,
                model,
                &templates,
                &config_digest,
                prev,
                out_dir,
            ),
            None => BuildDecision::Rebuild {
                spec: spec.clone(),
                reason: RebuildReason::NoUsableManifest,
            },
        };
        decisions.push(decision);
    }
    let stale = match usable_prev {
        Some(prev) => stale_artifact_paths(prev, specs),
        None => Vec::new(),
    };
    Ok(BuildPlan {
        decisions,
        stale,
        templates,
        config_digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_site(dir: &Path, files: &[(&str, &str)]) {
        for (rel, content) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, content).expect("write");
        }
    }

    fn plan_site(dir: &Path) {
        write_site(
            dir,
            &[
                (
                    "signal.toml",
                    "[site]\ntitle = \"Plan\"\nbase_url = \"https://example.com/\"\n[collections.posts]\nsource = \"content/posts\"\nroute_prefix = \"/posts/\"\n[collections.notes]\nsource = \"content/notes\"\nroute_prefix = \"/notes/\"\n[taxonomy]\nroute_prefix = \"/topics/\"\n[feed]\n",
                ),
                (
                    "content/posts/alpha.md",
                    "---\ntitle: Alpha\ndate: 2026-02-01\ntopics: [\"Rust\"]\n---\n\nAlpha body words here.\n",
                ),
                (
                    "content/posts/beta.md",
                    "---\ntitle: Beta\ndate: 2026-03-01\ntopics: [\"Rust\"]\n---\n\nBeta body words here.\n",
                ),
                (
                    "content/notes/n.md",
                    "---\ntitle: N\ndate: 2026-01-01\ntopics: [\"Misc\"]\n---\n\nNote body words here.\n",
                ),
                (
                    "templates/post.html",
                    "<html><body>{{ content | safe }}</body></html>",
                ),
                (
                    "templates/section.html",
                    "<html><body>section</body></html>",
                ),
                (
                    "templates/topics.html",
                    "<html><body>topics</body></html>",
                ),
                (
                    "templates/topic.html",
                    "<html><body>topic</body></html>",
                ),
                ("static/asset.txt", "asset-1"),
            ],
        );
    }

    struct PlanCtx {
        root: std::path::PathBuf,
        out: std::path::PathBuf,
        config: SignalConfig,
        model: SiteModel,
        renderer: signal_render::MiniJinjaRenderer,
        specs: Vec<ArtifactSpec>,
        _dir: tempfile::TempDir,
        _out: tempfile::TempDir,
    }

    impl PlanCtx {
        fn build() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let out = tempfile::tempdir().expect("tempdir");
            plan_site(dir.path());
            let summary = crate::build::build_site_from_disk(dir.path(), &out.path().join("out"))
                .expect("first builds");
            let out_dir = out.path().join("out");
            let text =
                std::fs::read_to_string(dir.path().join("signal.toml")).expect("config text");
            let config: SignalConfig = toml::from_str(&text).expect("config parses");
            let (model, _) = crate::ingest::ingest_site(dir.path(), &config).expect("model");
            let renderer = crate::build::load_templates(dir.path()).expect("renderer");
            Self {
                root: dir.path().to_path_buf(),
                out: out_dir,
                config,
                model,
                renderer,
                specs: summary.specs,
                _dir: dir,
                _out: out,
            }
        }

        fn previous(&self) -> PreviousManifest {
            crate::manifest::load_previous(&self.out)
        }

        fn plan(&self, previous: &PreviousManifest) -> BuildPlan {
            plan(
                &self.specs,
                &self.config,
                &self.model,
                &self.renderer,
                &self.root,
                &self.out,
                previous,
            )
            .expect("plans")
        }

        fn decide(&self, path: &str, previous: &Manifest) -> BuildDecision {
            let spec = self
                .specs
                .iter()
                .find(|s| s.path == path)
                .expect("spec exists")
                .clone();
            decide_artifact(
                &spec,
                &self.config,
                &self.root,
                &self.model,
                &template_digests(&self.renderer).expect("digests"),
                &config_digest(&self.config),
                previous,
                &self.out,
            )
        }
    }

    fn usable(ctx: &PlanCtx) -> Manifest {
        match ctx.previous() {
            PreviousManifest::Usable(manifest) => manifest,
            other => panic!("expected usable manifest, got {other:?}"),
        }
    }

    #[test]
    fn identical_build_reuses_everything() {
        let ctx = PlanCtx::build();
        let planned = ctx.plan(&ctx.previous());
        assert!(planned.stale.is_empty());
        assert_eq!(planned.rebuilt_count(), 0);
        assert_eq!(planned.reused_count(), ctx.specs.len());
        assert!(planned.decisions.iter().all(|d| d.is_reuse()));
    }

    #[test]
    fn unchanged_static_asset_reuses() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        match ctx.decide("asset.txt", &manifest) {
            BuildDecision::Reuse { spec, .. } => assert_eq!(spec.path, "asset.txt"),
            other => panic!("static must reuse, got {other:?}"),
        }
        let planned = ctx.plan(&ctx.previous());
        assert!(planned.rebuild_paths().is_empty());
    }

    #[test]
    fn unchanged_rendered_artifact_reuses() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        match ctx.decide("posts/alpha/index.html", &manifest) {
            BuildDecision::Reuse { .. } => {}
            other => panic!("page must reuse, got {other:?}"),
        }
        match ctx.decide("posts/index.html", &manifest) {
            BuildDecision::Reuse { .. } => {}
            other => panic!("section must reuse, got {other:?}"),
        }
    }

    #[test]
    fn absent_manifest_rebuilds_everything_with_reason() {
        let ctx = PlanCtx::build();
        let planned = ctx.plan(&PreviousManifest::Absent);
        assert_eq!(planned.rebuilt_count(), ctx.specs.len());
        assert!(planned.stale.is_empty());
        for decision in &planned.decisions {
            assert_eq!(
                decision.reason(),
                Some(&RebuildReason::NoUsableManifest),
                "got {decision:?}"
            );
        }
    }

    #[test]
    fn unusable_manifest_rebuilds_everything_without_pruning() {
        let ctx = PlanCtx::build();
        let previous = PreviousManifest::Unusable {
            reason: "corrupt".to_string(),
        };
        let planned = ctx.plan(&previous);
        assert_eq!(planned.rebuilt_count(), ctx.specs.len());
        assert!(planned.stale.is_empty());
        assert!(planned
            .decisions
            .iter()
            .all(|d| d.reason() == Some(&RebuildReason::NoUsableManifest)));
    }

    #[test]
    fn generation_mismatch_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.generation = None;
        assert!(matches!(
            ctx.decide("posts/alpha/index.html", &manifest),
            BuildDecision::Rebuild {
                reason: RebuildReason::GenerationMismatch,
                ..
            }
        ));
    }

    #[test]
    fn missing_record_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.artifacts.remove("posts/alpha/index.html");
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::MissingRecord),
        );
    }

    #[test]
    fn kind_mismatch_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        if let Some(record) = manifest.artifacts.get_mut("posts/alpha/index.html") {
            record.kind = signal_core::ArtifactKind::CollectionIndex;
        }
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::KindChanged),
        );
    }

    #[test]
    fn canonical_input_structure_change_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        // Structurally alter the recorded inputs without touching digests:
        // drop the page's related-query input.
        if let Some(record) = manifest.artifacts.get_mut("posts/alpha/index.html") {
            record.inputs.retain(|input| {
                !matches!(
                    input,
                    InputRef::Query { key } if key.starts_with("related:")
                )
            });
        }
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::InputsChanged),
        );
    }

    #[test]
    fn entry_change_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.entries.insert(
            "/posts/alpha/".to_string(),
            crate::manifest::digest_bytes(b"tampered"),
        );
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::EntryChanged {
                route: "/posts/alpha/".to_string(),
            }),
        );
        // The section listing does not name the entry directly.
        assert!(ctx.decide("posts/index.html", &manifest).is_reuse());
    }

    #[test]
    fn query_change_rebuilds_dependents_only() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.queries.insert(
            "summaries:posts".to_string(),
            crate::manifest::digest_bytes(b"tampered"),
        );
        assert_eq!(
            ctx.decide("posts/index.html", &manifest).reason(),
            Some(&RebuildReason::QueryChanged {
                key: "summaries:posts".to_string(),
            }),
        );
        // The alpha page names Entry+related+TemplateSet+Config, not the
        // section summaries query.
        assert!(ctx.decide("posts/alpha/index.html", &manifest).is_reuse());
    }

    #[test]
    fn template_change_rebuilds_rendered_artifacts_only() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.templates.insert(
            "post.html".to_string(),
            crate::manifest::digest_bytes(b"tampered"),
        );
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::TemplateSetChanged),
        );
        // Feeds, sitemap, search, and statics record no template input.
        assert!(ctx.decide("index.xml", &manifest).is_reuse());
        assert!(ctx.decide("index.json", &manifest).is_reuse());
        assert!(ctx.decide("asset.txt", &manifest).is_reuse());
    }

    #[test]
    fn config_change_rebuilds_config_consumers() {
        let ctx = PlanCtx::build();
        let mut manifest = usable(&ctx);
        manifest.config_digest = crate::manifest::digest_bytes(b"tampered");
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::ConfigChanged),
        );
        // The search index names only its query (it consumes no config).
        assert!(ctx.decide("index.json", &manifest).is_reuse());
    }

    #[test]
    fn missing_output_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        std::fs::remove_file(ctx.out.join("posts/alpha/index.html")).expect("delete output");
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::OutputMissing),
        );
    }

    #[test]
    fn modified_output_rebuilds_with_reason() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        std::fs::write(
            ctx.out.join("posts/alpha/index.html"),
            "<html>touched</html>",
        )
        .expect("touch output");
        assert_eq!(
            ctx.decide("posts/alpha/index.html", &manifest).reason(),
            Some(&RebuildReason::OutputChanged),
        );
    }

    #[test]
    fn removed_artifact_becomes_stale() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        let mut specs = ctx.specs.clone();
        specs.retain(|s| s.path != "posts/alpha/index.html");
        assert_eq!(
            stale_artifact_paths(&manifest, &specs),
            vec!["posts/alpha/index.html"]
        );
        let mut full = ctx.specs.clone();
        full.retain(|_| true);
        assert!(stale_artifact_paths(&manifest, &full).is_empty());
    }

    #[test]
    fn stale_detection_is_plan_minus_manifest_sorted() {
        let ctx = PlanCtx::build();
        let manifest = usable(&ctx);
        let stale = stale_artifact_paths(&manifest, &[]);
        let mut sorted = stale.clone();
        sorted.sort();
        assert_eq!(stale, sorted);
        assert_eq!(stale.len(), manifest.artifacts.len());
    }

    #[test]
    fn artifact_inputs_cover_resolution() {
        // Structural guard against resolve↔inputs drift: every planned
        // spec must yield a non-empty input declaration, every declared
        // input must digest against current state, and every resolvable
        // spec must be plannable (no silent skew between the two).
        let ctx = PlanCtx::build();
        for spec in &ctx.specs {
            let inputs = artifact_inputs(spec, &ctx.config, &ctx.model).expect("inputs derive");
            assert!(!inputs.is_empty(), "spec {} names no inputs", spec.path);
            for input in &inputs {
                let digest = match input {
                    InputRef::Entry { route } => Some(entry_digest(
                        ctx.model
                            .lookup_by_route(&Route::new(route.clone()))
                            .expect("entry exists"),
                    )),
                    InputRef::Query { key } => {
                        Some(digest_query(&ctx.config, &ctx.model, key).expect("query digests"))
                    }
                    InputRef::TemplateSet => Some(digest_json(
                        &template_digests(&ctx.renderer).expect("templates"),
                    )),
                    InputRef::Config => Some(config_digest(&ctx.config)),
                    InputRef::Static { path } => {
                        let bytes = std::fs::read(ctx.root.join("static").join(path))
                            .expect("static reads");
                        Some(digest_bytes(&bytes))
                    }
                    InputRef::Template { .. } => None,
                };
                assert!(digest.is_some(), "input {input:?} must digest");
            }
            // Resolution must agree the spec is well-formed.
            crate::build::resolve_artifact(spec, &ctx.config, &ctx.root, &ctx.model, &ctx.renderer)
                .expect("resolves");
        }
    }
}
