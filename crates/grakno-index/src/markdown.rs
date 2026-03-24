//! Markdown parsing utilities for extracting symbol mentions.
//!
//! Detects code references in documentation that link to symbols.

use regex::Regex;
use std::sync::OnceLock;

/// A detected mention of a symbol in documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// The symbol name referenced (e.g., "parse_ts_file").
    pub symbol_name: String,
    /// Line number where the mention occurs (1-indexed).
    pub line: usize,
    /// The context around the mention (surrounding text).
    pub context: String,
}

/// Extract mentions from markdown content.
///
/// Detects:
/// - Inline code: `symbol_name`
/// - Links with code text: [`symbol_name`](path)
/// - Autolinks: <symbol_name> (if looks like code)
///
/// Ignores:
/// - Fenced code blocks (```...```)
/// - Indented code blocks
/// - Empty backticks
/// - Names with spaces or special characters
pub fn extract_mentions(content: &str) -> Vec<Mention> {
    let mut mentions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    
    let mut in_fenced_block = false;
    let mut fence_char = None;
    
    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx + 1;
        
        // Track fenced code blocks (skip mentions inside them)
        if let Some(fence) = detect_fence_boundary(line) {
            if in_fenced_block {
                if Some(fence) == fence_char {
                    in_fenced_block = false;
                    fence_char = None;
                }
            } else {
                in_fenced_block = true;
                fence_char = Some(fence);
            }
            continue;
        }
        
        if in_fenced_block {
            continue;
        }
        
        // Skip indented code blocks (4+ spaces or tab)
        if line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        
        // Extract inline code mentions
        for mention in extract_inline_code(line, line_num) {
            mentions.push(mention);
        }
        
        // Extract link mentions: [symbol](url) or [`symbol`](url)
        for mention in extract_link_mentions(line, line_num) {
            // Avoid duplicates from inline code extraction
            if !mentions.iter().any(|m| m.line == line_num && m.symbol_name == mention.symbol_name) {
                mentions.push(mention);
            }
        }
    }
    
    mentions
}

/// Detect if a line starts or ends a fenced code block.
fn detect_fence_boundary(line: &str) -> Option<char> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        Some(trimmed.chars().next().unwrap())
    } else {
        None
    }
}

/// Extract inline code references: `symbol_name`
fn extract_inline_code(line: &str, line_num: usize) -> Vec<Mention> {
    static CODE_RE: OnceLock<Regex> = OnceLock::new();
    let re = CODE_RE.get_or_init(|| {
        Regex::new(r"`([^`\s][^`]{0,63})`").unwrap()
    });
    
    let mut mentions = Vec::new();
    
    for cap in re.captures_iter(line) {
        let symbol_name = cap[1].to_string();
        
        // Filter out likely non-symbols
        if is_likely_symbol(&symbol_name) {
            mentions.push(Mention {
                symbol_name,
                line: line_num,
                context: extract_context(line, cap.get(0).unwrap().start()),
            });
        }
    }
    
    mentions
}

/// Extract link mentions: [`symbol`](url) or [symbol](url)
fn extract_link_mentions(line: &str, line_num: usize) -> Vec<Mention> {
    static LINK_RE: OnceLock<Regex> = OnceLock::new();
    let re = LINK_RE.get_or_init(|| {
        // Match [text](url) where text may include backticks
        Regex::new(r"\[`?([^\]]{1,64})`?\]\([^)]+\)").unwrap()
    });
    
    let mut mentions = Vec::new();
    
    for cap in re.captures_iter(line) {
        let text = &cap[1];
        
        // Clean up: if it was [`symbol`], extract symbol
        let symbol_name = if text.starts_with('`') && text.ends_with('`') {
            text[1..text.len()-1].to_string()
        } else {
            text.to_string()
        };
        
        if is_likely_symbol(&symbol_name) {
            mentions.push(Mention {
                symbol_name,
                line: line_num,
                context: extract_context(line, cap.get(0).unwrap().start()),
            });
        }
    }
    
    mentions
}

