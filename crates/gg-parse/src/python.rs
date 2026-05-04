use gg_core::config::Config;
use gg_core::error::{GgError, GgResult};
use gg_core::types::*;
use smol_str::SmolStr;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};
use xxhash_rust::xxh3::xxh3_64;

use crate::language::LanguageProvider;

/// Tree-sitter query for Python definitions.
const DEFINITIONS_QUERY: &str = r#"
(class_definition
  name: (identifier) @name) @definition.class

(function_definition
  name: (identifier) @name) @definition.function

(expression_statement
  (assignment
    left: (identifier) @name
    type: (type)) @definition.property)
"#;

/// Tree-sitter query for Python calls.
const CALLS_QUERY: &str = r#"
; Simple call: foo()
(call
  function: (identifier) @call.name) @call

; Method call: obj.method()
(call
  function: (attribute
    object: (_) @call.receiver
    attribute: (identifier) @call.name)) @call.method
"#;

/// Tree-sitter query for Python imports.
const IMPORTS_QUERY: &str = r#"
; import module
(import_statement
  name: (dotted_name) @import.source) @import

; import module as alias
(import_statement
  name: (aliased_import
    name: (dotted_name) @import.source)) @import.alias

; from module import name
(import_from_statement
  module_name: (dotted_name) @import.source) @import.from

; from .relative import name
(import_from_statement
  module_name: (relative_import) @import.source) @import.relative
"#;

/// Tree-sitter query for Python heritage (class inheritance).
const HERITAGE_QUERY: &str = r#"
(class_definition
  name: (identifier) @heritage.class
  superclasses: (argument_list
    (identifier) @heritage.extends))
"#;

pub struct PythonProvider {
    language: tree_sitter::Language,
}

