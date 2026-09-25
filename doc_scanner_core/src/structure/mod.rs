pub mod bank_statement;
pub mod grocery_bill;

use crate::error::EngineError;
use crate::schema::{DocumentData, DocumentType};
use crate::taxonomy::Taxonomy;

pub fn structure(
    text: &str,
    document_type: DocumentType,
    taxonomy: &Taxonomy,
) -> Result<DocumentData, EngineError> {
    match document_type {
        DocumentType::BankStatement => {
            bank_statement::structure(text, taxonomy).map(DocumentData::BankStatement)
        }
        DocumentType::GroceryBill => {
            grocery_bill::structure(text, taxonomy).map(DocumentData::GroceryBill)
        }
    }
}
