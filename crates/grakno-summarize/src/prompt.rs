use crate::error::{Result, SummarizeError};
use serde::{Deserialize, Serialize};

/// Parsed LLM summary response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSummary {
    pub short: String,
    pub detailed: String,
    pub keywords: Vec<String>,
}

/// Metadata for a symbol defined in a source unit.
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub line_start: Option<i64>,
}

pub const SYSTEM_PROMPT: &str = "\
You are a code documentation assistant. \
Return ONLY bare JSON with no markdown fences or extra text. \
The JSON must have exactly these keys: \
{\"short\": \"<one-sentence summary>\", \"detailed\": \"<2-3 sentence description>\", \"keywords\": [\"<keyword>\", ...]}";

/// Build a user prompt for summarizing a source unit (file).
pub fn source_unit_prompt(
    file_path: &str,
    component_name: &str,
    language: Option<&str>,
    symbols: &[SymbolInfo],
) -> String {
    let mut prompt =
        format!("Summarize this source file.\nPath: {file_path}\nComponent: {component_name}\n");
    if let Some(lang) = language {
        prompt.push_str(&format!("Language: {lang}\n"));
    }
    if !symbols.is_empty() {
        prompt.push_str("Defined symbols:\n");
        for sym in symbols {
            match sym.line_start {
                Some(line) => {
                    prompt.push_str(&format!("  - {} ({}) L{}\n", sym.name, sym.kind, line))
                }
                None => prompt.push_str(&format!("  - {} ({})\n", sym.name, sym.kind)),
            }
        }
    }
    prompt
}

/// Build a user prompt for summarizing a component (roll-up of source unit summaries).
pub fn component_prompt(
    component_name: &str,
    component_path: Option<&str>,
    dependency_names: &[String],
    source_unit_summaries: &[(String, String)], // (file_path, short_summary)
) -> String {
    let mut prompt = format!("Summarize this software component.\nName: {component_name}\n");
    if let Some(path) = component_path {
        prompt.push_str(&format!("Path: {path}\n"));
    }
    if !dependency_names.is_empty() {
        prompt.push_str(&format!("Dependencies: {}\n", dependency_names.join(", ")));
    }
    if !source_unit_summaries.is_empty() {
        prompt.push_str("Source files:\n");
        for (path, summary) in source_unit_summaries {
            prompt.push_str(&format!("  - {path}: {summary}\n"));
        }
    }
    prompt
}

/// Parse the LLM response JSON into an `LlmSummary`.
pub fn parse_llm_response(raw: &str) -> Result<LlmSummary> {
    // Strip markdown code fences if present
    let trimmed = raw.trim();
    let json_str = if trimmed.starts_with("```") {
        let inner = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```");
        inner.trim_end_matches("```").trim()
    } else {
        trimmed
    };

    serde_json::from_str::<LlmSummary>(json_str)
        .map_err(|e| SummarizeError::ParseResponse(format!("{e}: {json_str}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"short": "A helper module", "detailed": "Provides utility functions for string processing.", "keywords": ["util", "strings"]}"#;
        let result = parse_llm_response(raw).unwrap();
        assert_eq!(result.short, "A helper module");
        assert_eq!(result.keywords, vec!["util", "strings"]);
    }

    #[test]
    fn parse_json_with_code_fences() {
        let raw = "```json\n{\"short\": \"X\", \"detailed\": \"Y\", \"keywords\": []}\n```";
        let result = parse_llm_response(raw).unwrap();
        assert_eq!(result.short, "X");
    }

    #[test]
    fn parse_invalid_json() {
        let raw = "not json at all";
        assert!(parse_llm_response(raw).is_err());
    }

    #[test]
    fn source_unit_prompt_format() {
        let symbols = vec![
            SymbolInfo {
                name: "Foo".to_string(),
                kind: "struct".to_string(),
                line_start: Some(10),
            },
            SymbolInfo {
                name: "bar".to_string(),
                kind: "fn".to_string(),
                line_start: None,
            },
        ];
        let prompt = source_unit_prompt("src/lib.rs", "my-crate", Some("rust"), &symbols);
        assert!(prompt.contains("Path: src/lib.rs"));
        assert!(prompt.contains("Component: my-crate"));
        assert!(prompt.contains("Language: rust"));
        assert!(prompt.contains("Foo (struct) L10"));
        assert!(prompt.contains("bar (fn)"));
    }

    #[test]
    fn component_prompt_format() {
        let deps = vec!["serde".to_string(), "tokio".to_string()];
        let summaries = vec![
            ("src/lib.rs".to_string(), "Main library entry".to_string()),
            ("src/util.rs".to_string(), "Utility helpers".to_string()),
        ];
        let prompt = component_prompt("my-crate", Some("crates/my-crate"), &deps, &summaries);
        assert!(prompt.contains("Name: my-crate"));
        assert!(prompt.contains("Path: crates/my-crate"));
        assert!(prompt.contains("Dependencies: serde, tokio"));
        assert!(prompt.contains("src/lib.rs: Main library entry"));
    }

    #[test]
    fn system_prompt_mentions_json_keys() {
        assert!(SYSTEM_PROMPT.contains("short"));
        assert!(SYSTEM_PROMPT.contains("detailed"));
        assert!(SYSTEM_PROMPT.contains("keywords"));
    }
}
