use gg_core::config::Config;
use gg_core::error::{GgError, GgResult};
use gg_core::types::*;
use smol_str::SmolStr;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};
use xxhash_rust::xxh3::xxh3_64;

use crate::language::LanguageProvider;

const DEFINITIONS_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @definition.function

(method_declaration
  name: (field_identifier) @name) @definition.method

(type_spec
  name: (type_identifier) @name
  type: (_) @type.kind) @definition.type

(type_alias
  name: (type_identifier) @name) @definition.type_alias

(method_elem
  name: (field_identifier) @name) @definition.method
"#;

const CALLS_QUERY: &str = r#"
(call_expression
  function: (identifier) @call.name) @call

(call_expression
  function: (selector_expression
    operand: (_) @call.receiver
    field: (field_identifier) @call.name)) @call.method
"#;

const IMPORTS_QUERY: &str = r#"
(import_spec
  path: [(interpreted_string_literal) (raw_string_literal)] @import.source) @import
"#;

const HERITAGE_QUERY: &str = r#"
(type_spec
  name: (type_identifier) @heritage.child
  type: (interface_type
    (type_elem
      (type_identifier) @heritage.extends))) @heritage

(type_spec
  name: (type_identifier) @heritage.child
  type: (struct_type
    (field_declaration_list
      (field_declaration
        type: (type_identifier) @heritage.extends)))) @heritage
"#;

pub struct GoProvider {
    language: tree_sitter::Language,
}

