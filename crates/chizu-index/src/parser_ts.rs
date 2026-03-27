use tree_sitter::{Node, Parser};

use crate::error::IndexError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsSymbolKind {
    Function,
    Class,
    Interface,
    TypeAlias,
    Enum,
    Const,
    Let,
    Var,
    Export,
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsSymbol {
    pub name: String,
    pub kind: TsSymbolKind,
    pub line_start: usize,
    pub line_end: usize,
    pub exported: bool,
    pub is_default_export: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsImport {
    pub path: String,
    pub symbols: Vec<String>,             // named imports
    pub default_import: Option<String>,   // default import name
    pub namespace_import: Option<String>, // * as name
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct TsParseResult {
    pub symbols: Vec<TsSymbol>,
    pub imports: Vec<TsImport>,
    pub exports: Vec<String>, // re-exports from "path"
}

pub fn parse_ts_file(source: &str) -> Result<TsParseResult, IndexError> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    parser
        .set_language(&language)
        .map_err(|e| IndexError::Parse(e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| IndexError::Parse("tree-sitter failed to parse".to_string()))?;

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    extract_toplevel(
        &tree.root_node(),
        source,
        &mut symbols,
        &mut imports,
        &mut exports,
    );
    Ok(TsParseResult {
        symbols,
        imports,
        exports,
    })
}

fn extract_toplevel(
    root: &Node,
    source: &str,
    symbols: &mut Vec<TsSymbol>,
    imports: &mut Vec<TsImport>,
    exports: &mut Vec<String>,
) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "export_statement" => {
                extract_export_statement(&child, source, symbols, imports, exports);
            }
            "import_statement" => {
                if let Some(imp) = extract_import(&child, source) {
                    imports.push(imp);
                }
            }
            "function_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Function, false, false)
                {
                    symbols.push(sym);
                }
            }
            "class_declaration" => {
                if let Some(sym) = extract_symbol(&child, source, TsSymbolKind::Class, false, false)
                {
                    symbols.push(sym);
                }
            }
            "interface_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Interface, false, false)
                {
                    symbols.push(sym);
                }
            }
            "type_alias_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::TypeAlias, false, false)
                {
                    symbols.push(sym);
                }
            }
            "enum_declaration" => {
                if let Some(sym) = extract_symbol(&child, source, TsSymbolKind::Enum, false, false)
                {
                    symbols.push(sym);
                }
            }
            // Note: variable_declaration and lexical_declaration are only extracted
            // when inside an export_statement. Private module-level variables are not tracked.
            _ => {}
        }
    }
}

fn extract_export_statement(
    node: &Node,
    source: &str,
    symbols: &mut Vec<TsSymbol>,
    imports: &mut Vec<TsImport>,
    exports: &mut Vec<String>,
) {
    let mut cursor = node.walk();

    // Check for "export default"
    let is_default = node.children(&mut cursor).any(|c| {
        c.kind() == "default" || (c.kind() == "identifier" && &source[c.byte_range()] == "default")
    });

    // Check for re-exports first: export { ... } from "path" or export * from "path"
    if let Some(source_node) = find_child_by_kind(node, "string") {
        let path = extract_string_content(&source_node, source);

        // Check if it's export * from "path" or export { ... } from "path"
        let has_asterisk = node.children(&mut cursor).any(|c| c.kind() == "*");

        if has_asterisk || find_child_by_kind(node, "export_clause").is_some() {
            exports.push(path.clone());

            // Also add as import for tracking re-exports
            if let Some(clause) = find_child_by_kind(node, "export_clause") {
                if let Some(imp) = extract_export_clause_import(&clause, source, &source_node) {
                    imports.push(imp);
                }
            } else {
                // export * from "path" - add as import with wildcard
                imports.push(TsImport {
                    path,
                    symbols: vec!["*".to_string()],
                    default_import: None,
                    namespace_import: None,
                    line: node.start_position().row + 1,
                });
            }
            return; // This is a re-export, not a local symbol export
        }
    }

    // Find the actual declaration inside export
    cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Function, true, is_default)
                {
                    symbols.push(sym);
                }
            }
            "class_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Class, true, is_default)
                {
                    symbols.push(sym);
                }
            }
            "interface_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Interface, true, is_default)
                {
                    symbols.push(sym);
                }
            }
            "type_alias_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::TypeAlias, true, is_default)
                {
                    symbols.push(sym);
                }
            }
            "enum_declaration" => {
                if let Some(sym) =
                    extract_symbol(&child, source, TsSymbolKind::Enum, true, is_default)
                {
                    symbols.push(sym);
                }
            }
            "variable_declaration" => {
                if let Some(sym) = extract_variable(&child, source, true, is_default) {
                    symbols.push(sym);
                }
            }
            "lexical_declaration" => {
                if let Some(sym) = extract_lexical_declaration(&child, source, true, is_default) {
                    symbols.push(sym);
                }
            }
            _ => {}
        }
    }
}

