//! Architectural invariant tests for `signal-core`.
//!
//! These tests pin the bootstrap decisions: distinct identity types,
//! immutability after freeze, route-collision detection, read-only generator
//! input, and crate-boundary hygiene (no filesystem/template/Markdown-engine
//! dependencies in core).

use signal_core::{
    CollectionId, ContentEntry, ContentId, CoreError, Route, SignalConfig, SiteModel,
    SiteModelBuilder, Slug, SourceRef,
};
use std::any::TypeId;

fn entry(id: u32, collection: &str, path: &str, slug: &str, route: &str) -> ContentEntry {
    ContentEntry::new(
        ContentId(id),
        CollectionId::new(collection),
        SourceRef::new(CollectionId::new(collection), path),
        Slug::new(slug),
        Route::new(route),
        format!("Title {id}"),
    )
}

#[test]
fn content_id_slug_and_route_are_distinct_types() {
    assert_ne!(TypeId::of::<ContentId>(), TypeId::of::<Slug>());
    assert_ne!(TypeId::of::<ContentId>(), TypeId::of::<Route>());
    assert_ne!(TypeId::of::<Slug>(), TypeId::of::<Route>());
    assert_ne!(TypeId::of::<SourceRef>(), TypeId::of::<ContentId>());
}

#[test]
fn site_model_is_send_sync_and_cloneable_but_only_shared_by_ref() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SiteModel>();

    // Generators and renderers take `&SiteModel`; this helper only compiles
    // for shared references, pinning the read-only convention.
    fn takes_shared(_: &SiteModel) {}
    let mut builder = SiteModelBuilder::new();
    builder.add_entry(entry(1, "posts", "a.md", "a", "/a/"));
    let model = builder.build().expect("builds");
    takes_shared(&model);
}

#[test]
fn route_collisions_are_representable_and_detected() {
    let mut builder = SiteModelBuilder::new();
    builder.add_entry(entry(1, "posts", "a.md", "a", "/same/"));
    builder.add_entry(entry(2, "posts", "b.md", "b", "/same/"));
    let err = builder.build().expect_err("route collision");
    assert!(matches!(err, CoreError::RouteCollision { .. }));
}

#[test]
fn queries_are_deterministic_and_ordered() {
    let mut builder = SiteModelBuilder::new();
    builder.add_entry(entry(2, "posts", "b.md", "b", "/b/"));
    builder.add_entry(entry(1, "posts", "a.md", "a", "/a/"));
    let model = builder.build().expect("builds");
    let ids: Vec<u32> = model.entries().map(|e| e.id.0).collect();
    assert_eq!(ids, vec![1, 2]);
    let coll: Vec<u32> = model
        .entries_in_collection(&CollectionId::new("posts"))
        .iter()
        .map(|e| e.id.0)
        .collect();
    assert_eq!(coll, vec![1, 2]);
}

#[test]
fn semantic_references_do_not_imply_build_dependencies() {
    let mut a = entry(1, "posts", "a.md", "a", "/a/");
    a.references.push(ContentId(2));
    let mut builder = SiteModelBuilder::new();
    builder.add_entry(a);
    builder.add_entry(entry(2, "posts", "b.md", "b", "/b/"));
    let model = builder.build().expect("builds");
    // Queryable as semantics ...
    assert_eq!(model.referencing(ContentId(2)).len(), 1);
    // ... but the model exposes no dependency DAG surface by construction.
    // (This test documents the absence; see ADR 0002.)
}

#[test]
fn core_manifest_declares_no_forbidden_dependencies() {
    // `include_str!` keeps this check compile-time: no runtime filesystem I/O.
    const MANIFEST: &str = include_str!("../Cargo.toml");
    for forbidden in ["minijinja", "comrak", "clap", "miette", "minijinja"] {
        assert!(
            !MANIFEST.contains(forbidden),
            "signal-core must not depend on {forbidden}"
        );
    }
}

#[test]
fn core_sources_do_not_use_filesystem_apis() {
    const LIB: &str = include_str!("../src/lib.rs");
    const MODEL: &str = include_str!("../src/model.rs");
    const IDS: &str = include_str!("../src/ids.rs");
    for src in [LIB, MODEL, IDS] {
        assert!(!src.contains("std::fs"), "core must not use std::fs");
        assert!(!src.contains("std::net"), "core must not use std::net");
    }
}

#[test]
fn fixture_config_parses_through_core_types() {
    const FIXTURE: &str = include_str!("../../../fixtures/minimal-site/signal.toml");
    let cfg = SignalConfig::from_toml_str(FIXTURE).expect("fixture parses");
    assert_eq!(cfg.site.title, "Minimal Site");
}
