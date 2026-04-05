use gg_core::config::Config;
use gg_core::error::{GgError, GgResult};
use gg_core::types::*;
use smol_str::SmolStr;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};

use crate::language::LanguageProvider;

/// JavaScript-compatible definition query (no TS-only nodes like type_identifier).
const JS_DEFINITIONS_QUERY: &str = r#"
; Classes
(class_declaration
  name: (identifier) @name) @definition.class

; Functions
(function_declaration
  name: (identifier) @name) @definition.function

; Arrow functions assigned to variables
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (arrow_function))) @definition.function

; Function expressions assigned to variables
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (function_expression))) @definition.function

; Exported arrow functions
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: (arrow_function)))) @definition.function

; Exported function expressions
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: (function_expression)))) @definition.function

; Methods
(method_definition
  name: (property_identifier) @name) @definition.method

; Class fields
(field_definition
  property: (property_identifier) @name) @definition.property
"#;

/// JavaScript-compatible heritage query.
const JS_HERITAGE_QUERY: &str = r#"
; Class extends
(class_declaration
  name: (identifier) @heritage.class
  (class_heritage
    (identifier) @heritage.extends))
"#;

/// Tree-sitter query for extracting definitions from TypeScript/TSX.
const DEFINITIONS_QUERY: &str = r#"
; Classes
(class_declaration
  name: (type_identifier) @name) @definition.class

; Interfaces
(interface_declaration
  name: (type_identifier) @name) @definition.interface

; Enums
(enum_declaration
  name: (identifier) @name) @definition.enum

; Type aliases
(type_alias_declaration
  name: (type_identifier) @name) @definition.type_alias

; Functions
(function_declaration
  name: (identifier) @name) @definition.function

; Arrow functions assigned to variables
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (arrow_function))) @definition.function

; Function expressions assigned to variables
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (function_expression))) @definition.function

; Exported arrow functions
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: (arrow_function)))) @definition.function

; Exported function expressions
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: (function_expression)))) @definition.function

; Methods
(method_definition
  name: [(property_identifier) (private_property_identifier)] @name) @definition.method

; Abstract methods
(abstract_method_signature
  name: (property_identifier) @name) @definition.method

; Class properties
(public_field_definition
  name: [(property_identifier) (private_property_identifier)] @name) @definition.property
"#;

/// Tree-sitter query for extracting call sites.
const CALLS_QUERY: &str = r#"
; Simple function calls: foo()
(call_expression
  function: (identifier) @call.name) @call

; Method calls: obj.method()
(call_expression
  function: (member_expression
    object: (_) @call.receiver
    property: (property_identifier) @call.name)) @call.method

; Constructor calls: new Foo()
(new_expression
  constructor: (identifier) @call.name) @call.new
"#;

/// Tree-sitter query for extracting imports.
const IMPORTS_QUERY: &str = r#"
; import statements
(import_statement
  source: (string (string_fragment) @import.source)) @import

; re-exports
(export_statement
  source: (string (string_fragment) @reexport.source)) @import.reexport
"#;

/// Tree-sitter query for extracting heritage (extends/implements).
const HERITAGE_QUERY: &str = r#"
; Class extends
(class_declaration
  name: (type_identifier) @heritage.class
  (class_heritage
    (extends_clause
      value: (identifier) @heritage.extends)))

; Class implements
(class_declaration
  name: (type_identifier) @heritage.class
  (class_heritage
    (implements_clause
      (type_identifier) @heritage.implements)))

; Interface extends
(interface_declaration
  name: (type_identifier) @heritage.child
  (extends_type_clause
    (type_identifier) @heritage.extends))
"#;

pub struct TypeScriptProvider {
    ts_language: tree_sitter::Language,
    tsx_language: tree_sitter::Language,
    js_language: tree_sitter::Language,
    jsx_language: tree_sitter::Language,
}

impl TypeScriptProvider {
    pub fn new() -> Self {
        Self {
            ts_language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tsx_language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            js_language: tree_sitter_javascript::LANGUAGE.into(),
            jsx_language: tree_sitter_javascript::LANGUAGE.into(),
        }
    }

