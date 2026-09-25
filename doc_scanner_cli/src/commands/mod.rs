pub mod convert;
pub mod seed_report;

use std::path::PathBuf;

use doc_scanner_core::taxonomy::Taxonomy;

/// Loads the taxonomy from `path` if given, falling back to an empty
/// taxonomy (no suggestions) otherwise, per design doc §6 `--taxonomy` semantics.
pub fn load_taxonomy(path: &Option<PathBuf>) -> Taxonomy {
    match path {
        Some(p) => Taxonomy::load(p).unwrap_or_else(|e| {
            tracing::warn!(error = %e, path = %p.display(), "failed to load taxonomy, using empty taxonomy");
            Taxonomy::empty()
        }),
        None => Taxonomy::empty(),
    }
}

pub fn write_json(
    value: &serde_json::Value,
    output: &Option<PathBuf>,
    pretty: bool,
) -> std::io::Result<()> {
    let rendered = if pretty {
        serde_json::to_string_pretty(value).expect("Value serialization cannot fail")
    } else {
        serde_json::to_string(value).expect("Value serialization cannot fail")
    };

    match output {
        Some(path) => std::fs::write(path, rendered),
        None => {
            println!("{rendered}");
            Ok(())
        }
    }
}