impl GoProvider {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_go::LANGUAGE.into(),
        }
    }

    fn parse_tree(&self, path: &Path, source: &[u8]) -> GgResult<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| GgError::Parse {
                file: path.display().to_string(),
                message: format!("Failed to set Go language: {e}"),
            })?;
        parser.parse(source, None).ok_or_else(|| GgError::Parse {
            file: path.display().to_string(),
            message: "Tree-sitter parse returned None".to_string(),
        })
    }

    fn extract_definitions(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> GgResult<Vec<GraphNode>> {
        let lang = tree.language();
        let query = Query::new(&lang, DEFINITIONS_QUERY).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Definition query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut nodes = Vec::new();

        while let Some(m) = matches.next() {
            let mut name: Option<&str> = None;
            let mut def_node: Option<Node> = None;
            let mut type_kind: Option<&str> = None;
            let mut label = NodeLabel::CodeElement;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "name" => name = Some(capture.node.utf8_text(source).unwrap_or("")),
                    "type.kind" => type_kind = Some(capture.node.kind()),
                    s if s.starts_with("definition.") => {
                        def_node = Some(capture.node);
                        label = match s {
                            "definition.function" => NodeLabel::Function,
                            "definition.method" => NodeLabel::Method,
                            "definition.type" => label_for_go_type(type_kind),
                            "definition.type_alias" => NodeLabel::TypeAlias,
                            _ => NodeLabel::CodeElement,
                        };
                    }
                    _ => {}
                }
            }

            if label == NodeLabel::Type {
                label = label_for_go_type(type_kind);
            }

            if let (Some(name_str), Some(node)) = (name, def_node) {
                if name_str.is_empty() {
                    continue;
                }

                let start = node.start_position();
                let end = node.end_position();
                let id = SmolStr::new(format!("{}::{}::{}", file_path, name_str, start.row + 1));
                let container_name = go_container_name(node, source);
                let body_hash = hash_node_body(node, source);

                let mut props = NodeProperties::symbol(
                    name_str,
                    file_path,
                    Language::Go,
                    (start.row + 1) as u32,
                    (end.row + 1) as u32,
                );
                props.is_exported = is_go_exported(name_str);
                if let Some(params) = count_go_params(node) {
                    props.parameter_count = Some(params);
                }
                props.return_type = go_return_type(node, source).map(SmolStr::new);
                props.set_diff_metadata(label, container_name, body_hash);

                nodes.push(GraphNode {
                    id,
                    label,
                    properties: props,
                });
            }
        }

        Ok(nodes)
    }

    fn extract_calls(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        definitions: &[GraphNode],
    ) -> GgResult<Vec<ExtractedCall>> {
        let lang = tree.language();
        let query = Query::new(&lang, CALLS_QUERY).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Calls query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut calls = Vec::new();

        while let Some(m) = matches.next() {
            let mut callee_name: Option<&str> = None;
            let mut receiver: Option<&str> = None;
            let mut call_node: Option<Node> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "call.name" => {
                        callee_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "call.receiver" => {
                        receiver = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "call" | "call.method" => call_node = Some(capture.node),
                    _ => {}
                }
            }

            if let (Some(name), Some(node)) = (callee_name, call_node) {
                if name.is_empty() {
                    continue;
                }
                let line = (node.start_position().row + 1) as u32;
                let caller_id = find_enclosing(node, definitions, file_path)
                    .unwrap_or_else(|| SmolStr::new(format!("{}::__module__", file_path)));
                let args_count = node
                    .child_by_field_name("arguments")
                    .map(|args| args.named_child_count() as u32)
                    .unwrap_or(0);

                calls.push(ExtractedCall {
                    caller_id,
                    callee_name: SmolStr::new(name),
                    receiver: receiver.map(SmolStr::new),
                    file_path: SmolStr::new(file_path),
                    line,
                    arguments_count: args_count,
                });
            }
        }

        Ok(calls)
    }

    fn extract_imports(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> GgResult<Vec<ExtractedImport>> {
        let lang = tree.language();
        let query = Query::new(&lang, IMPORTS_QUERY).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Imports query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut imports = Vec::new();

        while let Some(m) = matches.next() {
            let mut module_source: Option<String> = None;
            let mut import_node: Option<Node> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "import.source" => {
                        module_source = Some(strip_go_string(
                            capture.node.utf8_text(source).unwrap_or(""),
                        ));
                    }
                    "import" => import_node = Some(capture.node),
                    _ => {}
                }
            }

            if let (Some(module_path), Some(node)) = (module_source, import_node) {
                let line = (node.start_position().row + 1) as u32;
                let alias = node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .filter(|name| *name != "." && *name != "_")
                    .map(String::from);
                let imported_name = alias
                    .clone()
                    .unwrap_or_else(|| last_path_segment(&module_path).to_string());

                imports.push(ExtractedImport {
                    source_file: SmolStr::new(file_path),
                    imported_name: SmolStr::new(imported_name),
                    module_path: SmolStr::new(module_path),
                    alias: alias.map(SmolStr::new),
                    is_default: false,
                    is_namespace: true,
                    line,
                });
            }
        }

        Ok(imports)
    }

    fn extract_heritage(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        definitions: &[GraphNode],
    ) -> GgResult<Vec<ExtractedHeritage>> {
        let lang = tree.language();
        let query = Query::new(&lang, HERITAGE_QUERY).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Heritage query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut heritage = Vec::new();

        while let Some(m) = matches.next() {
            let mut child_name: Option<&str> = None;
            let mut parent_name: Option<&str> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "heritage.child" => {
                        child_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "heritage.extends" => {
                        parent_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    _ => {}
                }
            }

            if let (Some(child), Some(parent)) = (child_name, parent_name) {
                if child == parent {
                    continue;
                }
                let child_id = definitions
                    .iter()
                    .find(|n| n.properties.name.as_str() == child)
                    .map(|n| n.id.clone())
                    .unwrap_or_else(|| SmolStr::new(format!("{}::{}", file_path, child)));

                heritage.push(ExtractedHeritage {
                    child_id,
                    parent_name: SmolStr::new(parent),
                    kind: HeritageKind::Extends,
                    file_path: SmolStr::new(file_path),
                });
            }
        }

        Ok(heritage)
    }
}

impl Default for GoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for GoProvider {
    fn language(&self) -> Language {
        Language::Go
    }

    fn extensions(&self) -> &[&str] {
        &["go"]
    }

    fn parse(&self, path: &Path, source: &[u8], _config: &Config) -> GgResult<ParseResult> {
        let tree = self.parse_tree(path, source)?;
        let file_path = path.to_string_lossy();

        let definitions = self.extract_definitions(&tree, source, &file_path)?;
        let calls = self.extract_calls(&tree, source, &file_path, &definitions)?;
        let imports = self.extract_imports(&tree, source, &file_path)?;
        let heritage = self.extract_heritage(&tree, source, &file_path, &definitions)?;

        Ok(ParseResult {
            nodes: definitions,
            imports,
            calls,
            heritage,
            assignments: Vec::new(),
        })
    }
}

