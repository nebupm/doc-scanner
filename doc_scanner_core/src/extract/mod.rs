pub mod image;
pub mod pdf;

use crate::error::EngineError;
use crate::input::ValidatedInput;
use crate::schema::{ExtractionMethod, InputType};

pub struct ExtractedText {
    pub text: String,
    pub method: ExtractionMethod,
}

pub fn extract(input: &ValidatedInput) -> Result<ExtractedText, EngineError> {
    match input.input_type {
        InputType::Pdf => pdf::extract_text_layer(&input.bytes),
        InputType::Image => image::extract_via_ocr(&input.bytes),
    }
}
