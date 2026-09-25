use std::collections::HashMap;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::Serialize;

use doc_scanner_core::schema::{CategorySuggestion, DocumentData, Envelope};

use super::{load_taxonomy, write_json};

#[derive(Clone, Copy, ValueEnum)]
pub enum ReportFormat {
    Json,
    Csv,
}

#[derive(Args)]
pub struct SeedReportArgs {
    /// Directory of sample PDFs/images to mine for merchant strings
    #[arg(long)]
    pub input_dir: PathBuf,

    /// Output format for the merchant/category report
    #[arg(long, value_enum, default_value_t = ReportFormat::Json)]
    pub format: ReportFormat,

    /// Path to the starter taxonomy config used for suggested_category
    #[arg(long)]
    pub taxonomy: Option<PathBuf>,

    /// Instead of a report, read back a reviewed/edited report from this file
    /// and emit merchant_category_rules INSERT statements
    #[arg(long)]
    pub emit_sql: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct MerchantEntry {
    merchant: String,
    occurrences: u32,
    example_raw_text: Vec<String>,
    suggested_category: Option<CategorySuggestion>,
}

pub fn run(args: SeedReportArgs) -> i32 {
    if let Some(reviewed_path) = &args.emit_sql {
        return emit_sql(reviewed_path);
    }

    let entries = match std::fs::read_dir(&args.input_dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!(
                "error: cannot read input directory {}: {e}",
                args.input_dir.display()
            );
            return 2;
        }
    };

    let taxonomy = load_taxonomy(&args.taxonomy);
    let mut merchants: HashMap<String, MerchantEntry> = HashMap::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let (envelope, _) = doc_scanner_core::convert(&path, &taxonomy);
        let Envelope::Success { data, .. } = envelope else {
            tracing::warn!(path = %path.display(), "skipping file that failed to convert");
            continue;
        };

        collect_merchants(&data, &mut merchants);
    }

    let mut ranked: Vec<MerchantEntry> = merchants.into_values().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.occurrences));

    match args.format {
        ReportFormat::Json => {
            let value = serde_json::to_value(&ranked).expect("serialization cannot fail");
            if let Err(e) = write_json(&value, &None, true) {
                eprintln!("error: failed to write report: {e}");
                return 6;
            }
        }
        ReportFormat::Csv => print_csv(&ranked),
    }

    0
}

fn collect_merchants(data: &DocumentData, merchants: &mut HashMap<String, MerchantEntry>) {
    match data {
        DocumentData::BankStatement(bank) => {
            for txn in &bank.transactions {
                let Some(merchant) = &txn.merchant else {
                    continue;
                };
                record(merchants, merchant, &txn.raw_text, &txn.category_suggestion);
            }
        }
        DocumentData::GroceryBill(grocery) => {
            let Some(merchant) = &grocery.merchant.name else {
                return;
            };
            for item in &grocery.items {
                record(
                    merchants,
                    merchant,
                    &item.raw_text,
                    &item.category_suggestion,
                );
            }
        }
    }
}

fn record(
    merchants: &mut HashMap<String, MerchantEntry>,
    merchant: &str,
    raw_text: &str,
    suggestion: &Option<CategorySuggestion>,
) {
    let entry = merchants
        .entry(merchant.to_string())
        .or_insert_with(|| MerchantEntry {
            merchant: merchant.to_string(),
            occurrences: 0,
            example_raw_text: Vec::new(),
            suggested_category: suggestion.clone(),
        });
    entry.occurrences += 1;
    if entry.example_raw_text.len() < 3 {
        entry.example_raw_text.push(raw_text.to_string());
    }
}

fn print_csv(ranked: &[MerchantEntry]) {
    println!("merchant,occurrences,suggested_category,example_raw_text");
    for entry in ranked {
        let category = entry
            .suggested_category
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let example = entry.example_raw_text.first().cloned().unwrap_or_default();
        println!(
            "{},{},{},{}",
            csv_escape(&entry.merchant),
            entry.occurrences,
            csv_escape(&category),
            csv_escape(&example)
        );
    }
}

fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Reads back a reviewed/edited report and emits `merchant_category_rules`
/// INSERT statements, per design doc §10.2.1. Requires a `category_id` per
/// entry, which the ingestion layer (not this engine) owns — so this reads a
/// reviewed report shape that has already been annotated with one.
fn emit_sql(reviewed_path: &PathBuf) -> i32 {
    #[derive(serde::Deserialize)]
    struct ReviewedEntry {
        merchant: String,
        category_id: String,
    }

    let raw = match std::fs::read_to_string(reviewed_path) {
        Ok(raw) => raw,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", reviewed_path.display());
            return 2;
        }
    };

    let reviewed: Vec<ReviewedEntry> = match serde_json::from_str(&raw) {
        Ok(reviewed) => reviewed,
        Err(e) => {
            eprintln!("error: invalid reviewed report JSON: {e}");
            return 2;
        }
    };

    for entry in reviewed {
        println!(
            "INSERT INTO merchant_category_rules (tenant_id, match_type, pattern, category_id, priority, source) \
             VALUES (NULL, 'exact', '{}', '{}', 100, 'system_seed');",
            entry.merchant.replace('\'', "''"),
            entry.category_id.replace('\'', "''")
        );
    }

    0
}
