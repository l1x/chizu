use tree_sitter::Node;

use crate::error::IndexError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstroFrontmatterProp {
    pub name: String,
    pub ts_type: Option<String>, // TypeScript type annotation if present
    pub has_default: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstroSlot {
    pub name: String, // "default" for <slot />, named for <slot name="x" />
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstroImport {
    pub path: String,
    pub symbols: Vec<String>,
    pub default_import: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct AstroParseResult {
    pub frontmatter_props: Vec<AstroFrontmatterProp>,
    pub slots: Vec<AstroSlot>,
    pub imports: Vec<AstroImport>,
    pub client_directives: Vec<String>, // "client:load", "client:idle", etc.
}

/// Parse an Astro file, extracting frontmatter props, slots, imports, and client directives.
pub fn parse_astro_file(source: &str) -> Result<AstroParseResult, IndexError> {
    let mut frontmatter_props = Vec::new();
    let mut slots = Vec::new();
    let mut imports = Vec::new();
    let mut client_directives = Vec::new();

    // Split the source into frontmatter and template sections
    let (frontmatter, template_start) = split_frontmatter(source);

    // Parse frontmatter as TypeScript if present
    if let Some(fm) = frontmatter {
        parse_frontmatter(fm, source, &mut frontmatter_props, &mut imports)?;
    }

    // Parse template for slots and client directives
    if let Some(start) = template_start {
        parse_template(source, start, &mut slots, &mut client_directives);
    }

    Ok(AstroParseResult {
        frontmatter_props,
        slots,
        imports,
        client_directives,
    })
}

/// Split the source into frontmatter content (between --- markers) and template start position.
/// Returns (Some((start, end)), template_start) where template_start is the byte position
/// where the template section begins.
fn split_frontmatter(source: &str) -> (Option<(usize, usize)>, Option<usize>) {
    // Find the first --- marker
    let start_marker = match source.find("---") {
        Some(pos) => pos,
        None => return (None, Some(0)), // No frontmatter, entire file is template starting at 0
    };

    // Find the end of the first --- line
    let after_start = start_marker + 3;
    let rest = &source[after_start..];

    // Find the closing --- marker
    let end_marker_rel = match rest.find("---") {
        Some(pos) => pos,
        None => return (None, Some(0)), // No closing marker, treat as template starting at 0
    };

    let frontmatter_start = after_start;
    let frontmatter_end = after_start + end_marker_rel;
    let template_start = frontmatter_end + 3;

    let frontmatter = if frontmatter_start < frontmatter_end {
        Some((frontmatter_start, frontmatter_end))
    } else {
        None
    };

    let template = if template_start < source.len() {
        Some(template_start)
    } else {
        None
    };

    (frontmatter, template)
}

/// Parse the frontmatter section using tree-sitter-typescript.
fn parse_frontmatter(
    frontmatter_range: (usize, usize),
    source: &str,
    props: &mut Vec<AstroFrontmatterProp>,
    imports: &mut Vec<AstroImport>,
) -> Result<(), IndexError> {
    let (start, end) = frontmatter_range;
    let frontmatter_source = &source[start..end];

    // Parse frontmatter as TypeScript
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    parser
        .set_language(&language)
        .map_err(|e| IndexError::Parse(e.to_string()))?;

    let tree = parser
        .parse(frontmatter_source, None)
        .ok_or_else(|| IndexError::Parse("tree-sitter failed to parse frontmatter".to_string()))?;

    let root = tree.root_node();
    extract_from_frontmatter(&root, frontmatter_source, start, props, imports);

    Ok(())
}

/// Extract props and imports from the frontmatter AST.
fn extract_from_frontmatter(
    root: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
    imports: &mut Vec<AstroImport>,
) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                if let Some(imp) = extract_import(&child, source, offset) {
                    imports.push(imp);
                }
            }
            "interface_declaration" => {
                // Check if this is `interface Props`
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &source[name_node.byte_range()];
                    if name == "Props" {
                        extract_props_interface(&child, source, offset, props);
                    }
                }
            }
            "lexical_declaration" => {
                // Check for `const { ... } = Astro.props` pattern
                extract_props_destructuring(&child, source, offset, props);
            }
            _ => {}
        }
    }
}