fn extract_import(node: &Node, source: &str) -> Option<TsImport> {
    let source_node = find_child_by_kind(node, "string")?;
    let path = extract_string_content(&source_node, source);
    let line = node.start_position().row + 1;

    let mut default_import = None;
    let mut namespace_import = None;
    let mut symbols = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_clause" => {
                let mut clause_cursor = child.walk();
                for clause_child in child.children(&mut clause_cursor) {
                    match clause_child.kind() {
                        "identifier" => {
                            // Default import: import React from "..."
                            default_import = Some(source[clause_child.byte_range()].to_string());
                        }
                        "named_imports" => {
                            // Named imports: import { a, b } from "..."
                            symbols.extend(extract_named_imports(&clause_child, source));
                        }
                        "namespace_import" => {
                            // Namespace import: import * as utils from "..."
                            if let Some(name) = extract_namespace_import(&clause_child, source) {
                                namespace_import = Some(name);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "identifier" => {
                // Direct default import without import_clause
                default_import = Some(source[child.byte_range()].to_string());
            }
            _ => {}
        }
    }

    Some(TsImport {
        path,
        symbols,
        default_import,
        namespace_import,
        line,
    })
}

fn extract_named_imports(node: &Node, source: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "import_specifier" {
            // Get the name being imported (handles "as" aliases by taking original name)
            if let Some(name_node) = child.child_by_field_name("name") {
                symbols.push(source[name_node.byte_range()].to_string());
            } else {
                // Fallback: get first identifier
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == "identifier" || inner_child.kind() == "type_identifier"
                    {
                        symbols.push(source[inner_child.byte_range()].to_string());
                        break;
                    }
                }
            }
        }
    }

    symbols
}

fn extract_namespace_import(node: &Node, source: &str) -> Option<String> {
    // namespace_import structure: "*" "as" identifier
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some(source[child.byte_range()].to_string());
        }
    }
    None
}

fn extract_export_clause_import(node: &Node, source: &str, source_node: &Node) -> Option<TsImport> {
    let path = extract_string_content(source_node, source);
    let line = node.start_position().row + 1;

    let mut symbols = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "export_specifier" {
            if let Some(name_node) = child.child_by_field_name("name") {
                symbols.push(source[name_node.byte_range()].to_string());
            } else {
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == "identifier" || inner_child.kind() == "type_identifier"
                    {
                        symbols.push(source[inner_child.byte_range()].to_string());
                        break;
                    }
                }
            }
        }
    }

    Some(TsImport {
        path,
        symbols,
        default_import: None,
        namespace_import: None,
        line,
    })
}

