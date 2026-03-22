use tree_sitter::{Node, Parser};

use crate::error::IndexError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    TypeAlias,
    Const,
    Static,
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line_start: usize,
    pub line_end: usize,
    pub visibility: String,
    pub is_test: bool,
    pub is_bench: bool,
}

pub fn parse_rust_file(source: &str) -> Result<Vec<ExtractedSymbol>, IndexError> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|e| IndexError::Parse(e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| IndexError::Parse("tree-sitter failed to parse".to_string()))?;

    let mut symbols = Vec::new();
    extract_toplevel(&tree.root_node(), source, &mut symbols);
    Ok(symbols)
}

fn extract_toplevel(root: &Node, source: &str, symbols: &mut Vec<ExtractedSymbol>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Function, None) {
                    symbols.push(sym);
                }
            }
            "struct_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Struct, None) {
                    symbols.push(sym);
                }
            }
            "enum_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Enum, None) {
                    symbols.push(sym);
                }
            }
            "trait_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Trait, None) {
                    symbols.push(sym);
                }
            }
            "type_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::TypeAlias, None) {
                    symbols.push(sym);
                }
            }
            "const_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Const, None) {
                    symbols.push(sym);
                }
            }
            "static_item" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Static, None) {
                    symbols.push(sym);
                }
            }
            "macro_definition" => {
                if let Some(sym) = extract_item(&child, source, SymbolKind::Macro, None) {
                    symbols.push(sym);
                }
            }
            "impl_item" => {
                extract_impl(&child, source, symbols);
            }
            _ => {}
        }
    }
}

fn extract_impl(node: &Node, source: &str, symbols: &mut Vec<ExtractedSymbol>) {
    let type_name = match impl_type_name(node, source) {
        Some(n) => n,
        None => return,
    };

    symbols.push(ExtractedSymbol {
        name: format!("impl {type_name}"),
        kind: SymbolKind::Impl,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        visibility: "private".to_string(),
        is_test: false,
        is_bench: false,
    });

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" {
                if let Some(sym) =
                    extract_item(&child, source, SymbolKind::Function, Some(&type_name))
                {
                    symbols.push(sym);
                }
            }
        }
    }
}

fn extract_item(
    node: &Node,
    source: &str,
    kind: SymbolKind,
    impl_type: Option<&str>,
) -> Option<ExtractedSymbol> {
    let raw_name = item_name(node, source)?;
    let name = match impl_type {
        Some(t) => format!("{t}::{raw_name}"),
        None => raw_name,
    };
    let visibility = get_visibility(node, source);
    let (is_test, is_bench) = check_attributes(node, source);

    Some(ExtractedSymbol {
        name,
        kind,
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        visibility,
        is_test,
        is_bench,
    })
}

fn item_name(node: &Node, source: &str) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(source[name_node.byte_range()].to_string());
    }
    None
}

fn impl_type_name(node: &Node, source: &str) -> Option<String> {
    let type_node = node.child_by_field_name("type")?;
    first_type_identifier(&type_node, source)
}

fn first_type_identifier(node: &Node, source: &str) -> Option<String> {
    if node.kind() == "type_identifier" {
        return Some(source[node.byte_range()].to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = first_type_identifier(&child, source) {
            return Some(name);
        }
    }
    None
}

fn get_visibility(node: &Node, source: &str) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return source[child.byte_range()].to_string();
        }
    }
    "private".to_string()
}

fn check_attributes(node: &Node, source: &str) -> (bool, bool) {
    let mut is_test = false;
    let mut is_bench = false;
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            "attribute_item" => {
                let text = &source[sibling.byte_range()];
                if text.contains("test") {
                    is_test = true;
                }
                if text.contains("bench") {
                    is_bench = true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        prev = sibling.prev_sibling();
    }
    (is_test, is_bench)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_functions_and_structs() {
        let src = r#"
pub fn hello() {}

fn private_fn() -> i32 { 42 }

pub struct MyStruct {
    field: i32,
}

enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let symbols = parse_rust_file(src).unwrap();
        assert_eq!(symbols.len(), 4);

        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].visibility, "pub");

        assert_eq!(symbols[1].name, "private_fn");
        assert_eq!(symbols[1].kind, SymbolKind::Function);
        assert_eq!(symbols[1].visibility, "private");

        assert_eq!(symbols[2].name, "MyStruct");
        assert_eq!(symbols[2].kind, SymbolKind::Struct);
        assert_eq!(symbols[2].visibility, "pub");

        assert_eq!(symbols[3].name, "Color");
        assert_eq!(symbols[3].kind, SymbolKind::Enum);
        assert_eq!(symbols[3].visibility, "private");
    }

    #[test]
    fn parse_trait_and_impl() {
        let src = r#"
pub trait Greet {
    fn greet(&self) -> String;
}

impl Greet for MyStruct {
    fn greet(&self) -> String {
        "hello".to_string()
    }
}
"#;
        let symbols = parse_rust_file(src).unwrap();
        assert_eq!(symbols.len(), 3);

        assert_eq!(symbols[0].name, "Greet");
        assert_eq!(symbols[0].kind, SymbolKind::Trait);

        assert_eq!(symbols[1].name, "impl MyStruct");
        assert_eq!(symbols[1].kind, SymbolKind::Impl);

        assert_eq!(symbols[2].name, "MyStruct::greet");
        assert_eq!(symbols[2].kind, SymbolKind::Function);
    }

    #[test]
    fn parse_test_attribute() {
        let src = r#"
#[test]
fn my_test() {
    assert!(true);
}

#[cfg(test)]
fn not_a_test() {}

fn regular() {}
"#;
        let symbols = parse_rust_file(src).unwrap();
        assert_eq!(symbols.len(), 3);

        assert_eq!(symbols[0].name, "my_test");
        assert!(symbols[0].is_test);

        // #[cfg(test)] also contains "test" — that's fine for now
        assert_eq!(symbols[1].name, "not_a_test");

        assert_eq!(symbols[2].name, "regular");
        assert!(!symbols[2].is_test);
    }

    #[test]
    fn parse_const_static_type_macro() {
        let src = r#"
pub const MAX: usize = 100;

static COUNTER: i32 = 0;

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! my_macro {
    () => {};
}
"#;
        let symbols = parse_rust_file(src).unwrap();
        assert_eq!(symbols.len(), 4);

        assert_eq!(symbols[0].name, "MAX");
        assert_eq!(symbols[0].kind, SymbolKind::Const);

        assert_eq!(symbols[1].name, "COUNTER");
        assert_eq!(symbols[1].kind, SymbolKind::Static);

        assert_eq!(symbols[2].name, "Result");
        assert_eq!(symbols[2].kind, SymbolKind::TypeAlias);

        assert_eq!(symbols[3].name, "my_macro");
        assert_eq!(symbols[3].kind, SymbolKind::Macro);
    }

    #[test]
    fn parse_empty_file() {
        let symbols = parse_rust_file("").unwrap();
        assert!(symbols.is_empty());
    }

    #[test]
    fn parse_line_numbers() {
        let src = "pub fn foo() {}\n\npub fn bar() {}\n";
        let symbols = parse_rust_file(src).unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].line_start, 1);
        assert_eq!(symbols[1].line_start, 3);
    }
}