/// Extract an import statement.
fn extract_import(node: &Node, source: &str, offset: usize) -> Option<AstroImport> {
    let line = node.start_position().row + 1 + offset_to_line(source, offset);

    // Get the import clause (the part between 'import' and 'from')
    let mut default_import = None;
    let mut symbols = Vec::new();
    let mut path = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_clause" => {
                let mut clause_cursor = child.walk();
                for clause_child in child.children(&mut clause_cursor) {
                    match clause_child.kind() {
                        "identifier" => {
                            default_import = Some(source[clause_child.byte_range()].to_string());
                        }
                        "named_imports" => {
                            // Extract named imports from { ... }
                            extract_named_imports(&clause_child, source, &mut symbols);
                        }
                        _ => {}
                    }
                }
            }
            "string" => {
                let raw = &source[child.byte_range()];
                path = Some(raw.trim_matches('"').trim_matches('\'').to_string());
            }
            _ => {}
        }
    }

    path.map(|p| AstroImport {
        path: p,
        symbols,
        default_import,
        line,
    })
}

/// Extract named imports from the `{ ... }` clause.
fn extract_named_imports(node: &Node, source: &str, symbols: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_specifier" => {
                // Get the local name (the name used in this file)
                if let Some(name_node) = child.child_by_field_name("name") {
                    symbols.push(source[name_node.byte_range()].to_string());
                } else if let Some(identifier) = child.child(0) {
                    // Simple import: { Foo }
                    if identifier.kind() == "identifier" {
                        symbols.push(source[identifier.byte_range()].to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract properties from `interface Props { ... }`.
fn extract_props_interface(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    // Find the body of the interface
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "interface_body" || child.kind() == "object_type" {
            extract_interface_body(&child, source, offset, props);
        }
    }
}

/// Extract property signatures from interface body.
fn extract_interface_body(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "property_signature" {
            extract_property_signature(&child, source, offset, props);
        }
    }
}

/// Extract a single property signature.
fn extract_property_signature(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };

    let name = source[name_node.byte_range()].to_string();
    let line = node.start_position().row + 1 + offset_to_line(source, offset);

    // Check for type annotation
    let mut ts_type = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_annotation" {
            ts_type = Some(source[child.byte_range()].to_string());
        }
    }

    props.push(AstroFrontmatterProp {
        name,
        ts_type,
        has_default: false, // Will be updated if destructuring pattern is found
        line,
    });
}

/// Extract `const { a, b = default } = Astro.props` pattern.
fn extract_props_destructuring(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    // Check if this is a const declaration with Astro.props
    let text = &source[node.byte_range()];

    // Quick check for Astro.props pattern
    if !text.contains("Astro.props") {
        return;
    }

    // Find the variable declarator
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            extract_destructuring_pattern(&child, source, offset, props);
        }
    }
}

/// Extract destructuring pattern from variable declarator.
fn extract_destructuring_pattern(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    // Check if the value references Astro.props
    let full_text = &source[node.byte_range()];
    if !full_text.contains("Astro.props") {
        return;
    }

    // Find the object_pattern (destructuring)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "object_pattern" {
            extract_object_pattern(&child, source, offset, props);
        }
    }
}

/// Extract properties from object destructuring pattern.
fn extract_object_pattern(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "shorthand_property_identifier_pattern" => {
                // Simple destructuring: { foo }
                let name = source[child.byte_range()].to_string();
                let line = child.start_position().row + 1 + offset_to_line(source, offset);

                // Check if this prop already exists from interface parsing
                if let Some(existing) = props.iter_mut().find(|p| p.name == name) {
                    // Update line if needed
                    existing.line = line;
                } else {
                    props.push(AstroFrontmatterProp {
                        name,
                        ts_type: None,
                        has_default: false,
                        line,
                    });
                }
            }
            "object_assignment_pattern" => {
                // Destructuring with default: { foo = defaultValue }
                extract_assignment_pattern(&child, source, offset, props);
            }
            _ => {}
        }
    }
}

/// Extract an assignment pattern (key = value) from destructuring.
fn extract_assignment_pattern(
    node: &Node,
    source: &str,
    offset: usize,
    props: &mut Vec<AstroFrontmatterProp>,
) {
    let line = node.start_position().row + 1 + offset_to_line(source, offset);

    // For object_assignment_pattern in TypeScript:
    // - Contains: shorthand_property_identifier_pattern, "=", and the default value
    let mut cursor = node.walk();
    let mut name = None;

    for child in node.children(&mut cursor) {
        match child.kind() {
            "shorthand_property_identifier_pattern" | "identifier" => {
                name = Some(source[child.byte_range()].to_string());
            }
            _ => {}
        }
    }

    // For object_assignment_pattern, having this node type means there's a default value
    let has_default = true;

    if let Some(name) = name {
        // Check if this prop already exists
        if let Some(existing) = props.iter_mut().find(|p| p.name == name) {
            existing.has_default = has_default;
            existing.line = line;
        } else {
            props.push(AstroFrontmatterProp {
                name,
                ts_type: None,
                has_default,
                line,
            });
        }
    }
}

