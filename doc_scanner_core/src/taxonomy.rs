use std::path::Path;

use serde::Deserialize;

use crate::schema::CategorySuggestion;

#[derive(Debug, Deserialize)]
struct TaxonomyRule {
    category: String,
    patterns: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TaxonomyConfig {
    rules: Vec<TaxonomyRule>,
}

pub struct Taxonomy {
    rules: Vec<TaxonomyRule>,
}

const EXACT_MATCH_CONFIDENCE: f64 = 0.9;
const SUBSTRING_MATCH_CONFIDENCE: f64 = 0.6;

impl Taxonomy {
    /// Loads rules from an external JSON config file, per AGENTS.md:
    /// "Never hardcode taxonomy data in Rust source."
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let config: TaxonomyConfig = serde_json::from_str(&raw)?;
        Ok(Taxonomy {
            rules: config.rules,
        })
    }

    pub fn empty() -> Self {
        Taxonomy { rules: Vec::new() }
    }

    /// Matches normalized merchant/item text against the loaded taxonomy.
    /// Advisory only — never sets `category`, only `category_suggestion`.
    pub fn suggest(&self, text: &str) -> Option<CategorySuggestion> {
        let lower = text.to_lowercase();
        for rule in &self.rules {
            for pattern in &rule.patterns {
                let pattern_lower = pattern.to_lowercase();
                if lower == pattern_lower {
                    return Some(CategorySuggestion {
                        name: rule.category.clone(),
                        match_confidence: EXACT_MATCH_CONFIDENCE,
                        matched_pattern: pattern.clone(),
                    });
                }
            }
        }
        for rule in &self.rules {
            for pattern in &rule.patterns {
                let pattern_lower = pattern.to_lowercase();
                if lower.contains(&pattern_lower) {
                    return Some(CategorySuggestion {
                        name: rule.category.clone(),
                        match_confidence: SUBSTRING_MATCH_CONFIDENCE,
                        matched_pattern: pattern.clone(),
                    });
                }
            }
        }
        None
    }
}

/// Strips trailing digits/reference codes and normalizes whitespace/case, per
/// design doc §5.5. A simple heuristic, not a claim of precision.
pub fn normalize_merchant(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let cleaned: Vec<&str> = trimmed
        .split_whitespace()
        .filter(|tok| !tok.chars().all(|c| c.is_ascii_digit()))
        .collect();

    if cleaned.is_empty() {
        return None;
    }

    let joined = cleaned.join(" ");
    Some(title_case(&joined))
}

fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
