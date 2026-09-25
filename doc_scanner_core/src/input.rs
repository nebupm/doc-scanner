use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::EngineError;
use crate::schema::InputType;

/// 25 MB, per design doc §8 resource limits.
pub const MAX_INPUT_BYTES: u64 = 25 * 1024 * 1024;

pub struct ValidatedInput {
    pub bytes: Vec<u8>,
    pub input_type: InputType,
    /// SHA-256 hex digest of `bytes`, for the log record's `content_hash`
    /// attribute (design doc §14.2) — correlation/dedup only, not integrity.
    pub content_hash: String,
}

/// Best-effort file size for the log record's `bytes` attribute, used even
/// when validation fails before (or without) reading the full file contents.
pub fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Sniffs magic bytes rather than trusting the file extension, per design doc §3 stage 1.
pub fn detect_and_validate(path: &Path) -> Result<ValidatedInput, EngineError> {
    let metadata = std::fs::metadata(path).map_err(|e| EngineError::InvalidInput {
        code: "INPUT_NOT_FOUND",
        message: format!("cannot read {}: {e}", path.display()),
    })?;

    if metadata.len() == 0 {
        return Err(EngineError::InvalidInput {
            code: "INPUT_EMPTY",
            message: format!("{} is empty", path.display()),
        });
    }

    if metadata.len() > MAX_INPUT_BYTES {
        return Err(EngineError::InvalidInput {
            code: "INPUT_TOO_LARGE",
            message: format!(
                "{} is {} bytes, exceeds the {} byte limit",
                path.display(),
                metadata.len(),
                MAX_INPUT_BYTES
            ),
        });
    }

    let bytes = std::fs::read(path).map_err(|e| EngineError::InvalidInput {
        code: "INPUT_UNREADABLE",
        message: format!("cannot read {}: {e}", path.display()),
    })?;

    let input_type = sniff_type(&bytes).ok_or_else(|| EngineError::UnsupportedFormat {
        code: "UNRECOGNIZED_INPUT_TYPE",
        message: format!(
            "{} is neither a recognizable PDF nor a supported image format",
            path.display()
        ),
    })?;

    let content_hash = format!("{:x}", Sha256::digest(&bytes));

    Ok(ValidatedInput {
        bytes,
        input_type,
        content_hash,
    })
}

fn sniff_type(bytes: &[u8]) -> Option<InputType> {
    if bytes.starts_with(b"%PDF-") {
        return Some(InputType::Pdf);
    }
    // JPEG, PNG magic bytes. Full OCR/image decoding path is phase 2.
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
    {
        return Some(InputType::Image);
    }
    None
}