/// Convert byte offset to line number (0-indexed).
fn offset_to_line(source: &str, offset: usize) -> usize {
    let slice = &source[..offset.min(source.len())];
    slice.matches('\n').count()
}

/// Parse the template section for slots and client directives.
fn parse_template(
    source: &str,
    template_start: usize,
    slots: &mut Vec<AstroSlot>,
    client_directives: &mut Vec<String>,
) {
    let template = &source[template_start..];

    // Parse slots using simple pattern matching
    parse_slots(template, template_start, source, slots);

    // Parse client directives
    parse_client_directives(template, client_directives);
}

/// Parse slot tags from the template using simple string scanning.
fn parse_slots(template: &str, template_offset: usize, source: &str, slots: &mut Vec<AstroSlot>) {
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Look for '<slot'
        if i + 4 < chars.len()
            && chars[i] == '<'
            && chars[i + 1] == 's'
            && chars[i + 2] == 'l'
            && chars[i + 3] == 'o'
            && chars[i + 4] == 't'
        {
            // Found slot tag start, now find the end of the tag
            let start = i;
            i += 5;

            // Check for word boundary (space, >, or /)
            if i < chars.len() && !is_word_char(chars[i]) {
                // Calculate line number in the original source
                let template_line = template[..start].matches('\n').count();
                // Line number is absolute in the original source (1-indexed)
                let line = template_line + source[..template_offset].matches('\n').count() + 1;

                // Parse attributes
                let mut name = "default".to_string();

                // Find the end of the tag
                while i < chars.len() && chars[i] != '>' {
                    // Look for name attribute
                    if i + 4 < chars.len()
                        && chars[i] == 'n'
                        && chars[i + 1] == 'a'
                        && chars[i + 2] == 'm'
                        && chars[i + 3] == 'e'
                    {
                        // Found 'name', now look for ="value"
                        let mut j = i + 4;
                        // Skip whitespace
                        while j < chars.len() && chars[j].is_whitespace() {
                            j += 1;
                        }
                        if j < chars.len() && chars[j] == '=' {
                            j += 1;
                            // Skip whitespace
                            while j < chars.len() && chars[j].is_whitespace() {
                                j += 1;
                            }
                            // Find quote char
                            if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') {
                                let quote = chars[j];
                                j += 1;
                                let name_start = j;
                                while j < chars.len() && chars[j] != quote {
                                    j += 1;
                                }
                                if j < chars.len() {
                                    name = template[name_start..j].to_string();
                                }
                            }
                        }
                    }
                    i += 1;
                }

                slots.push(AstroSlot { name, line });
            }
        }
        i += 1;
    }
}

