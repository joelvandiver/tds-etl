use anyhow::Result;
use clap::{Parser, Subcommand};
use tds_etl::pipeline::Pipeline;
use std::path::PathBuf;
use std::process::ExitCode;

/// Metadata-driven ETL worker: the left model, right model, and the map
/// between them are all data files; this binary interprets them.
#[derive(Parser)]
#[command(name = "etl", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a pipeline spec end to end.
    Run {
        /// Path to a pipeline spec (YAML or JSON).
        spec: PathBuf,
    },
    /// Statically check a pipeline spec's mapping against its models
    /// without touching any data.
    Validate {
        /// Path to a pipeline spec (YAML or JSON).
        spec: PathBuf,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate { spec } => {
            let pipeline = Pipeline::load(&spec)?;
            let errors = pipeline.validate();
            if errors.is_empty() {
                println!(
                    "ok: mapping '{}' -> '{}' is consistent with both models",
                    pipeline.left.name, pipeline.right.name
                );
                Ok(ExitCode::SUCCESS)
            } else {
                for e in &errors {
                    eprintln!("invalid: {e}");
                }
                Ok(ExitCode::FAILURE)
            }
        }
        Command::Run { spec } => {
            let pipeline = Pipeline::load(&spec)?;
            let report = pipeline.run()?;
            println!(
                "read {} row(s): wrote {} to {}, rejected {}",
                report.rows_read,
                report.rows_written,
                pipeline.spec.output.path.display(),
                report.rejects.len()
            );
            for reject in &report.rejects {
                eprintln!(
                    "  row {} rejected at {:?}: {}",
                    reject.row,
                    reject.stage,
                    reject.errors.join("; ")
                );
            }
            if let Some(path) = &pipeline.spec.rejects {
                if !report.rejects.is_empty() {
                    eprintln!("  reject detail written to {}", path.display());
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
