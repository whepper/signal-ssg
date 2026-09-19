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
    },
    /// Validate `signal.toml` and report collections.
    Check {
        /// Site root containing `signal.toml`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { root, out } => {
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
            let config_path = root.join("signal.toml");
            let cfg = signal_cli::load_config_from_file(&config_path).into_diagnostic()?;
            println!("site: {}", cfg.site.title);
            for id in cfg.collection_ids() {
                println!("collection: {id}");
            }
            Ok(())
        }
    }
}