fn extract_symbol(
    node: &Node,
    source: &str,
    kind: TsSymbolKind,
    exported: bool,
    is_default_export: bool,
) -> Option<TsSymbol> {
    let name = item_name(node, source)?;
    let line_start = node.start_position().row + 1;
    let line_end = node.end_position().row + 1;

    Some(TsSymbol {
        name,
        kind,
        line_start,
        line_end,
        exported,
        is_default_export,
    })
}

fn extract_variable(
    node: &Node,
    source: &str,
    exported: bool,
    is_default_export: bool,
) -> Option<TsSymbol> {
    // variable_declaration: kind="var" + declarator(s)
    let kind_str = node
        .child_by_field_name("kind")
        .map(|k| source[k.byte_range()].to_string())
        .unwrap_or_else(|| "var".to_string());

    let kind = match kind_str.as_str() {
        "const" => TsSymbolKind::Const,
        "let" => TsSymbolKind::Let,
        _ => TsSymbolKind::Var,
    };

    // Get first declarator's name
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name) = child.child_by_field_name("name") {
                let name_str = source[name.byte_range()].to_string();
                let line_start = node.start_position().row + 1;
                let line_end = node.end_position().row + 1;

                return Some(TsSymbol {
                    name: name_str,
                    kind,
                    line_start,
                    line_end,
                    exported,
                    is_default_export,
                });
            }
        }
    }

    None
}

fn extract_lexical_declaration(
    node: &Node,
    source: &str,
    exported: bool,
    is_default_export: bool,
) -> Option<TsSymbol> {
    // lexical_declaration: "const" or "let" + declarator(s)
    let text = source[node.byte_range()].to_string();
    let kind = if text.starts_with("const") {
        TsSymbolKind::Const
    } else {
        TsSymbolKind::Let
    };

    // Get first declarator's name
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name) = child.child_by_field_name("name") {
                let name_str = source[name.byte_range()].to_string();
                let line_start = node.start_position().row + 1;
                let line_end = node.end_position().row + 1;

                return Some(TsSymbol {
                    name: name_str,
                    kind,
                    line_start,
                    line_end,
                    exported,
                    is_default_export,
                });
            }
        }
    }

    None
}

fn item_name(node: &Node, source: &str) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(source[name_node.byte_range()].to_string());
    }
    None
}

#[allow(clippy::manual_find)]
fn find_child_by_kind<'a>(node: &'a Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

