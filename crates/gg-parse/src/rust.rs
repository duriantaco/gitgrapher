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
(struct_item
  name: (type_identifier) @name) @definition.struct

(enum_item
  name: (type_identifier) @name) @definition.enum

(trait_item
  name: (type_identifier) @name) @definition.trait

(impl_item
  type: (_) @name) @definition.impl

(type_item
  name: (type_identifier) @name) @definition.type_alias

(mod_item
  name: (identifier) @name) @definition.namespace

(macro_definition
  name: (identifier) @name) @definition.macro

(function_item
  name: (identifier) @name) @definition.function

(function_signature_item
  name: (identifier) @name) @definition.method
"#;

const CALLS_QUERY: &str = r#"
(call_expression
  function: (identifier) @call.name) @call

(call_expression
  function: (field_expression
    value: (_) @call.receiver
    field: (field_identifier) @call.name)) @call.method

(call_expression
  function: (scoped_identifier
    path: (_) @call.receiver
    name: (identifier) @call.name)) @call.scoped

(macro_invocation
  macro: (identifier) @call.name) @call.macro
"#;

const IMPORTS_QUERY: &str = r#"
(use_declaration
  argument: (_) @import.source) @import
"#;

const HERITAGE_QUERY: &str = r#"
(impl_item
  trait: [(type_identifier) (scoped_type_identifier name: (type_identifier))] @heritage.implements
  type: (_) @heritage.child) @heritage

(trait_item
  name: (type_identifier) @heritage.child
  bounds: (trait_bounds
    (type_identifier) @heritage.extends)) @heritage
"#;

pub struct RustProvider {
    language: tree_sitter::Language,
}

impl RustProvider {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
        }
    }

    fn parse_tree(&self, path: &Path, source: &[u8]) -> GgResult<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| GgError::Parse {
                file: path.display().to_string(),
                message: format!("Failed to set Rust language: {e}"),
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
            let mut label = NodeLabel::CodeElement;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "name" => name = Some(capture.node.utf8_text(source).unwrap_or("")),
                    s if s.starts_with("definition.") => {
                        def_node = Some(capture.node);
                        label = match s {
                            "definition.struct" => NodeLabel::Struct,
                            "definition.enum" => NodeLabel::Enum,
                            "definition.trait" => NodeLabel::Trait,
                            "definition.impl" => NodeLabel::Impl,
                            "definition.type_alias" => NodeLabel::TypeAlias,
                            "definition.namespace" => NodeLabel::Namespace,
                            "definition.macro" => NodeLabel::Macro,
                            "definition.function" => NodeLabel::Function,
                            "definition.method" => NodeLabel::Method,
                            _ => NodeLabel::CodeElement,
                        };
                    }
                    _ => {}
                }
            }

            if let (Some(name_str), Some(node)) = (name, def_node) {
                if name_str.is_empty() {
                    continue;
                }

                let actual_label = if label == NodeLabel::Function && is_rust_method(node) {
                    NodeLabel::Method
                } else {
                    label
                };
                let clean_name = clean_rust_type_name(name_str);
                let start = node.start_position();
                let end = node.end_position();
                let id = SmolStr::new(format!("{}::{}::{}", file_path, clean_name, start.row + 1));
                let container_name = rust_container_name(node, source);
                let body_hash = hash_node_body(node, source);

                let mut props = NodeProperties::symbol(
                    clean_name.as_str(),
                    file_path,
                    Language::Rust,
                    (start.row + 1) as u32,
                    (end.row + 1) as u32,
                );
                props.is_exported = rust_visibility(node, source).is_some();
                props.is_async = node
                    .utf8_text(source)
                    .is_ok_and(|text| text.contains("async fn"));
                if let Some(params) = count_rust_params(node) {
                    props.parameter_count = Some(params);
                }
                props.return_type = rust_return_type(node, source).map(SmolStr::new);
                props.visibility = rust_visibility(node, source).map(SmolStr::new);
                props.set_diff_metadata(actual_label, container_name, body_hash);

                nodes.push(GraphNode {
                    id,
                    label: actual_label,
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
                    "call" | "call.method" | "call.scoped" | "call.macro" => {
                        call_node = Some(capture.node);
                    }
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
            let mut import_text: Option<&str> = None;
            let mut import_node: Option<Node> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "import.source" => {
                        import_text = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "import" => import_node = Some(capture.node),
                    _ => {}
                }
            }

            if let (Some(text), Some(node)) = (import_text, import_node) {
                let line = (node.start_position().row + 1) as u32;
                for item in parse_rust_use(text) {
                    imports.push(ExtractedImport {
                        source_file: SmolStr::new(file_path),
                        imported_name: SmolStr::new(item.imported_name),
                        module_path: SmolStr::new(item.module_path),
                        alias: item.alias.map(SmolStr::new),
                        is_default: false,
                        is_namespace: false,
                        line,
                    });
                }
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
            let mut child_name: Option<String> = None;
            let mut parent_name: Option<&str> = None;
            let mut kind = HeritageKind::Extends;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "heritage.child" => {
                        child_name = Some(clean_rust_type_name(
                            capture.node.utf8_text(source).unwrap_or(""),
                        ));
                    }
                    "heritage.extends" => {
                        parent_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                        kind = HeritageKind::Extends;
                    }
                    "heritage.implements" => {
                        parent_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                        kind = HeritageKind::Implements;
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
                    parent_name: SmolStr::new(clean_rust_type_name(parent)),
                    kind,
                    file_path: SmolStr::new(file_path),
                });
            }
        }

        Ok(heritage)
    }
}

