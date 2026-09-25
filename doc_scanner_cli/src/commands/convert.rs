use std::path::PathBuf;

use clap::Args;

use super::{load_taxonomy, write_json};

#[derive(Args)]
pub struct ConvertArgs {
    /// Path to the PDF or image file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Write JSON to this file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Pretty-print JSON (default is compact, better for piping)
    #[arg(long)]
    pub pretty: bool,

    /// Path to the taxonomy config used for category_suggestion
    #[arg(long)]
    pub taxonomy: Option<PathBuf>,
}

pub fn run(args: ConvertArgs) -> i32 {
    let taxonomy = load_taxonomy(&args.taxonomy);
    let (envelope, exit_code) = doc_scanner_core::convert(&args.input, &taxonomy);

    let value = serde_json::to_value(&envelope).expect("Envelope serialization cannot fail");
    if let Err(e) = write_json(&value, &args.output, args.pretty) {
        eprintln!("error: failed to write output: {e}");
        return 6;
    }

    exit_code
}
