//! `signal` command-line interface: ingest, generate, render, write.

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use std::path::PathBuf;

/// Signal SSG: deterministic static site compiler.
#[derive(Debug, Parser)]
#[command(name = "signal", version, about = "Signal static site generator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Build a site root into an output directory.
    Build {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output directory.
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        /// Explain the build plan without writing, pruning, or persisting:
        /// prints which artifacts would be reused, which would be rebuilt
        /// and why, and which stale outputs would be pruned.
        #[arg(long, default_value_t = false)]
        explain: bool,
    },
    /// Validate `signal.toml`, internal references, and report collections.
    Check {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Explain the build plan or one artifact without building.
    ///
    /// Without a target this prints the same plan `build --explain`
    /// prints; with an asset target (`images/hero.jpg`, `/images/hero.jpg`,
    /// or `static/images/hero.jpg`) it prints that asset's source, type,
    /// size, referrers, output, and reuse/rebuild decision. With `--width`
    /// (and optionally `--format`, default `webp`) it instead explains the
    /// requested image derivative: source, input and output dimensions,
    /// output path, dependencies, and reuse/rebuild decision. A derivative
    /// output path (`images/hero-640.webp`), a generated social card path
    /// (`social/posts/example.png`), or any other planned artifact output
    /// path (`index.json`, `sitemap.xml`, `robots.txt`, `404.html`) is
    /// explained directly; the search index also reports its document count.
    Explain {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output directory (for manifest-aware reuse/rebuild decisions).
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        /// Artifact to explain (a source asset, derivative output, social
        /// card, or any planned output path such as `index.json`). Omit for
        /// the whole plan.
        target: Option<String>,
        /// Derivative width to explain (requires `target`).
        #[arg(long)]
        width: Option<u32>,
        /// Derivative format to explain (requires `target`; default `webp`).
        #[arg(long)]
        format: Option<String>,
    },
    /// Inspect one published page's resolved, bounded context as JSON.
    Inspect {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Page route (for example `/posts/hello/`) or model source reference.
        page: String,
        /// Machine-readable output format. Only `json` is currently public.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Serve a site root locally with rebuilds on source changes.
    Serve {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output directory to generate and serve.
        #[arg(long, default_value = "dist")]
        out: PathBuf,
        /// Interface to bind.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind (`0` picks an ephemeral port and reports it).
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { root, out, explain } => {
            if explain {
                let text =
                    signal_cli::explain::explain_site_from_disk(&root, &out).into_diagnostic()?;
                print!("{text}");
                return Ok(());
            }
            let summary = signal_cli::build_site_from_disk(&root, &out).into_diagnostic()?;
            println!("Signal build complete");
            println!("  planned: {}", summary.specs.len());
            println!("  reused: {}", summary.reused);
            println!("  rebuilt: {}", summary.rebuilt);
            println!("  pruned: {}", summary.pruned);
            println!("  pages written: {}", summary.pages_written);
            println!("  drafts skipped: {}", summary.drafts_skipped);
            println!("  static files: {}", summary.static_files);
            println!("  derived images: {}", summary.derived_images);
            println!("  social images: {}", summary.social_images);
            println!("  out: {}", out.display());
            Ok(())
        }
        Commands::Check { root } => {
            // Configuration, ingestion, spec generation, and internal
            // reference validation — no artifacts resolved or written,
            // nothing pruned, manifest untouched.
            let report = signal_cli::link_check::check_site_from_disk(&root).into_diagnostic()?;
            println!("site: {}", report.title);
            for id in &report.collections {
                println!("collection: {id}");
            }
            println!(
                "references: {} checked ({} external skipped)",
                report.references.checked, report.references.external_skipped
            );
            println!(
                "assets: {} discovered ({} referenced, {} resolved, {} missing, {} unsafe; {} derivatives, {} social images)",
                report.assets.discovered,
                report.assets.referenced,
                report.assets.resolved,
                report.assets.missing,
                report.assets.unsafe_paths,
                report.assets.derivatives,
                report.assets.social_images,
            );
            // A6 diagnostics: advisory observations, never failures. The
            // summary stays one line for a clean site; details render only
            // when there is something to report.
            use signal_cli::diagnostics::{self, Severity};
            let warnings = diagnostics::count(&report.diagnostics, Severity::Warning);
            let infos = diagnostics::count(&report.diagnostics, Severity::Info);
            println!(
                "diagnostics: {}, {}",
                diagnostics::label(warnings, Severity::Warning),
                diagnostics::label(infos, Severity::Info),
            );
            if !report.diagnostics.is_empty() {
                print!("{}", diagnostics::render(&report.diagnostics));
            }
            Ok(())
        }
        Commands::Explain {
            root,
            out,
            target,
            width,
            format,
        } => {
            match target {
                Some(target) => {
                    // A social card is explained by path alone: `--width`
                    // describes image derivatives, so pairing the two is a
                    // usage error rather than a confusing plan lookup.
                    let social_target = signal_cli::assets::normalize_asset_target(&target)
                        .ok()
                        .and_then(|normalized| signal_core::social_image_route(&normalized))
                        .is_some();
                    if social_target && (width.is_some() || format.is_some()) {
                        return Err(miette::miette!(
                            "social images are explained by path alone: drop --width/--format"
                        ));
                    }
                    let text = match width {
                        Some(width) => signal_cli::explain::explain_derivative_from_disk(
                            &root,
                            &out,
                            &target,
                            width,
                            format.as_deref(),
                        )
                        .into_diagnostic()?,
                        None => {
                            if format.is_some() {
                                return Err(miette::miette!(
                                    "--format requires --width (both describe one derivative)"
                                ));
                            }
                            signal_cli::explain::explain_asset_from_disk(&root, &out, &target)
                                .into_diagnostic()?
                        }
                    };
                    print!("{text}");
                }
                None => {
                    if width.is_some() || format.is_some() {
                        return Err(miette::miette!("--width/--format require an asset target"));
                    }
                    let text = signal_cli::explain::explain_site_from_disk(&root, &out)
                        .into_diagnostic()?;
                    print!("{text}");
                }
            }
            Ok(())
        }
        Commands::Inspect { root, page, format } => {
            if format != "json" {
                return Err(miette::miette!(
                    "unsupported inspect format {format:?}; use --format json"
                ));
            }
            let inspection =
                signal_cli::inspect::inspect_page_from_disk(&root, &page).into_diagnostic()?;
            println!("{}", signal_cli::inspect::inspection_json(&inspection));
            Ok(())
        }
        Commands::Serve {
            root,
            out,
            host,
            port,
        } => {
            let options = signal_cli::serve::ServeOptions {
                root,
                out,
                host,
                port,
                debounce: signal_cli::serve::DEFAULT_DEBOUNCE,
            };
            signal_cli::serve::serve_forever(&options).into_diagnostic()?;
            Ok(())
        }
    }
}
