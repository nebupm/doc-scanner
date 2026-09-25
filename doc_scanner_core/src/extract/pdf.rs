use super::ExtractedText;
use crate::error::EngineError;
use crate::schema::ExtractionMethod;

/// Minimum non-whitespace character count below which a PDF's text layer is
/// considered sparse/absent (i.e. a scanned PDF needing OCR instead).
/// OCR fallback for scanned PDFs is phase 2 — see design doc §12.
const MIN_TEXT_LAYER_CHARS: usize = 20;

pub fn extract_text_layer(bytes: &[u8]) -> Result<ExtractedText, EngineError> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|e| EngineError::Extraction {
        code: "PDF_PARSE_FAILED",
        message: format!("failed to parse PDF: {e}"),
    })?;

    if text.trim().chars().count() < MIN_TEXT_LAYER_CHARS {
        return Err(EngineError::Extraction {
            code: "PDF_TEXT_LAYER_SPARSE",
            message: "PDF has no usable text layer; this looks like a scanned PDF, \
                      which requires OCR (not yet supported)"
                .to_string(),
        });
    }

    Ok(ExtractedText {
        text,
        method: ExtractionMethod::PdfTextLayer,
    })
}
