use dashmap::DashMap;
use gg_core::types::{Language, NodeId, NodeLabel};
use smol_str::SmolStr;
use std::collections::HashMap;

/// A symbol definition extracted from source code.
#[derive(Debug, Clone)]
pub struct SymbolDefinition {
    pub node_id: NodeId,
    pub name: SmolStr,
    pub label: NodeLabel,
    pub file_path: SmolStr,
    pub language: Language,
    pub is_exported: bool,
    pub return_type: Option<SmolStr>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

/// Concurrent symbol table for parallel population during parsing.
///
/// Uses DashMap for lock-free concurrent reads and writes
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// file_path -> name -> definitions
    by_file: DashMap<SmolStr, HashMap<SmolStr, Vec<SymbolDefinition>>>,
    /// name -> definitions (for cross-file lookup)
    by_name: DashMap<SmolStr, Vec<SymbolDefinition>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a symbol definition. Thread-safe.
    pub fn insert(&self, def: SymbolDefinition) {
        // Insert into by_file index
        self.by_file
            .entry(def.file_path.clone())
            .or_default()
            .entry(def.name.clone())
            .or_default()
            .push(def.clone());

        // Insert into by_name index
        self.by_name.entry(def.name.clone()).or_default().push(def);
    }

    /// Look up symbols by name within a specific file.
    pub fn lookup_in_file(&self, file_path: &str, name: &str) -> Vec<SymbolDefinition> {
        self.by_file
            .get(file_path)
            .and_then(|file_syms| file_syms.get(name).cloned())
            .unwrap_or_default()
    }

    /// Look up symbols by name across all files.
    pub fn lookup_global(&self, name: &str) -> Vec<SymbolDefinition> {
        self.by_name
            .get(name)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Look up exported symbols by name.
    pub fn lookup_exported(&self, name: &str) -> Vec<SymbolDefinition> {
        self.lookup_global(name)
            .into_iter()
            .filter(|d| d.is_exported)
            .collect()
    }

    /// Get all symbols defined in a file.
    pub fn symbols_in_file(&self, file_path: &str) -> Vec<SymbolDefinition> {
        self.by_file
            .get(file_path)
            .map(|file_syms| file_syms.values().flatten().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove all symbols for a file (used in incremental re-indexing).
    pub fn remove_file(&self, file_path: &str) {
        if let Some((_, file_syms)) = self.by_file.remove(file_path) {
            for name in file_syms.keys() {
                if let Some(mut global) = self.by_name.get_mut(name) {
                    global.retain(|d| d.file_path != file_path);
                }
            }
        }
    }

    pub fn total_symbols(&self) -> usize {
        self.by_name.iter().map(|e| e.value().len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_def(name: &str, file: &str, exported: bool) -> SymbolDefinition {
        SymbolDefinition {
            node_id: SmolStr::new(format!("{file}::{name}")),
            name: SmolStr::new(name),
            label: NodeLabel::Function,
            file_path: SmolStr::new(file),
            language: Language::TypeScript,
            is_exported: exported,
            return_type: None,
            start_line: Some(1),
            end_line: Some(10),
        }
    }

    #[test]
    fn test_insert_and_lookup() {
        let table = SymbolTable::new();
        table.insert(make_def("foo", "src/a.ts", true));
        table.insert(make_def("bar", "src/a.ts", false));
        table.insert(make_def("foo", "src/b.ts", true));

        assert_eq!(table.lookup_in_file("src/a.ts", "foo").len(), 1);
        assert_eq!(table.lookup_global("foo").len(), 2);
        assert_eq!(table.lookup_exported("bar").len(), 0);
        assert_eq!(table.total_symbols(), 3);
    }

    #[test]
    fn test_remove_file() {
        let table = SymbolTable::new();
        table.insert(make_def("foo", "src/a.ts", true));
        table.insert(make_def("foo", "src/b.ts", true));

        table.remove_file("src/a.ts");
        assert_eq!(table.lookup_global("foo").len(), 1);
        assert!(table.lookup_in_file("src/a.ts", "foo").is_empty());
    }
}