    fn language_for_path(&self, path: &Path) -> tree_sitter::Language {
        match path.extension().and_then(|e| e.to_str()) {
            Some("tsx") => self.tsx_language.clone(),
            Some("jsx") => self.jsx_language.clone(),
            Some("js") | Some("mjs") | Some("cjs") => self.js_language.clone(),
            _ => self.ts_language.clone(),
        }
    }

    /// Parse a file as JavaScript (used by JavaScriptProvider).
    pub fn parse_as_js(
        &self,
        path: &Path,
        source: &[u8],
        _config: &Config,
    ) -> GgResult<ParseResult> {
        let lang = match path.extension().and_then(|e| e.to_str()) {
            Some("jsx") => &self.jsx_language,
            _ => &self.js_language,
        };
        let tree = self.parse_tree_with_lang(path, source, lang)?;
        let file_path = path.to_string_lossy();
        let ts_lang = tree.language();

        // Use JS-specific queries (no type_identifier, interface, etc.)
        let definitions =
            self.extract_with_query(&ts_lang, JS_DEFINITIONS_QUERY, &tree, source, &file_path)?;
        let calls = self.extract_calls(&tree, source, &file_path, &definitions)?;
        let imports = self.extract_imports(&tree, source, &file_path)?;

        // Heritage with JS-compatible query
        let heritage = self.extract_heritage_with_query(
            &ts_lang,
            JS_HERITAGE_QUERY,
            &tree,
            source,
            &file_path,
            &definitions,
        )?;

        let mut result = ParseResult {
            nodes: definitions,
            imports,
            calls,
            heritage,
            assignments: Vec::new(),
        };
        for node in &mut result.nodes {
            node.properties.language = Some(Language::JavaScript);
        }
        Ok(result)
    }

