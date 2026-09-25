use serde::Serialize;

use crate::error::{EngineError, Stage};

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    Pdf,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    BankStatement,
    GroceryBill,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    PdfTextLayer,
    Ocr,
}

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub input_file: String,
    pub input_type: Option<InputType>,
    pub document_type: Option<DocumentType>,
    pub extraction_method: Option<ExtractionMethod>,
    pub processing_time_ms: u128,
    pub engine_version: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatementPeriod {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub bank_name: Option<String>,
    pub account_holder_name: Option<String>,
    pub account_type: AccountType,
    pub currency: Option<String>,
    pub statement_period: Option<StatementPeriod>,
    pub opening_balance: Option<f64>,
    pub closing_balance: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategorySuggestion {
    pub name: String,
    pub match_confidence: f64,
    pub matched_pattern: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transaction {
    pub date: Option<String>,
    pub raw_text: String,
    pub description: Option<String>,
    pub merchant: Option<String>,
    pub debit: Option<f64>,
    pub credit: Option<f64>,
    pub balance: Option<f64>,
    /// Always null from the engine — see AGENTS.md "engine never decides a category".
    pub category: Option<String>,
    pub category_suggestion: Option<CategorySuggestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BankStatementData {
    pub account: Account,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroceryMerchant {
    pub name: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub date: Option<String>,
    pub receipt_number: Option<String>,
    pub payment_method: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroceryItem {
    pub name: String,
    pub raw_text: String,
    pub quantity: Option<f64>,
    pub unit_price: Option<f64>,
    pub total_price: Option<f64>,
    pub category: Option<String>,
    pub category_suggestion: Option<CategorySuggestion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Totals {
    pub subtotal: Option<f64>,
    pub tax: Option<f64>,
    pub discount: Option<f64>,
    pub total: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroceryBillData {
    pub merchant: GroceryMerchant,
    pub receipt: Receipt,
    pub items: Vec<GroceryItem>,
    pub totals: Totals,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DocumentData {
    BankStatement(BankStatementData),
    GroceryBill(GroceryBillData),
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub stage: Stage,
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Envelope {
    Success { meta: Meta, data: DocumentData },
    Error { meta: Meta, error: ErrorDetail },
}

impl Envelope {
    pub fn error(meta: Meta, err: &EngineError) -> Self {
        Envelope::Error {
            meta,
            error: ErrorDetail {
                stage: err.stage(),
                code: err.code().to_string(),
                message: err.message(),
                details: None,
            },
        }
    }
}
