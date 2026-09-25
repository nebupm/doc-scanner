use crate::error::EngineError;
use crate::schema::BankStatementData;
use crate::taxonomy::Taxonomy;

/// No bank-format-specific parser is registered yet — see design doc §4:
/// format coverage grows only as fast as parsers are written per issuer layout.
/// A clean UNRECOGNIZED_FORMAT error is the correct behavior here, not a
/// best-effort guess at the structure.
pub fn structure(_text: &str, _taxonomy: &Taxonomy) -> Result<BankStatementData, EngineError> {
    Err(EngineError::Structuring {
        code: "UNRECOGNIZED_FORMAT",
        message: "no bank statement parser is registered for this document's layout".to_string(),
    })
}