fn label_for_go_type(type_kind: Option<&str>) -> NodeLabel {
    match type_kind {
        Some("struct_type") => NodeLabel::Struct,
        Some("interface_type") => NodeLabel::Interface,
        _ => NodeLabel::Type,
    }
}

fn hash_node_body(node: Node, source: &[u8]) -> Option<u64> {
    node.utf8_text(source)
        .ok()
        .map(|text| xxh3_64(text.as_bytes()))
}

fn is_go_exported(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_uppercase() && ch.is_alphabetic())
}

fn count_go_params(node: Node) -> Option<u32> {
    node.child_by_field_name("parameters")
        .map(count_parameter_list_items)
}

fn count_parameter_list_items(node: Node) -> u32 {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "parameter_declaration" => {
                let named = child
                    .children_by_field_name("name", &mut child.walk())
                    .count() as u32;
                count += named.max(1);
            }
            "variadic_parameter_declaration" => count += 1,
            _ => {}
        }
    }
    count
}

fn go_return_type<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name("result")
        .and_then(|n| n.utf8_text(source).ok())
}

fn go_container_name(node: Node, source: &[u8]) -> Option<String> {
    if node.kind() == "method_declaration" {
        return node
            .child_by_field_name("receiver")
            .and_then(|receiver| receiver_type_name(receiver, source));
    }

    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "type_spec" {
            return parent
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(String::from);
        }
        current = parent.parent();
    }
    None
}

fn receiver_type_name(node: Node, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?;
    let cleaned = text
        .trim_matches(|ch| ch == '(' || ch == ')')
        .trim()
        .trim_start_matches('*')
        .trim();
    cleaned
        .split_whitespace()
        .last()
        .map(|name| name.trim_start_matches('*').to_string())
        .filter(|name| !name.is_empty())
}

fn strip_go_string(text: &str) -> String {
    text.trim_matches(|ch| ch == '"' || ch == '`').to_string()
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn find_enclosing(node: Node, definitions: &[GraphNode], file_path: &str) -> Option<SmolStr> {
    let call_line = (node.start_position().row + 1) as u32;
    definitions
        .iter()
        .filter(|d| {
            d.properties.file_path.as_str() == file_path
                && d.properties.start_line.unwrap_or(0) <= call_line
                && d.properties.end_line.unwrap_or(u32::MAX) >= call_line
        })
        .min_by_key(|d| {
            d.properties.end_line.unwrap_or(u32::MAX) - d.properties.start_line.unwrap_or(0)
        })
        .map(|d| d.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_go(code: &str) -> ParseResult {
        let provider = GoProvider::new();
        let config = Config::default();
        let path = Path::new("sample.go");
        provider.parse(path, code.as_bytes(), &config).unwrap()
    }

    #[test]
    fn extracts_go_fixture() {
        let result = parse_go(include_str!("../fixtures/go/sample.go"));
        let labels: Vec<_> = result
            .nodes
            .iter()
            .map(|n| (n.label, n.properties.name.as_str()))
            .collect();

        assert!(labels.contains(&(NodeLabel::Struct, "User")));
        assert!(labels.contains(&(NodeLabel::Interface, "Repository")));
        assert!(labels.contains(&(NodeLabel::Function, "NewUser")));
        assert!(labels.contains(&(NodeLabel::Method, "Name")));
        assert!(!result.imports.is_empty());
        assert!(result
            .calls
            .iter()
            .any(|call| call.callee_name == "Println"));
        assert!(result.heritage.iter().any(|h| h.parent_name == "Embedded"));
    }

    #[test]
    fn marks_exported_go_symbols() {
        let result = parse_go("package app\n\nfunc NewUser() {}\nfunc helper() {}\n");
        let exported = result
            .nodes
            .iter()
            .find(|n| n.properties.name == "NewUser")
            .unwrap();
        let private = result
            .nodes
            .iter()
            .find(|n| n.properties.name == "helper")
            .unwrap();
        assert!(exported.properties.is_exported);
        assert!(!private.properties.is_exported);
    }
}