/// Check if a character is a word character (letter, digit, or underscore).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Parse client directives from the template using simple string scanning.
fn parse_client_directives(template: &str, client_directives: &mut Vec<String>) {
    let directives = [
        "client:load",
        "client:idle",
        "client:visible",
        "client:media",
        "client:only",
    ];

    for directive in &directives {
        if template.contains(directive) && !client_directives.contains(&directive.to_string()) {
            client_directives.push(directive.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_props() {
        let src = r#"---
interface Props {
    title: string;
    count?: number;
}
const { title, count = 0 } = Astro.props;
---
<h1>{title}</h1>
"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.frontmatter_props.len(), 2);
        assert_eq!(result.frontmatter_props[0].name, "title");
        assert_eq!(result.frontmatter_props[1].name, "count");
        assert!(result.frontmatter_props[1].has_default);
    }

    #[test]
    fn parse_slots() {
        let src = r#"---
---
<slot />
<slot name="header" />
<slot name="footer" />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.slots.len(), 3);
        let slot_names: Vec<_> = result.slots.iter().map(|s| s.name.clone()).collect();
        assert!(slot_names.contains(&"default".to_string()));
        assert!(slot_names.contains(&"header".to_string()));
    }

    #[test]
    fn parse_client_directives() {
        let src = r#"---
import Component from './Component.astro';
---
<Component client:load />
<Component client:idle />
<Other />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.client_directives.len(), 2);
        assert!(result
            .client_directives
            .contains(&"client:load".to_string()));
    }

    #[test]
    fn parse_frontmatter_imports() {
        let src = r#"---
import { Layout } from './Layout.astro';
import Counter from '../components/Counter.astro';
---
<Layout><Counter /></Layout>"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn parse_default_slot_only() {
        let src = r#"---
---
<slot />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.slots.len(), 1);
        assert_eq!(result.slots[0].name, "default");
    }

    #[test]
    fn parse_multiple_client_directives_same_type() {
        let src = r#"---
---
<ComponentA client:load />
<ComponentB client:load />
<ComponentC client:visible />"#;
        let result = parse_astro_file(src).unwrap();
        // Should only contain unique directives
        assert_eq!(result.client_directives.len(), 2);
        assert!(result
            .client_directives
            .contains(&"client:load".to_string()));
        assert!(result
            .client_directives
            .contains(&"client:visible".to_string()));
    }

    #[test]
    fn parse_interface_props_with_types() {
        let src = r#"---
interface Props {
    title: string;
    count: number;
    optional?: boolean;
}
const { title, count = 0, optional } = Astro.props;
---
<div>{title}</div>
"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.frontmatter_props.len(), 3);

        let title_prop = result
            .frontmatter_props
            .iter()
            .find(|p| p.name == "title")
            .unwrap();
        assert_eq!(title_prop.ts_type.as_deref(), Some(": string"));

        let count_prop = result
            .frontmatter_props
            .iter()
            .find(|p| p.name == "count")
            .unwrap();
        assert!(count_prop.has_default);
    }

    #[test]
    fn parse_import_with_named_and_default() {
        let src = r#"---
import { Layout, Header } from './Layout.astro';
import Counter from '../components/Counter.astro';
import * as utils from '../utils';
---
<Layout><Header /><Counter /></Layout>"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.imports.len(), 3);

        let layout_import = result
            .imports
            .iter()
            .find(|i| i.path == "./Layout.astro")
            .unwrap();
        assert_eq!(layout_import.symbols, vec!["Layout", "Header"]);
        assert!(layout_import.default_import.is_none());

        let counter_import = result
            .imports
            .iter()
            .find(|i| i.path == "../components/Counter.astro")
            .unwrap();
        assert_eq!(counter_import.default_import, Some("Counter".to_string()));
    }

    #[test]
    fn parse_empty_frontmatter() {
        let src = r#"---
---
<div>Hello</div>"#;
        let result = parse_astro_file(src).unwrap();
        assert!(result.frontmatter_props.is_empty());
        assert!(result.imports.is_empty());
    }

    #[test]
    fn parse_no_frontmatter() {
        let src = r#"<div>No frontmatter here</div>"#;
        let result = parse_astro_file(src).unwrap();
        assert!(result.frontmatter_props.is_empty());
        assert!(result.imports.is_empty());
        assert!(result.slots.is_empty());
        assert!(result.client_directives.is_empty());
    }

    #[test]
    fn parse_client_visible_directive() {
        let src = r#"---
---
<Component client:visible />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.client_directives.len(), 1);
        assert!(result
            .client_directives
            .contains(&"client:visible".to_string()));
    }

    #[test]
    fn parse_props_without_interface() {
        let src = r#"---
const { title, description = "" } = Astro.props;
---
<h1>{title}</h1>
<p>{description}</p>"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.frontmatter_props.len(), 2);

        let desc_prop = result
            .frontmatter_props
            .iter()
            .find(|p| p.name == "description")
            .unwrap();
        assert!(desc_prop.has_default);
    }

    #[test]
    fn parse_slot_line_numbers() {
        let src = r#"---
---
<div>
    <slot />
</div>
<slot name="footer" />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.slots.len(), 2);
        // First slot is on line 4 (0-indexed line 3 + 1)
        assert_eq!(result.slots[0].line, 4);
        // Second slot is on line 6 (0-indexed line 5 + 1)
        assert_eq!(result.slots[1].line, 6);
    }

    #[test]
    fn parse_single_quoted_slot_name() {
        let src = r#"---
---
<slot name='sidebar' />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.slots.len(), 1);
        assert_eq!(result.slots[0].name, "sidebar");
    }

    #[test]
    fn parse_slot_with_extra_whitespace() {
        let src = r#"---
---
<slot   name =   "extra"   />"#;
        let result = parse_astro_file(src).unwrap();
        assert_eq!(result.slots.len(), 1);
        assert_eq!(result.slots[0].name, "extra");
    }

    #[test]
    fn parse_client_media_directive() {
        let src = r#"---
---
<Component client:media="(max-width: 768px)" />"#;
        let result = parse_astro_file(src).unwrap();
        assert!(result
            .client_directives
            .contains(&"client:media".to_string()));
    }
}
