use gg_core::config::Config;
use gg_core::error::GgResult;
use gg_core::types::*;
use std::path::Path;

use crate::language::LanguageProvider;
use crate::typescript::TypeScriptProvider;

/// JavaScript language provider.
///
/// Reuses the TypeScript provider since tree-sitter-javascript and
/// tree-sitter-typescript share the same AST node types for all the
/// patterns we extract (functions, classes, imports, calls, heritage).
/// The only difference is the grammar used for parsing.
pub struct JavaScriptProvider {
    inner: TypeScriptProvider,
}

impl JavaScriptProvider {
    pub fn new() -> Self {
        Self {
            inner: TypeScriptProvider::new(),
        }
    }
}

impl Default for JavaScriptProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProvider for JavaScriptProvider {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn extensions(&self) -> &[&str] {
        &["js", "jsx", "mjs", "cjs"]
    }

    fn parse(&self, path: &Path, source: &[u8], config: &Config) -> GgResult<ParseResult> {
        // Parse using the JS grammar via the TS provider's parse_js method
        self.inner.parse_as_js(path, source, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_js(code: &str) -> ParseResult {
        let provider = JavaScriptProvider::new();
        let config = Config::default();
        let path = Path::new("test.js");
        provider.parse(path, code.as_bytes(), &config).unwrap()
    }

    #[test]
    fn test_js_function() {
        let result = parse_js("function greet(name) { return 'Hello ' + name; }");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].label, NodeLabel::Function);
        assert_eq!(result.nodes[0].properties.name.as_str(), "greet");
    }

    #[test]
    fn test_js_class() {
        let result = parse_js(
            r#"
            class User {
                constructor(name) {
                    this.name = name;
                }
                greet() {
                    return this.name;
                }
            }
            "#,
        );
        let labels: Vec<_> = result.nodes.iter().map(|n| n.label).collect();
        assert!(labels.contains(&NodeLabel::Class));
        assert!(labels.contains(&NodeLabel::Method));
    }

    #[test]
    fn test_js_arrow_function() {
        let result = parse_js("const add = (a, b) => a + b;");
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].properties.name.as_str(), "add");
    }

    #[test]
    fn test_js_imports() {
        let result = parse_js(
            r#"
            const express = require('express');
            import { Router } from 'express';
            "#,
        );
        // import statement should be captured
        assert!(!result.imports.is_empty());
    }

    #[test]
    fn test_js_calls() {
        let result = parse_js(
            r#"
            function main() {
                console.log("hello");
                fetch("/api/data");
            }
            "#,
        );
        assert!(result.calls.len() >= 2);
    }
}