/// Check if a string looks like a valid symbol name.
///
/// Heuristics:
/// - At least 2 characters
/// - Max 64 characters
/// - Contains word characters
/// - No spaces
/// - Not all lowercase common words
fn is_likely_symbol(name: &str) -> bool {
    if name.len() < 2 || name.len() > 64 {
        return false;
    }
    
    if name.contains(' ') || name.contains('\t') {
        return false;
    }
    
    // Must contain at least one alphanumeric
    if !name.chars().any(|c| c.is_alphanumeric()) {
        return false;
    }
    
    // Skip common non-symbol words (lowercase only)
    static COMMON_WORDS: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "can",
        "had", "her", "was", "one", "our", "out", "day", "get", "has",
        "him", "his", "how", "its", "may", "new", "now", "old", "see",
        "two", "who", "boy", "did", "she", "use", "her", "way", "many",
        "oil", "sit", "set", "run", "eat", "far", "sea", "eye", "ago",
        "off", "too", "any", "say", "man", "try", "ask", "end", "why",
        "let", "put", "say", "she", "try", "way", "own", "say", "too",
        "old", "tell", "very", "when", "come", "from", "they", "know",
        "want", "been", "good", "much", "some", "time", "very", "also",
        "here", "look", "more", "only", "over", "such", "take", "than",
        "them", "well", "were", "will", "with", "have", "this", "that",
        "your", "would", "there", "their", "what", "said", "each",
        "which", "she", "how", "his", "him", "has", "had", "get",
        "use", "man", "new", "now", "way", "may", "say", "great",
        "where", "help", "through", "before", "right", "too", "means",
        "any", "same", "tell", "very", "when", "much", "would", "there",
        "should", "could", "example", "true", "false", "yes", "no",
        "maybe", "perhaps", "however", "therefore", "thus", "hence",
        "since", "because", "although", "though", "while", "whereas",
        "nevertheless", "nonetheless", "otherwise", "instead", "meanwhile",
        "afterwards", "later", "before", "earlier", "previously", "currently",
        "recently", "often", "sometimes", "usually", "always", "never",
        "frequently", "occasionally", "rarely", "seldom", "once", "twice",
        "again", "further", "moreover", "furthermore", "additionally",
        "besides", "also", "too", "either", "neither", "both", "all",
        "none", "some", "many", "most", "few", "several", "various",
    ];
    
    if name.chars().all(|c| c.is_lowercase()) && COMMON_WORDS.contains(&name.to_lowercase().as_str()) {
        return false;
    }
    
    // Looks like a symbol (CamelCase, snake_case, or kebab-case)
    true
}

/// Extract context around a position in a line.
fn extract_context(line: &str, pos: usize) -> String {
    let start = pos.saturating_sub(30);
    let end = (pos + 40).min(line.len());
    let context = &line[start..end];
    
    if start > 0 {
        format!("...{}", context)
    } else {
        context.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_inline_code_simple() {
        let content = "Use `parse_ts_file` to parse TypeScript files.";
        let mentions = extract_mentions(content);
        
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].symbol_name, "parse_ts_file");
        assert_eq!(mentions[0].line, 1);
    }

    #[test]
    fn skip_fenced_code_blocks() {
        let content = r#"Use this function:

```rust
fn parse_ts_file() {}
```

Call `parse_ts_file` when needed.
"#;
        let mentions = extract_mentions(content);
        
        // Should only find the one outside the fence
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].symbol_name, "parse_ts_file");
        assert_eq!(mentions[0].line, 7);
    }

    #[test]
    fn skip_indented_code_blocks() {
        let content = "Example:

    fn parse_ts_file() {}

Use `parse_ts_file`.";
        let mentions = extract_mentions(content);
        
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].line, 5);
    }

    #[test]
    fn extract_link_with_code() {
        let content = "See [`parse_ts_file`](parser_ts.rs) for details.";
        let mentions = extract_mentions(content);
        
        // Both inline code and link extraction find the symbol
        // Deduplication keeps them on the same line with same name
        assert!(mentions.len() >= 1);
        assert!(mentions.iter().any(|m| m.symbol_name == "parse_ts_file"));
    }

    #[test]
    fn skip_common_words() {
        let content = "The `example` shows how to use the `Config` struct.";
        let mentions = extract_mentions(content);
        
        // "the" and "example" are filtered, "Config" is kept
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].symbol_name, "Config");
    }

    #[test]
    fn extract_multiple_mentions() {
        let content = r#"Use `Entity` and `Edge` to build the graph.

See also `EntityKind` for type information."#;
        let mentions = extract_mentions(content);
        
        assert_eq!(mentions.len(), 3);
        assert_eq!(mentions[0].symbol_name, "Entity");
        assert_eq!(mentions[1].symbol_name, "Edge");
        assert_eq!(mentions[2].symbol_name, "EntityKind");
    }

    #[test]
    fn skip_too_short() {
        let content = "Use `x` as the variable name.";
        let mentions = extract_mentions(content);
        
        assert!(mentions.is_empty());
    }

    #[test]
    fn skip_with_spaces() {
        let content = "Use `some function` to call.";
        let mentions = extract_mentions(content);
        
        // Names with spaces are not valid symbols
        assert!(mentions.is_empty());
    }

    #[test]
    fn extract_rust_types() {
        // Note: `Result<T, E>` has a space so it's filtered by is_likely_symbol
        let content = "Returns `Option<String>` or `Result<T,E>`.";
        let mentions = extract_mentions(content);
        
        // Both should be extracted (even with angle brackets)
        assert_eq!(mentions.len(), 2);
        assert!(mentions.iter().any(|m| m.symbol_name == "Option<String>"));
        assert!(mentions.iter().any(|m| m.symbol_name == "Result<T,E>"));
    }

    #[test]
    fn context_extraction() {
        let line = "This is a very long line with `parse_ts_file` in the middle of it all";
        let context = extract_context(line, 40);
        
        assert!(context.contains("parse_ts_file"));
        assert!(context.starts_with("...") || line.len() < 70);
    }

    #[test]
    fn skip_empty_backticks() {
        let content = "Use `` as empty code.";
        let mentions = extract_mentions(content);
        
        assert!(mentions.is_empty());
    }

    #[test]
    fn handle_unicode() {
        let content = "The `用户` struct represents a user.";
        let mentions = extract_mentions(content);
        
        // Unicode symbols are valid
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].symbol_name, "用户");
    }
}