fn extract_string_content(node: &Node, source: &str) -> String {
    let text = &source[node.byte_range()];
    // Remove quotes from string literal
    text.trim_matches('"').trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_function_and_class() {
        let src = r#"
export function foo() {}
export class Bar {}
const secret = 1;
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 2); // foo, Bar (not secret)
        assert!(result.symbols[0].exported);
        assert_eq!(result.symbols[0].name, "foo");
        assert_eq!(result.symbols[0].kind, TsSymbolKind::Function);
        assert!(result.symbols[1].exported);
        assert_eq!(result.symbols[1].name, "Bar");
        assert_eq!(result.symbols[1].kind, TsSymbolKind::Class);
    }

    #[test]
    fn parse_imports() {
        let src = r#"
import React from 'react';
import { useState, useEffect } from 'react';
import * as utils from './utils';
import './styles.css';
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.imports.len(), 4);

        // First import: default import
        assert_eq!(result.imports[0].default_import, Some("React".to_string()));
        assert_eq!(result.imports[0].path, "react");
        assert!(result.imports[0].symbols.is_empty());

        // Second import: named imports
        assert_eq!(result.imports[1].default_import, None);
        assert_eq!(result.imports[1].path, "react");
        assert_eq!(result.imports[1].symbols, vec!["useState", "useEffect"]);

        // Third import: namespace import
        assert_eq!(
            result.imports[2].namespace_import,
            Some("utils".to_string())
        );
        assert_eq!(result.imports[2].path, "./utils");

        // Fourth import: side effect import (no symbols)
        assert_eq!(result.imports[3].default_import, None);
        assert_eq!(result.imports[3].path, "./styles.css");
        assert!(result.imports[3].symbols.is_empty());
    }

    #[test]
    fn parse_reexports() {
        let src = r#"export { foo } from './foo';"#;
        let result = parse_ts_file(src).unwrap();
        // Should appear in exports
        assert_eq!(result.exports.len(), 1);
        assert_eq!(result.exports[0], "./foo");
        // Also appears in imports
        assert_eq!(result.imports.len(), 1);
        assert_eq!(result.imports[0].path, "./foo");
        assert_eq!(result.imports[0].symbols, vec!["foo"]);
    }

    #[test]
    fn parse_interface_and_type_alias() {
        let src = r#"
export interface User {
    name: string;
}
export type ID = string;
interface Internal {}
type Local = number;
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 4);

        assert_eq!(result.symbols[0].name, "User");
        assert_eq!(result.symbols[0].kind, TsSymbolKind::Interface);
        assert!(result.symbols[0].exported);

        assert_eq!(result.symbols[1].name, "ID");
        assert_eq!(result.symbols[1].kind, TsSymbolKind::TypeAlias);
        assert!(result.symbols[1].exported);

        assert_eq!(result.symbols[2].name, "Internal");
        assert_eq!(result.symbols[2].kind, TsSymbolKind::Interface);
        assert!(!result.symbols[2].exported);

        assert_eq!(result.symbols[3].name, "Local");
        assert_eq!(result.symbols[3].kind, TsSymbolKind::TypeAlias);
        assert!(!result.symbols[3].exported);
    }

    #[test]
    fn parse_enum() {
        let src = r#"
export enum Status {
    Active,
    Inactive,
}
enum InternalState {
    A, B
}
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 2);

        assert_eq!(result.symbols[0].name, "Status");
        assert_eq!(result.symbols[0].kind, TsSymbolKind::Enum);
        assert!(result.symbols[0].exported);

        assert_eq!(result.symbols[1].name, "InternalState");
        assert_eq!(result.symbols[1].kind, TsSymbolKind::Enum);
        assert!(!result.symbols[1].exported);
    }

    #[test]
    fn parse_export_default() {
        let src = r#"
export default function main() {}
export default class MyClass {}
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 2);

        assert!(result.symbols[0].exported);
        assert!(result.symbols[0].is_default_export);
        assert_eq!(result.symbols[0].name, "main");

        assert!(result.symbols[1].exported);
        assert!(result.symbols[1].is_default_export);
        assert_eq!(result.symbols[1].name, "MyClass");
    }

    #[test]
    fn parse_const_let_var() {
        let src = r#"
export const PI = 3.14;
export let count = 0;
export var oldVar = 1;
const internal = 42;
"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 3); // Only exported ones

        assert_eq!(result.symbols[0].name, "PI");
        assert_eq!(result.symbols[0].kind, TsSymbolKind::Const);

        assert_eq!(result.symbols[1].name, "count");
        assert_eq!(result.symbols[1].kind, TsSymbolKind::Let);

        assert_eq!(result.symbols[2].name, "oldVar");
        assert_eq!(result.symbols[2].kind, TsSymbolKind::Var);
    }

    #[test]
    fn parse_export_all() {
        let src = r#"export * from './module';"#;
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.exports.len(), 1);
        assert_eq!(result.exports[0], "./module");
    }

    #[test]
    fn parse_empty_file() {
        let result = parse_ts_file("").unwrap();
        assert!(result.symbols.is_empty());
        assert!(result.imports.is_empty());
        assert!(result.exports.is_empty());
    }

    #[test]
    fn parse_line_numbers() {
        let src = "export function foo() {}\n\nexport class Bar {}\n";
        let result = parse_ts_file(src).unwrap();
        assert_eq!(result.symbols.len(), 2);
        assert_eq!(result.symbols[0].line_start, 1);
        assert_eq!(result.symbols[1].line_start, 3);
    }
}
