use super::ExtractedText;
use crate::error::EngineError;

/// Local Tesseract OCR path — phase 2 per design doc §12, not yet implemented.
pub fn extract_via_ocr(_bytes: &[u8]) -> Result<ExtractedText, EngineError> {
    Err(EngineError::Extraction {
        code: "OCR_NOT_IMPLEMENTED",
        message: "image input requires OCR, which is not yet implemented".to_string(),
    })
}
