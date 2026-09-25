use crate::schema::{DocumentType, ExtractionMethod};

/// Attributes for the one structured log record emitted per `convert` call
/// (design doc §14). Emitted via `tracing` as semantic fields — the actual
/// on-disk JSON key names (`timestamp`, `level`, `fields`, `target`, ...)
/// come from whichever `tracing-subscriber` formatter the binary configures,
/// not the literal `Timestamp`/`SeverityText`/... names from the OTEL spec.
pub struct ConversionLogAttributes<'a> {
    pub file_name: &'a str,
    pub success: bool,
    pub bytes: Option<u64>,
    pub processing_time_ms: u128,
    pub document_type: Option<DocumentType>,
    pub extraction_method: Option<ExtractionMethod>,
    pub content_hash: Option<&'a str>,
    pub error_stage: Option<&'a str>,
    pub error_code: Option<&'a str>,
}

/// Emits the one conversion log record for this call, at INFO on success or
/// ERROR on failure. Never makes a network call — stays within stderr/whatever
/// `tracing-subscriber` writer the caller configured, per the local-only
/// constraint in AGENTS.md.
pub fn log_conversion(attrs: &ConversionLogAttributes) {
    let status = if attrs.success { "success" } else { "failed" };
    let document_type = attrs.document_type.map(document_type_str);
    let extraction_method = attrs.extraction_method.map(extraction_method_str);

    if attrs.success {
        tracing::info!(
            file_name = attrs.file_name,
            status,
            bytes = attrs.bytes,
            processing_time_ms = attrs.processing_time_ms as u64,
            document_type,
            extraction_method,
            content_hash = attrs.content_hash,
            "conversion completed"
        );
    } else {
        tracing::error!(
            file_name = attrs.file_name,
            status,
            bytes = attrs.bytes,
            processing_time_ms = attrs.processing_time_ms as u64,
            document_type,
            extraction_method,
            content_hash = attrs.content_hash,
            "error.stage" = attrs.error_stage,
            "error.code" = attrs.error_code,
            "conversion failed"
        );
    }
}

fn document_type_str(document_type: DocumentType) -> &'static str {
    match document_type {
        DocumentType::BankStatement => "bank_statement",
        DocumentType::GroceryBill => "grocery_bill",
    }
}

fn extraction_method_str(method: ExtractionMethod) -> &'static str {
    match method {
        ExtractionMethod::PdfTextLayer => "pdf_text_layer",
        ExtractionMethod::Ocr => "ocr",
    }
}
