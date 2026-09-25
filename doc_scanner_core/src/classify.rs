use crate::error::EngineError;
use crate::schema::DocumentType;

const BANK_STATEMENT_KEYWORDS: &[&str] = &[
    "statement period",
    "opening balance",
    "closing balance",
    "account holder",
    "sort code",
    "iban",
];

const GROCERY_BILL_KEYWORDS: &[&str] = &[
    "subtotal",
    "total due",
    "receipt",
    "cashier",
    "qty",
    "item price",
];

/// Keyword/heuristic classifier per design doc §3 stage 3. Callers may bypass
/// this with an explicit `--type` flag (not yet wired at the CLI layer).
pub fn classify(text: &str) -> Result<DocumentType, EngineError> {
    let lower = text.to_lowercase();

    let bank_score = BANK_STATEMENT_KEYWORDS
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();
    let grocery_score = GROCERY_BILL_KEYWORDS
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();

    match bank_score.cmp(&grocery_score) {
        std::cmp::Ordering::Greater => Ok(DocumentType::BankStatement),
        std::cmp::Ordering::Less => Ok(DocumentType::GroceryBill),
        std::cmp::Ordering::Equal if bank_score > 0 => Ok(DocumentType::BankStatement),
        _ => Err(EngineError::Classification {
            code: "CLASSIFICATION_INCONCLUSIVE",
            message: "could not determine whether this is a bank statement or grocery bill"
                .to_string(),
        }),
    }
}
