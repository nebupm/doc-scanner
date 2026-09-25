mod commands;
mod logging;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use logging::LogSink;

#[derive(Parser)]
#[command(name = "doc-scanner", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the PDF or image file (used when no subcommand is given, defaulting to `convert`)
    #[arg(short, long, global = false)]
    input: Option<std::path::PathBuf>,

    #[arg(short, long)]
    output: Option<std::path::PathBuf>,

    #[arg(long)]
    pretty: bool,

    #[arg(long)]
    taxonomy: Option<std::path::PathBuf>,

    /// Write structured logs (design doc §14) to this file instead of stderr
    #[arg(long, global = true)]
    log_file: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    Convert(commands::convert::ConvertArgs),
    SeedReport(commands::seed_report::SeedReportArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let log_sink = match &cli.log_file {
        Some(path) => match LogSink::file(path) {
            Ok(sink) => sink,
            Err(e) => {
                eprintln!("error: cannot open log file {}: {e}", path.display());
                return ExitCode::from(2);
            }
        },
        None => LogSink::stderr(),
    };

    tracing_subscriber::fmt()
        .with_writer(log_sink)
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let exit_code = match cli.command {
        Some(Commands::Convert(args)) => commands::convert::run(args),
        Some(Commands::SeedReport(args)) => commands::seed_report::run(args),
        None => {
            // `convert` is the default subcommand, per design doc §6.
            let Some(input) = cli.input else {
                eprintln!("error: --input is required (or use the `convert` subcommand)");
                return ExitCode::from(2);
            };
            commands::convert::run(commands::convert::ConvertArgs {
                input,
                output: cli.output,
                pretty: cli.pretty,
                taxonomy: cli.taxonomy,
            })
        }
    };

    ExitCode::from(exit_code as u8)
}
