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
