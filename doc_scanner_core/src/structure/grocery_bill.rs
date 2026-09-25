use crate::error::EngineError;
use crate::schema::GroceryBillData;
use crate::taxonomy::Taxonomy;

/// No retailer-format-specific parser is registered yet — see design doc §4.
pub fn structure(_text: &str, _taxonomy: &Taxonomy) -> Result<GroceryBillData, EngineError> {
    Err(EngineError::Structuring {
        code: "UNRECOGNIZED_FORMAT",
        message: "no grocery bill parser is registered for this document's layout".to_string(),
    })
}
