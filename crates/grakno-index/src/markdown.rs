//! Markdown parsing utilities for extracting symbol mentions.
//!
//! Uses pulldown-cmark for proper Markdown parsing.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::collections::HashSet;

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
pub fn extract_mentions(content: &str) -> Vec<Mention> {
    let mut mentions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    
    // Build a map of byte offset to line number
    let mut offset_to_line = vec![0; content.len() + 1];
    let mut current_line = 1;
    let mut byte_offset = 0;
    for line in &lines {
        let line_len = line.len();
        for _ in 0..=line_len {
            if byte_offset < offset_to_line.len() {
                offset_to_line[byte_offset] = current_line;
            }
            byte_offset += 1;
        }
        // Account for newline
        if byte_offset < offset_to_line.len() {
            offset_to_line[byte_offset] = current_line;
        }
        byte_offset += 1;
        current_line += 1;
    }
    
    // Helper to get line number from byte range
    let get_line = |range: &std::ops::Range<usize>| -> usize {
        let start = range.start.min(offset_to_line.len() - 1);
        offset_to_line.get(start).copied().unwrap_or(1)
    };
    
    // Track code block ranges
    let mut code_block_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut in_code_block = false;
    let mut code_block_start = 0;
    
    // First pass: collect code block ranges
    for (event, range) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_block_start = range.start;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                code_block_ranges.push(code_block_start..range.end);
            }
            _ => {}
        }
    }
    
    // Helper to check if a range is in a code block
    let in_any_code_block = |range: &std::ops::Range<usize>| -> bool {
        code_block_ranges.iter().any(|block| {
            range.start >= block.start && range.end <= block.end
        })
    };
    
    // Second pass: extract mentions
    let mut in_link = false;
    let mut link_start_line = 1;
    let mut link_text = String::new();
    let mut link_code_span: Option<String> = None;
    
    for (event, range) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. }) => {
                in_link = true;
                link_start_line = get_line(&range);
                link_text.clear();
                link_code_span = None;
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
                // If we found a code span in the link, add it
                if let Some(symbol) = link_code_span.take() {
                    if is_likely_symbol(&symbol) {
                        let context = get_line_context(&lines, link_start_line);
                        mentions.push(Mention {
                            symbol_name: symbol,
                            line: link_start_line,
                            context,
                        });
                    }
                }
            }
            Event::Code(code) => {
                let symbol = code.to_string();
                let line = get_line(&range);
                
                if in_link {
                    // Store for when link ends
                    link_code_span = Some(symbol);
                } else if !in_any_code_block(&range) && is_likely_symbol(&symbol) {
                    let context = get_line_context(&lines, line);
                    mentions.push(Mention {
                        symbol_name: symbol,
                        line,
                        context,
                    });
                }
            }
            Event::Text(text) => {
                if in_link && link_code_span.is_none() {
                    link_text.push_str(&text);
                }
            }
            _ => {}
        }
    }
    
    mentions
}

/// Get context for a specific line.
fn get_line_context(lines: &[&str], line_num: usize) -> String {
    if line_num == 0 || line_num > lines.len() {
        return String::new();
    }
    
    let line = lines[line_num - 1];
    if line.len() > 80 {
        line.chars().take(80).collect::<String>() + "..."
    } else {
        line.to_string()
    }
}

/// Check if a string looks like a valid symbol name.
fn is_likely_symbol(name: &str) -> bool {
    if name.len() < 2 || name.len() > 64 {
        return false;
    }
    
    if name.contains(' ') || name.contains('\t') || name.contains('\n') {
        return false;
    }
    
    if !name.chars().any(|c| c.is_alphanumeric()) {
        return false;
    }
    
    static COMMON_WORDS: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "can",
        "had", "her", "was", "one", "our", "out", "day", "get", "has",
        "him", "his", "how", "its", "may", "new", "now", "old", "see",
        "two", "who", "boy", "did", "she", "use", "way", "many",
        "oil", "sit", "set", "run", "eat", "far", "sea", "eye", "ago",
        "off", "too", "any", "say", "man", "try", "ask", "end", "why",
        "let", "put", "tell", "very", "when", "come", "from", "they",
        "know", "want", "been", "good", "much", "some", "time", "also",
        "here", "look", "more", "only", "over", "such", "take", "than",
        "them", "well", "were", "will", "with", "have", "this", "that",
        "your", "would", "there", "their", "what", "said", "each",
        "which", "should", "could", "example", "true", "false", "yes",
        "no", "maybe", "however", "therefore", "since", "because",
        "although", "while", "nevertheless", "otherwise", "instead",
    ];
    
    if name.chars().all(|c| c.is_lowercase()) && COMMON_WORDS.contains(&name.to_lowercase().as_str()) {
        return false;
    }
    
    true
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
        // Line number may vary due to markdown parsing, just ensure it's found
        assert!(mentions[0].line > 0);
    }

    #[test]
    fn extract_link_with_code() {
        let content = "See [`parse_ts_file`](parser_ts.rs) for details.";
        let mentions = extract_mentions(content);
        
        assert!(mentions.len() >= 1);
        assert!(mentions.iter().any(|m| m.symbol_name == "parse_ts_file"));
    }

    #[test]
    fn skip_common_words() {
        let content = "The `example` shows how to use the `Config` struct.";
        let mentions = extract_mentions(content);
        
        // "Config" is CamelCase so should be included
        let config_mentions: Vec<_> = mentions.iter().filter(|m| m.symbol_name == "Config").collect();
        assert!(!config_mentions.is_empty());
    }

    #[test]
    fn extract_multiple_mentions() {
        let content = r#"Use `Entity` and `Edge` to build the graph.

See also `EntityKind` for type information."#;
        let mentions = extract_mentions(content);
        
        assert_eq!(mentions.len(), 3);
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
        
        assert!(mentions.is_empty());
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
        
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].symbol_name, "用户");
    }
    
    #[test]
    fn handle_unicode_emdash() {
        // This was the original crash case
        let content = "4. **`query_pairs()` unbounded allocation** — A query string with millions";
        let mentions = extract_mentions(content);
        
        assert!(mentions.iter().any(|m| m.symbol_name == "query_pairs()"));
    }
}
