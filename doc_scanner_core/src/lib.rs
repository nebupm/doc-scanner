pub mod classify;
pub mod error;
pub mod extract;
pub mod input;
pub mod observability;
pub mod schema;
pub mod structure;
pub mod taxonomy;

use std::path::Path;
use std::time::Instant;

use error::EngineError;
use observability::{log_conversion, ConversionLogAttributes};
use schema::{Envelope, Meta};
use taxonomy::Taxonomy;

/// Bytes/hash of the input, tracked separately from `Meta` since they're log
/// attributes only (design doc §14.2) — not part of the public JSON envelope.
#[derive(Default)]
struct Telemetry {
    bytes: Option<u64>,
    content_hash: Option<String>,
}

/// Runs the full pipeline (design doc §3): input validation, extraction,
/// classification, structuring. Stage 5 (sanity-check validation) lands
/// alongside the first real structure/*.rs parser. Returns the envelope
/// alongside the coarse-grained process exit code for it (design doc §6).
/// Also emits one structured log record describing the outcome (§14).
pub fn convert(input_path: &Path, taxonomy: &Taxonomy) -> (Envelope, i32) {
    let started = Instant::now();
    let mut meta = Meta {
        input_file: input_path.display().to_string(),
        input_type: None,
        document_type: None,
        extraction_method: None,
        processing_time_ms: 0,
        engine_version: schema::ENGINE_VERSION,
    };
    let mut telemetry = Telemetry::default();

    let file_name = input_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| input_path.display().to_string());

    let (envelope, exit_code) = match run_pipeline(input_path, taxonomy, &mut meta, &mut telemetry)
    {
        Ok(data) => {
            meta.processing_time_ms = started.elapsed().as_millis();
            log_conversion(&ConversionLogAttributes {
                file_name: &file_name,
                success: true,
                bytes: telemetry.bytes,
                processing_time_ms: meta.processing_time_ms,
                document_type: meta.document_type,
                extraction_method: meta.extraction_method,
                content_hash: telemetry.content_hash.as_deref(),
                error_stage: None,
                error_code: None,
            });
            (Envelope::Success { meta, data }, 0)
        }
        Err(err) => {
            meta.processing_time_ms = started.elapsed().as_millis();
            let exit_code = err.exit_code();
            let stage = format!("{:?}", err.stage()).to_lowercase();
            log_conversion(&ConversionLogAttributes {
                file_name: &file_name,
                success: false,
                bytes: telemetry.bytes,
                processing_time_ms: meta.processing_time_ms,
                document_type: meta.document_type,
                extraction_method: meta.extraction_method,
                content_hash: telemetry.content_hash.as_deref(),
                error_stage: Some(&stage),
                error_code: Some(err.code()),
            });
            (Envelope::error(meta, &err), exit_code)
        }
    };

    (envelope, exit_code)
}

fn run_pipeline(
    input_path: &Path,
    taxonomy: &Taxonomy,
    meta: &mut Meta,
    telemetry: &mut Telemetry,
) -> Result<schema::DocumentData, EngineError> {
    let validated = match input::detect_and_validate(input_path) {
        Ok(validated) => validated,
        Err(err) => {
            // Best-effort size even when validation fails before/without a full read.
            telemetry.bytes = input::file_size(input_path);
            return Err(err);
        }
    };
    telemetry.bytes = Some(validated.bytes.len() as u64);
    telemetry.content_hash = Some(validated.content_hash.clone());
    meta.input_type = Some(validated.input_type);

    let extracted = extract::extract(&validated)?;
    meta.extraction_method = Some(extracted.method);

    let document_type = classify::classify(&extracted.text)?;
    meta.document_type = Some(document_type);

    structure::structure(&extracted.text, document_type, taxonomy)
}