impl PythonProvider {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_python::LANGUAGE.into(),
        }
    }

    fn parse_tree(&self, path: &Path, source: &[u8]) -> GgResult<tree_sitter::Tree> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| GgError::Parse {
                file: path.display().to_string(),
                message: format!("Failed to set Python language: {e}"),
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
                            "definition.function" => NodeLabel::Function,
                            "definition.property" => NodeLabel::Property,
                            _ => NodeLabel::CodeElement,
                        };
                    }
                    _ => {}
                }
            }

            if let (Some(name_str), Some(node)) = (name, def_node) {
                if name_str.is_empty() || name_str.starts_with('_') && name_str != "__init__" {
                    continue;
                }

                let start = node.start_position();
                let end = node.end_position();
                let id = SmolStr::new(format!("{}::{}::{}", file_path, name_str, start.row + 1));

                // Detect if this is a method (inside a class)
                let actual_label = if label == NodeLabel::Function {
                    if is_inside_class(node) {
                        NodeLabel::Method
                    } else {
                        NodeLabel::Function
                    }
                } else {
                    label
                };

                let is_exported = !name_str.starts_with('_');
                let is_async = is_async_def(node, source);
                let param_count = count_python_params(node, source);
                let decorators = extract_decorators(node, source);
                let is_static = decorators.iter().any(|d| d == "staticmethod");
                let container_name = enclosing_class_name(node, source);
                let body_hash = hash_node_body(node, source);

                let mut props = NodeProperties::symbol(
                    name_str,
                    file_path,
                    Language::Python,
                    (start.row + 1) as u32,
                    (end.row + 1) as u32,
                );
                props.is_exported = is_exported;
                props.is_async = is_async;
                props.is_static = is_static;
                if let Some(pc) = param_count {
                    props.parameter_count = Some(pc);
                }
                if !decorators.is_empty() {
                    props.annotations = Some(decorators.into_iter().map(SmolStr::new).collect());
                }
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
                let caller_id = find_enclosing(node, definitions, file_path)
                    .unwrap_or_else(|| SmolStr::new(format!("{}::__module__", file_path)));

                calls.push(ExtractedCall {
                    caller_id,
                    callee_name: SmolStr::new(name),
                    receiver: receiver.map(SmolStr::new),
                    file_path: SmolStr::new(file_path),
                    line,
                    arguments_count: node
                        .child_by_field_name("arguments")
                        .map(|a| a.named_child_count() as u32)
                        .unwrap_or(0),
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
            message: format!("Import query error: {e}"),
        })?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let mut imports = Vec::new();

        while let Some(m) = matches.next() {
            let mut module_source: Option<&str> = None;
            let mut import_node: Option<Node> = None;
            let mut is_from = false;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "import.source" => {
                        module_source = Some(capture.node.utf8_text(source).unwrap_or(""));
                        import_node = Some(capture.node);
                    }
                    "import" | "import.alias" | "import.from" | "import.relative" => {
                        if import_node.is_none() {
                            import_node = Some(capture.node);
                        }
                        if capture_name == &"import.from" || capture_name == &"import.relative" {
                            is_from = true;
                        }
                    }
                    _ => {}
                }
            }

            if let (Some(mod_source), Some(node)) = (module_source, import_node) {
                let line = (node.start_position().row + 1) as u32;
                // Convert Python module path: os.path → os/path
                let module_path = mod_source.replace('.', "/");

                if is_from {
                    // from x import a, b, c — extract named imports from parent
                    let parent = node.parent().unwrap_or(node);
                    let named = extract_python_from_names(parent, source);
                    if named.is_empty() {
                        imports.push(ExtractedImport {
                            source_file: SmolStr::new(file_path),
                            imported_name: SmolStr::new(mod_source),
                            module_path: SmolStr::new(&module_path),
                            alias: None,
                            is_default: false,
                            is_namespace: true,
                            line,
                        });
                    } else {
                        for (name, alias) in named {
                            imports.push(ExtractedImport {
                                source_file: SmolStr::new(file_path),
                                imported_name: SmolStr::new(&name),
                                module_path: SmolStr::new(&module_path),
                                alias: alias.map(SmolStr::new),
                                is_default: false,
                                is_namespace: false,
                                line,
                            });
                        }
                    }
                } else {
                    // import x or import x as y
                    imports.push(ExtractedImport {
                        source_file: SmolStr::new(file_path),
                        imported_name: SmolStr::new(mod_source),
                        module_path: SmolStr::new(&module_path),
                        alias: None,
                        is_default: false,
                        is_namespace: true,
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
            let mut class_name: Option<&str> = None;
            let mut parent_name: Option<&str> = None;

            for capture in m.captures {
                let capture_name = &query.capture_names()[capture.index as usize];
                match &**capture_name {
                    "heritage.class" => {
                        class_name = Some(capture.node.utf8_text(source).unwrap_or(""));
                    }
                    "heritage.extends" => {
                        parent_name = Some(capture.node.utf8_text(source).unwrap_or(""));
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
                    kind: HeritageKind::Extends,
                    file_path: SmolStr::new(file_path),
                });
            }
        }

        Ok(heritage)
    }
}

impl Default for PythonProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for PythonProvider {
    fn language(&self) -> Language {
        Language::Python
    }

    fn extensions(&self) -> &[&str] {
        &["py", "pyi"]
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_node_body(node: Node, source: &[u8]) -> Option<u64> {
    node.utf8_text(source)
        .ok()
        .map(|text| xxh3_64(text.as_bytes()))
}

fn enclosing_class_name(node: Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_definition" {
            if let Some(name_node) = parent.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source) {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        current = parent.parent();
    }
    None
}

/// Check if a function_definition is inside a class_definition.
fn is_inside_class(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_definition" {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Check if a function is async def.
fn is_async_def(node: Node, source: &[u8]) -> bool {
    let text = node.utf8_text(source).unwrap_or("");
    text.starts_with("async ")
}

/// Count parameters (excluding self/cls).
fn count_python_params(node: Node, _source: &[u8]) -> Option<u32> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "parameters" {
            let count = child.named_child_count() as u32;
            // Subtract 1 for self/cls in methods
            return Some(if is_inside_class(node) && count > 0 {
                count - 1
            } else {
                count
            });
        }
    }
    // Check inside function_definition for async
    let mut cursor2 = node.walk();
    for child in node.children(&mut cursor2) {
        if child.kind() == "function_definition" {
            return count_python_params(child, _source);
        }
    }
    None
}

/// Extract decorator names from a decorated definition.
fn extract_decorators(node: Node, source: &[u8]) -> Vec<String> {
    let mut decorators = Vec::new();
    if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            let mut cursor = parent.walk();
            for child in parent.children(&mut cursor) {
                if child.kind() == "decorator" {
                    if let Ok(text) = child.utf8_text(source) {
                        let name = text
                            .trim_start_matches('@')
                            .split('(')
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !name.is_empty() {
                            decorators.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    decorators
}

/// Extract named imports from `from x import a, b as c`.
fn extract_python_from_names(node: Node, source: &[u8]) -> Vec<(String, Option<String>)> {
    let mut results = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                // Simple name: `from x import foo`
                if let Ok(name) = child.utf8_text(source) {
                    // Skip the module name (first dotted_name is the module)
                    // Named imports come after "import" keyword
                    let child_start = child.start_byte();
                    let import_kw = node.utf8_text(source).unwrap_or("");
                    if let Some(import_pos) = import_kw.find("import") {
                        let import_byte = node.start_byte() + import_pos + 6;
                        if child_start > import_byte {
                            results.push((name.to_string(), None));
                        }
                    }
                }
            }
            "aliased_import" => {
                let name = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("");
                let alias = child
                    .child_by_field_name("alias")
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(String::from);
                if !name.is_empty() {
                    results.push((name.to_string(), alias));
                }
            }
            _ => {}
        }
    }
    results
}

/// Find the enclosing definition for a call site.
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

    fn parse_py(code: &str) -> ParseResult {
        let provider = PythonProvider::new();
        let config = Config::default();
        let path = Path::new("test.py");
        provider.parse(path, code.as_bytes(), &config).unwrap()
    }

    #[test]
    fn test_function() {
        let result = parse_py("def greet(name):\n    return f'Hello {name}'");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, NodeLabel::Function);
        assert_eq!(result.nodes[0].properties.name.as_str(), "greet");
        assert_eq!(result.nodes[0].properties.parameter_count, Some(1));
    }

    #[test]
    fn test_class_with_methods() {
        let result = parse_py(
            "class User:\n    def __init__(self, name):\n        self.name = name\n    def greet(self):\n        return self.name\n",
        );
        let labels: Vec<_> = result
            .nodes
            .iter()
            .map(|n| (n.label, n.properties.name.as_str()))
            .collect();
        assert!(labels.contains(&(NodeLabel::Class, "User")));
        assert!(labels.contains(&(NodeLabel::Method, "__init__")));
        assert!(labels.contains(&(NodeLabel::Method, "greet")));
        // __init__ should have 1 param (name), not 2 (self excluded)
        let init = result
            .nodes
            .iter()
            .find(|n| n.properties.name == "__init__")
            .unwrap();
        assert_eq!(init.properties.parameter_count, Some(1));
    }

    #[test]
    fn test_imports() {
        let result = parse_py(
            "import os\nimport numpy as np\nfrom pathlib import Path\nfrom .models import User\n",
        );
        assert!(result.imports.len() >= 3);
        let os_import = result.imports.iter().find(|i| i.imported_name == "os");
        assert!(os_import.is_some());
    }

    #[test]
    fn test_calls() {
        let result =
            parse_py("def main():\n    print('hello')\n    os.path.join('a', 'b')\n    User()\n");
        let names: Vec<_> = result
            .calls
            .iter()
            .map(|c| c.callee_name.as_str())
            .collect();
        assert!(names.contains(&"print"));
        assert!(names.contains(&"User"));
    }

    #[test]
    fn test_heritage() {
        let result =
            parse_py("class Admin(User):\n    def __init__(self):\n        super().__init__()\n");
        assert!(!result.heritage.is_empty());
        assert_eq!(result.heritage[0].parent_name.as_str(), "User");
        assert_eq!(result.heritage[0].kind, HeritageKind::Extends);
    }

    #[test]
    fn test_async_function() {
        let result = parse_py("async def fetch_data(url):\n    pass\n");
        assert_eq!(result.nodes.len(), 1);
        assert!(result.nodes[0].properties.is_async);
    }

    #[test]
    fn test_decorated_function() {
        let result = parse_py("@app.route('/api/users')\ndef get_users():\n    return []\n");
        assert_eq!(result.nodes.len(), 1);
        let node = &result.nodes[0];
        assert!(node
            .properties
            .annotations
            .as_ref()
            .is_some_and(|a| !a.is_empty()));
    }
}