impl Default for RustProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for RustProvider {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extensions(&self) -> &[&str] {
        &["rs"]
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

struct RustUseItem {
    imported_name: String,
    module_path: String,
    alias: Option<String>,
}

fn hash_node_body(node: Node, source: &[u8]) -> Option<u64> {
    node.utf8_text(source)
        .ok()
        .map(|text| xxh3_64(text.as_bytes()))
}

fn is_rust_method(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "impl_item" | "trait_item" => return true,
            "source_file" | "mod_item" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}

fn rust_container_name(node: Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "impl_item" => {
                return parent
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(clean_rust_type_name);
            }
            "trait_item" | "struct_item" | "enum_item" => {
                return parent
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(String::from);
            }
            _ => current = parent.parent(),
        }
    }
    None
}

fn rust_visibility<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return child.utf8_text(source).ok();
        }
    }
    None
}

fn count_rust_params(node: Node) -> Option<u32> {
    node.child_by_field_name("parameters").map(|params| {
        let mut count = 0;
        let mut cursor = params.walk();
        for child in params.named_children(&mut cursor) {
            match child.kind() {
                "parameter" | "variadic_parameter" => count += 1,
                "self_parameter" => {}
                _ => {}
            }
        }
        count
    })
}

fn rust_return_type<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name("return_type")
        .and_then(|n| n.utf8_text(source).ok())
}

fn clean_rust_type_name(text: &str) -> String {
    let trimmed = text.trim();
    let without_ref = trimmed
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();
    let base = without_ref
        .split('<')
        .next()
        .unwrap_or(without_ref)
        .trim()
        .trim_start_matches("crate::")
        .trim_start_matches("self::")
        .trim_start_matches("super::");
    base.rsplit("::").next().unwrap_or(base).trim().to_string()
}

fn parse_rust_use(text: &str) -> Vec<RustUseItem> {
    let normalized = text.trim().trim_end_matches(';').trim();
    if normalized.is_empty() {
        return Vec::new();
    }

    if let Some((prefix, list)) = normalized.split_once("::{") {
        let list = list.trim_end_matches('}');
        return list
            .split(',')
            .filter(|part| !part.is_empty())
            .map(|part| rust_use_item(prefix, part))
            .collect();
    }

    vec![rust_use_item("", normalized)]
}

fn rust_use_item(prefix: &str, item: &str) -> RustUseItem {
    let item = item.trim();
    let (path, alias) = item
        .split_once(" as ")
        .map(|(name, alias)| (name.trim(), Some(alias.trim().to_string())))
        .unwrap_or((item, None));
    let full = if prefix.is_empty() {
        path.to_string()
    } else {
        format!("{prefix}::{path}")
    };
    let mut parts: Vec<&str> = full
        .split("::")
        .map(str::trim)
        .filter(|part| !matches!(*part, "crate" | "self" | "super"))
        .filter(|part| !part.is_empty())
        .collect();
    let imported_name = parts.pop().unwrap_or(path).to_string();
    let module_path = if parts.is_empty() {
        imported_name.clone()
    } else {
        parts.join("/")
    };
    RustUseItem {
        imported_name,
        module_path,
        alias,
    }
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

    fn parse_rs(code: &str) -> ParseResult {
        let provider = RustProvider::new();
        let config = Config::default();
        let path = Path::new("sample.rs");
        provider.parse(path, code.as_bytes(), &config).unwrap()
    }

    #[test]
    fn extracts_rust_fixture() {
        let result = parse_rs(include_str!("../fixtures/rust/sample.rs"));
        let labels: Vec<_> = result
            .nodes
            .iter()
            .map(|n| (n.label, n.properties.name.as_str()))
            .collect();

        assert!(labels.contains(&(NodeLabel::Struct, "User")));
        assert!(labels.contains(&(NodeLabel::Trait, "Repository")));
        assert!(labels.contains(&(NodeLabel::Impl, "User")));
        assert!(labels.contains(&(NodeLabel::Function, "build_user")));
        assert!(labels.contains(&(NodeLabel::Method, "name")));
        assert!(!result.imports.is_empty());
        assert!(result.calls.iter().any(|call| call.callee_name == "new"));
        assert!(result
            .heritage
            .iter()
            .any(|h| h.kind == HeritageKind::Implements && h.parent_name == "Repository"));
    }

    #[test]
    fn parses_rust_use_alias() {
        let items = parse_rust_use("crate::services::UserService as Service");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].imported_name, "UserService");
        assert_eq!(items[0].module_path, "services");
        assert_eq!(items[0].alias.as_deref(), Some("Service"));
    }
}