    /// Extract definitions using a specific query string.
    fn extract_with_query(
        &self,
        lang: &tree_sitter::Language,
        query_str: &str,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> GgResult<Vec<GraphNode>> {
        let query = Query::new(lang, query_str).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Query error: {e}"),
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
                    "name" => {
                        name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    s if s.starts_with("definition.") => {
                        def_node = Some(capture.node);
                        label = match s {
                            "definition.class" => NodeLabel::Class,
                            "definition.interface" => NodeLabel::Interface,
                            "definition.enum" => NodeLabel::Enum,
                            "definition.type_alias" => NodeLabel::TypeAlias,
                            "definition.function" => NodeLabel::Function,
                            "definition.method" => NodeLabel::Method,
                            "definition.property" => NodeLabel::Property,
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
                let start = node.start_position();
                let end = node.end_position();
                let id = SmolStr::new(format!("{}::{}::{}", file_path, name_str, start.row + 1));
                let is_exported = is_node_exported(node, source);
                let is_async = is_node_async(node, source);
                let param_count = count_parameters(node, source);
                let return_type = extract_return_type(node, source);

                let mut props = NodeProperties::symbol(
                    name_str,
                    file_path,
                    Language::TypeScript,
                    (start.row + 1) as u32,
                    (end.row + 1) as u32,
                );
                props.is_exported = is_exported;
                props.is_async = is_async;
                if let Some(pc) = param_count {
                    props.parameter_count = Some(pc);
                }
                props.return_type = return_type.map(SmolStr::new);

                nodes.push(GraphNode {
                    id,
                    label,
                    properties: props,
                });
            }
        }
        Ok(nodes)
    }

    /// Extract heritage using a specific query string.
    fn extract_heritage_with_query(
        &self,
        lang: &tree_sitter::Language,
        query_str: &str,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        definitions: &[GraphNode],
    ) -> GgResult<Vec<ExtractedHeritage>> {
        let query = Query::new(lang, query_str).map_err(|e| GgError::Parse {
            file: file_path.to_string(),
            message: format!("Heritage query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut heritage = Vec::new();

        while let Some(m) = matches.next() {
            let mut class_name: Option<&str> = None;
            let mut parent_name: Option<&str> = None;
            let mut kind = HeritageKind::Extends;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "heritage.class" | "heritage.child" => {
                        class_name = Some(capture.node.utf8_text(source).unwrap_or(""));
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

            if let (Some(child), Some(parent)) = (class_name, parent_name) {
                let child_id = definitions
                    .iter()
                    .find(|n| n.properties.name.as_str() == child)
                    .map(|n| n.id.clone())
                    .unwrap_or_else(|| SmolStr::new(format!("{}::{}", file_path, child)));
                heritage.push(ExtractedHeritage {
                    child_id,
                    parent_name: SmolStr::new(parent),
                    kind,
                    file_path: SmolStr::new(file_path),
                });
            }
        }
        Ok(heritage)
    }

    fn parse_tree(&self, path: &Path, source: &[u8]) -> GgResult<tree_sitter::Tree> {
        let lang = self.language_for_path(path);
        self.parse_tree_with_lang(path, source, &lang)
    }

    fn parse_tree_with_lang(
        &self,
        path: &Path,
        source: &[u8],
        lang: &tree_sitter::Language,
    ) -> GgResult<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser.set_language(lang).map_err(|e| GgError::Parse {
            file: path.display().to_string(),
            message: format!("Failed to set language: {e}"),
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
                    "name" => {
                        name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    s if s.starts_with("definition.") => {
                        def_node = Some(capture.node);
                        label = match s {
                            "definition.class" => NodeLabel::Class,
                            "definition.interface" => NodeLabel::Interface,
                            "definition.enum" => NodeLabel::Enum,
                            "definition.type_alias" => NodeLabel::TypeAlias,
                            "definition.function" => NodeLabel::Function,
                            "definition.method" => NodeLabel::Method,
                            "definition.property" => NodeLabel::Property,
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

                let start = node.start_position();
                let end = node.end_position();
                let id = SmolStr::new(format!("{}::{}::{}", file_path, name_str, start.row + 1));

                let is_exported = is_node_exported(node, source);
                let is_async = is_node_async(node, source);
                let param_count = count_parameters(node, source);
                let return_type = extract_return_type(node, source);
                let visibility = extract_visibility(node, source);

                let mut props = NodeProperties::symbol(
                    name_str,
                    file_path,
                    Language::TypeScript,
                    (start.row + 1) as u32,
                    (end.row + 1) as u32,
                );
                props.is_exported = is_exported;
                props.is_async = is_async;
                if let Some(pc) = param_count {
                    props.parameter_count = Some(pc);
                }
                props.return_type = return_type.map(SmolStr::new);
                props.visibility = visibility.map(SmolStr::new);

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
                    s if s.starts_with("call") => {
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
                let caller_id = find_enclosing_definition(node, definitions, file_path)
                    .unwrap_or_else(|| SmolStr::new(format!("{}::__module__", file_path)));

                let args_count = node
                    .child_by_field_name("arguments")
                    .map(|args| args.named_child_count() as u32)
                    .unwrap_or(0);

                calls.push(ExtractedCall {
                    caller_id,
                    callee_name: SmolStr::new(name),
                    receiver: receiver.map(|r| SmolStr::new(r)),
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
            let mut module_source: Option<&str> = None;
            let mut import_node: Option<Node> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "import.source" | "reexport.source" => {
                        module_source = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "import" | "import.reexport" => {
                        import_node = Some(capture.node);
                    }
                    _ => {}
                }
            }

            if let (Some(mod_source), Some(node)) = (module_source, import_node) {
                let line = (node.start_position().row + 1) as u32;

                // Extract named imports from the import clause
                let named_imports = extract_import_specifiers(node, source);

                if named_imports.is_empty() {
                    // Default or namespace import
                    let (imported_name, is_default, is_namespace) =
                        extract_default_import(node, source);
                    imports.push(ExtractedImport {
                        source_file: SmolStr::new(file_path),
                        imported_name: SmolStr::new(imported_name),
                        module_path: SmolStr::new(mod_source),
                        alias: None,
                        is_default,
                        is_namespace,
                        line,
                    });
                } else {
                    for (name, alias) in named_imports {
                        imports.push(ExtractedImport {
                            source_file: SmolStr::new(file_path),
                            imported_name: SmolStr::new(name),
                            module_path: SmolStr::new(mod_source),
                            alias: alias.map(SmolStr::new),
                            is_default: false,
                            is_namespace: false,
                            line,
                        });
                    }
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
            let mut class_name: Option<&str> = None;
            let mut parent_name: Option<&str> = None;
            let mut kind = HeritageKind::Extends;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "heritage.class" | "heritage.child" => {
                        class_name = Some(capture.node.utf8_text(source).unwrap_or(""));
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

            if let (Some(child), Some(parent)) = (class_name, parent_name) {
                let child_id = definitions
                    .iter()
                    .find(|n| n.properties.name.as_str() == child)
                    .map(|n| n.id.clone())
                    .unwrap_or_else(|| SmolStr::new(format!("{}::{}", file_path, child)));

                heritage.push(ExtractedHeritage {
                    child_id,
                    parent_name: SmolStr::new(parent),
                    kind,
                    file_path: SmolStr::new(file_path),
                });
            }
        }

        Ok(heritage)
    }
}

impl LanguageProvider for TypeScriptProvider {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extensions(&self) -> &[&str] {
        &["ts", "tsx", "mts", "cts"]
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
            assignments: Vec::new(), // Phase 2: type resolution
        })
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a node is exported (has export_statement parent or export keyword).
fn is_node_exported(node: Node, source: &[u8]) -> bool {
    // Check if parent is export_statement
    if let Some(parent) = node.parent() {
        if parent.kind() == "export_statement" {
            return true;
        }
    }
    // Check if the node itself starts with "export"
    let text = node.utf8_text(source).unwrap_or("");
    text.starts_with("export ")
}

/// Check if a function/method is async.
fn is_node_async(node: Node, source: &[u8]) -> bool {
    let text = node.utf8_text(source).unwrap_or("");
    text.contains("async ")
}

/// Count function parameters.
fn count_parameters(node: Node, _source: &[u8]) -> Option<u32> {
    // Look for formal_parameters child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "formal_parameters" {
            return Some(child.named_child_count() as u32);
        }
    }
    // For arrow functions, check inside the value
    let mut cursor2 = node.walk();
    for child in node.children(&mut cursor2) {
        if child.kind() == "variable_declarator" {
            let mut c3 = child.walk();
            for grandchild in child.children(&mut c3) {
                if grandchild.kind() == "arrow_function"
                    || grandchild.kind() == "function_expression"
                {
                    let mut c4 = grandchild.walk();
                    for ggchild in grandchild.children(&mut c4) {
                        if ggchild.kind() == "formal_parameters" {
                            return Some(ggchild.named_child_count() as u32);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract return type annotation from a function/method.
fn extract_return_type<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_annotation" {
            // Get the type node inside the annotation
            if let Some(type_node) = child.named_child(0) {
                return type_node.utf8_text(source).ok();
            }
        }
        // For arrow/function expressions inside variable declarators
        if child.kind() == "variable_declarator" {
            let mut c2 = child.walk();
            for grandchild in child.children(&mut c2) {
                if grandchild.kind() == "arrow_function"
                    || grandchild.kind() == "function_expression"
                {
                    let mut c3 = grandchild.walk();
                    for ggchild in grandchild.children(&mut c3) {
                        if ggchild.kind() == "type_annotation" {
                            if let Some(type_node) = ggchild.named_child(0) {
                                return type_node.utf8_text(source).ok();
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract visibility modifier (public/private/protected).
fn extract_visibility<'a>(node: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            return child.utf8_text(source).ok();
        }
    }
    None
}

/// Find the enclosing definition for a call site.
fn find_enclosing_definition(
    node: Node,
    definitions: &[GraphNode],
    file_path: &str,
) -> Option<SmolStr> {
    let call_line = (node.start_position().row + 1) as u32;

    // Find the smallest definition that contains this line
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

/// Extract named import specifiers: `import { a, b as c } from "..."`
fn extract_import_specifiers<'a>(
    node: Node<'a>,
    source: &'a [u8],
) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "import_clause" {
            let mut c2 = child.walk();
            for clause_child in child.children(&mut c2) {
                if clause_child.kind() == "named_imports" {
                    let mut c3 = clause_child.walk();
                    for spec in clause_child.children(&mut c3) {
                        if spec.kind() == "import_specifier" {
                            let name = spec
                                .child_by_field_name("name")
                                .and_then(|n| n.utf8_text(source).ok())
                                .unwrap_or("");
                            let alias = spec
                                .child_by_field_name("alias")
                                .and_then(|n| n.utf8_text(source).ok())
                                .map(String::from);
                            if !name.is_empty() {
                                results.push((name.to_string(), alias));
                            }
                        }
                    }
                }
            }
        }
    }

    results
}

/// Extract default/namespace import name.
fn extract_default_import<'a>(node: Node<'a>, source: &'a [u8]) -> (String, bool, bool) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_clause" {
            let mut c2 = child.walk();
            for clause_child in child.children(&mut c2) {
                match clause_child.kind() {
                    "identifier" => {
                        let name = clause_child.utf8_text(source).unwrap_or("default");
                        return (name.to_string(), true, false);
                    }
                    "namespace_import" => {
                        if let Some(name_node) = clause_child.named_child(0) {
                            let name = name_node.utf8_text(source).unwrap_or("*");
                            return (name.to_string(), false, true);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    ("*".to_string(), false, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ts(code: &str) -> ParseResult {
        let provider = TypeScriptProvider::new();
        let config = Config::default();
        let path = Path::new("test.ts");
        provider.parse(path, code.as_bytes(), &config).unwrap()
    }

    #[test]
    fn test_extract_function() {
        let result = parse_ts("export function greet(name: string): string { return name; }");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, NodeLabel::Function);
        assert_eq!(result.nodes[0].properties.name.as_str(), "greet");
        assert!(result.nodes[0].properties.is_exported);
        assert_eq!(result.nodes[0].properties.parameter_count, Some(1));
    }

    #[test]
    fn test_extract_class() {
        let result = parse_ts(
            r#"
            class User {
                name: string;
                greet(): void {}
            }
            "#,
        );
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label).collect();
        assert!(labels.contains(&NodeLabel::Class));
        assert!(labels.contains(&NodeLabel::Method));
        assert!(labels.contains(&NodeLabel::Property));
    }

    #[test]
    fn test_extract_arrow_function() {
        let result = parse_ts("const add = (a: number, b: number) => a + b;");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, NodeLabel::Function);
        assert_eq!(result.nodes[0].properties.name.as_str(), "add");
    }

    #[test]
    fn test_extract_interface() {
        let result = parse_ts(
            r#"
            export interface UserService {
                getUser(id: string): Promise<User>;
            }
            "#,
        );
        assert!(result.nodes.iter().any(|n| n.label == NodeLabel::Interface));
    }

    #[test]
    fn test_extract_imports() {
        let result = parse_ts(
            r#"
            import { User, Role as UserRole } from './models';
            import express from 'express';
            import * as path from 'path';
            "#,
        );
        assert!(result.imports.len() >= 3);

        let user_import = result.imports.iter().find(|i| i.imported_name == "User");
        assert!(user_import.is_some());

        let role_import = result.imports.iter().find(|i| i.imported_name == "Role");
        assert!(role_import.is_some());
        assert_eq!(role_import.unwrap().alias.as_deref(), Some("UserRole"));
    }

    #[test]
    fn test_extract_calls() {
        let result = parse_ts(
            r#"
            function main() {
                console.log("hello");
                greet("world");
                const user = new User();
            }
            "#,
        );
        let call_names: Vec<_> = result
            .calls
            .iter()
            .map(|c| c.callee_name.as_str())
            .collect();
        assert!(call_names.contains(&"log"));
        assert!(call_names.contains(&"greet"));
        assert!(call_names.contains(&"User"));
    }

    #[test]
    fn test_extract_heritage() {
        let result = parse_ts(
            r#"
            class Admin extends User implements Serializable {
                serialize(): string { return ""; }
            }
            "#,
        );
        assert!(!result.heritage.is_empty());
        let extends = result
            .heritage
            .iter()
            .find(|h| h.kind == HeritageKind::Extends);
        assert!(extends.is_some());
        assert_eq!(extends.unwrap().parent_name.as_str(), "User");
    }

    #[test]
    fn test_extract_enum() {
        let result = parse_ts(
            r#"
            export enum Status {
                Active,
                Inactive,
                Pending
            }
            "#,
        );
        assert!(result.nodes.iter().any(|n| n.label == NodeLabel::Enum));
    }
}
