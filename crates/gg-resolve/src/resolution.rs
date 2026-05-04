use crate::symbol_table::{SymbolDefinition, SymbolTable};
use smol_str::SmolStr;
use std::collections::{HashMap, HashSet};

pub const CONFIDENCE_SAME_FILE: f64 = 0.95;
pub const CONFIDENCE_IMPORT_SCOPED: f64 = 0.9;
pub const CONFIDENCE_GLOBAL: f64 = 0.5;
pub const CONFIDENCE_INTERFACE_DISPATCH: f64 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionTier {
    SameFile,
    ImportScoped,
    Global,
}

impl ResolutionTier {
    pub fn confidence(&self) -> f64 {
        match self {
            Self::SameFile => CONFIDENCE_SAME_FILE,
            Self::ImportScoped => CONFIDENCE_IMPORT_SCOPED,
            Self::Global => CONFIDENCE_GLOBAL,
        }
    }
}

/// A resolved symbol candidate with its confidence tier.
#[derive(Debug, Clone)]
pub struct ResolvedCandidate {
    pub definition: SymbolDefinition,
    pub tier: ResolutionTier,
    pub confidence: f64,
}

/// Result of a tiered resolution lookup.
#[derive(Debug, Clone)]
pub struct TieredResult {
    pub candidates: Vec<ResolvedCandidate>,
    pub tier: ResolutionTier,
}

impl TieredResult {
    /// Get the best (highest confidence) candidate.
    pub fn best(&self) -> Option<&ResolvedCandidate> {
        self.candidates.first()
    }

    /// Whether this result is unambiguous (exactly one candidate).
    pub fn is_unique(&self) -> bool {
        self.candidates.len() == 1
    }
}

/// A named import binding: `import { User as U } from './models'`
/// Maps local name "U" → { source: "src/models.ts", exported: "User" }
#[derive(Debug, Clone)]
pub struct NamedImportBinding {
    pub source_path: SmolStr,
    pub exported_name: SmolStr,
}

/// The resolution context holds all cross-file linking state.
pub struct ResolutionContext {
    /// file_path → set of files it imports from
    import_map: HashMap<SmolStr, HashSet<SmolStr>>,

    /// file_path → (local_name → binding)
    named_import_map: HashMap<SmolStr, HashMap<SmolStr, NamedImportBinding>>,

    /// file_path → (module_alias → source_file)
    /// Python-specific: `import models` → module_alias_map['app.py']['models'] = 'models.py'
    module_alias_map: HashMap<SmolStr, HashMap<SmolStr, SmolStr>>,

    /// Per-file resolution cache
    cache: HashMap<SmolStr, HashMap<SmolStr, Option<TieredResult>>>,
}

impl ResolutionContext {
    pub fn new() -> Self {
        Self {
            import_map: HashMap::new(),
            named_import_map: HashMap::new(),
            module_alias_map: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Population (called during import processing)
    // -----------------------------------------------------------------------

    /// Record that `from_file` imports from `target_file`.
    pub fn add_import_edge(&mut self, from_file: SmolStr, target_file: SmolStr) {
        self.import_map
            .entry(from_file)
            .or_default()
            .insert(target_file);
    }

    /// Record a named import binding.
    pub fn add_named_binding(
        &mut self,
        file_path: SmolStr,
        local_name: SmolStr,
        source_path: SmolStr,
        exported_name: SmolStr,
    ) {
        self.named_import_map.entry(file_path).or_default().insert(
            local_name,
            NamedImportBinding {
                source_path,
                exported_name,
            },
        );
    }

    /// Record a module alias (Python `import module` style).
    pub fn add_module_alias(&mut self, file_path: SmolStr, alias: SmolStr, source_path: SmolStr) {
        self.module_alias_map
            .entry(file_path)
            .or_default()
            .insert(alias, source_path);
    }

    // -----------------------------------------------------------------------
    // Resolution (the core tiered algorithm)
    // -----------------------------------------------------------------------

    /// Resolve a symbol name from a given file, using tiered lookup.
    ///
    /// Resolution order:
    /// 1. Same-file (confidence 0.95)
    /// 2. Named binding chain (confidence 0.9)
    /// 3. Import-scoped (confidence 0.9)
    /// 4. Global (confidence 0.5)
    pub fn resolve(
        &mut self,
        name: &str,
        from_file: &str,
        symbols: &SymbolTable,
    ) -> Option<TieredResult> {
        // Check cache
        let file_key = SmolStr::new(from_file);
        let name_key = SmolStr::new(name);
        if let Some(file_cache) = self.cache.get(&file_key) {
            if let Some(cached) = file_cache.get(&name_key) {
                return cached.clone();
            }
        }

        let result = self.resolve_uncached(name, from_file, symbols);

        // Cache
        self.cache
            .entry(file_key)
            .or_default()
            .insert(name_key, result.clone());

        result
    }

    fn resolve_uncached(
        &self,
        name: &str,
        from_file: &str,
        symbols: &SymbolTable,
    ) -> Option<TieredResult> {
        // Tier 1: Same-file
        let same_file_defs = symbols.lookup_in_file(from_file, name);
        if !same_file_defs.is_empty() {
            return Some(TieredResult {
                candidates: same_file_defs
                    .into_iter()
                    .map(|d| ResolvedCandidate {
                        definition: d,
                        tier: ResolutionTier::SameFile,
                        confidence: CONFIDENCE_SAME_FILE,
                    })
                    .collect(),
                tier: ResolutionTier::SameFile,
            });
        }

        // Tier 2a-named: Walk binding chain through named imports
        if let Some(result) = self.resolve_via_named_binding(name, from_file, symbols) {
            return Some(result);
        }

        // Get all global definitions for this name
        let all_defs = symbols.lookup_global(name);
        if all_defs.is_empty() {
            return None;
        }

        // Tier 2a: Filter by import map
        if let Some(imported_files) = self.import_map.get(from_file) {
            let import_scoped: Vec<_> = all_defs
                .iter()
                .filter(|d| imported_files.contains(&d.file_path))
                .cloned()
                .collect();

            if !import_scoped.is_empty() {
                return Some(TieredResult {
                    candidates: import_scoped
                        .into_iter()
                        .map(|d| ResolvedCandidate {
                            definition: d,
                            tier: ResolutionTier::ImportScoped,
                            confidence: CONFIDENCE_IMPORT_SCOPED,
                        })
                        .collect(),
                    tier: ResolutionTier::ImportScoped,
                });
            }
        }

        // Tier 2a via module alias: check if receiver is a module alias
        if let Some(aliases) = self.module_alias_map.get(from_file) {
            // Check if `name` is `alias.something` — but for plain name lookup,
            // check if the name resolves in the aliased module's file
            for (alias, source_path) in aliases {
                if alias.as_str() == name {
                    // The name IS the module alias — look up default export
                    let module_defs = symbols.lookup_in_file(source_path, name);
                    if !module_defs.is_empty() {
                        return Some(TieredResult {
                            candidates: module_defs
                                .into_iter()
                                .map(|d| ResolvedCandidate {
                                    definition: d,
                                    tier: ResolutionTier::ImportScoped,
                                    confidence: CONFIDENCE_IMPORT_SCOPED,
                                })
                                .collect(),
                            tier: ResolutionTier::ImportScoped,
                        });
                    }
                }
            }
        }

        // Tier 3: Global (all defs, consumers must handle ambiguity)
        Some(TieredResult {
            candidates: all_defs
                .into_iter()
                .map(|d| ResolvedCandidate {
                    definition: d,
                    tier: ResolutionTier::Global,
                    confidence: CONFIDENCE_GLOBAL,
                })
                .collect(),
            tier: ResolutionTier::Global,
        })
    }

    /// Tier 2a-named: Walk the named import binding chain.
    ///
    /// Example: file A imports `User` from file B which re-exports from file C.
    /// This follows the chain: A → B → C to find the actual definition.
    fn resolve_via_named_binding(
        &self,
        name: &str,
        from_file: &str,
        symbols: &SymbolTable,
    ) -> Option<TieredResult> {
        let bindings = self.named_import_map.get(from_file)?;
        let binding = bindings.get(name)?;

        // Direct lookup in the source file
        let defs = symbols.lookup_in_file(&binding.source_path, &binding.exported_name);
        if !defs.is_empty() {
            return Some(TieredResult {
                candidates: defs
                    .into_iter()
                    .map(|d| ResolvedCandidate {
                        definition: d,
                        tier: ResolutionTier::ImportScoped,
                        confidence: CONFIDENCE_IMPORT_SCOPED,
                    })
                    .collect(),
                tier: ResolutionTier::ImportScoped,
            });
        }

        // Walk the chain: the source file might re-export from another file
        // Limit chain depth to prevent infinite loops
        let mut current_file = binding.source_path.clone();
        let mut current_name = binding.exported_name.clone();
        let mut depth = 0;
        const MAX_CHAIN_DEPTH: usize = 10;

        while depth < MAX_CHAIN_DEPTH {
            if let Some(next_bindings) = self.named_import_map.get(&current_file) {
                if let Some(next_binding) = next_bindings.get(&current_name) {
                    let next_defs = symbols
                        .lookup_in_file(&next_binding.source_path, &next_binding.exported_name);
                    if !next_defs.is_empty() {
                        return Some(TieredResult {
                            candidates: next_defs
                                .into_iter()
                                .map(|d| ResolvedCandidate {
                                    definition: d,
                                    tier: ResolutionTier::ImportScoped,
                                    confidence: CONFIDENCE_IMPORT_SCOPED,
                                })
                                .collect(),
                            tier: ResolutionTier::ImportScoped,
                        });
                    }
                    current_file = next_binding.source_path.clone();
                    current_name = next_binding.exported_name.clone();
                    depth += 1;
                    continue;
                }
            }
            break;
        }

        None
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Get the named import bindings for a file.
    pub fn named_bindings(&self, file_path: &str) -> Option<&HashMap<SmolStr, NamedImportBinding>> {
        self.named_import_map.get(file_path)
    }

    /// Get the import map entry for a file.
    pub fn imports_of(&self, file_path: &str) -> Option<&HashSet<SmolStr>> {
        self.import_map.get(file_path)
    }

    /// Clear resolution caches (call after modifying import/symbol state).
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get stats for debugging.
    pub fn stats(&self) -> ResolutionStats {
        ResolutionStats {
            import_edges: self.import_map.values().map(|s| s.len()).sum(),
            named_bindings: self.named_import_map.values().map(|m| m.len()).sum(),
            module_aliases: self.module_alias_map.values().map(|m| m.len()).sum(),
            cached_files: self.cache.len(),
        }
    }
}

impl Default for ResolutionContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ResolutionStats {
    pub import_edges: usize,
    pub named_bindings: usize,
    pub module_aliases: usize,
    pub cached_files: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gg_core::types::{Language, NodeLabel};

    fn make_def(name: &str, file: &str, label: NodeLabel) -> SymbolDefinition {
        SymbolDefinition {
            node_id: SmolStr::new(format!("{file}::{name}::1")),
            name: SmolStr::new(name),
            label,
            file_path: SmolStr::new(file),
            language: Language::TypeScript,
            is_exported: true,
            return_type: None,
            start_line: Some(1),
            end_line: Some(10),
        }
    }

    #[test]
    fn test_same_file_resolution() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("User", "src/models.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();
        let result = ctx.resolve("User", "src/models.ts", &symbols).unwrap();

        assert_eq!(result.tier, ResolutionTier::SameFile);
        assert_eq!(result.candidates.len(), 1);
        assert!((result.candidates[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_import_scoped_resolution() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("User", "src/models.ts", NodeLabel::Class));
        symbols.insert(make_def("User", "lib/models.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();
        ctx.add_import_edge(SmolStr::new("src/app.ts"), SmolStr::new("src/models.ts"));

        let result = ctx.resolve("User", "src/app.ts", &symbols).unwrap();
        assert_eq!(result.tier, ResolutionTier::ImportScoped);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(
            result.candidates[0].definition.file_path.as_str(),
            "src/models.ts"
        );
    }

    #[test]
    fn test_named_binding_resolution() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("User", "src/models.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();
        ctx.add_named_binding(
            SmolStr::new("src/app.ts"),
            SmolStr::new("U"),             // local name (aliased)
            SmolStr::new("src/models.ts"), // source file
            SmolStr::new("User"),          // exported name
        );

        let result = ctx.resolve("U", "src/app.ts", &symbols).unwrap();
        assert_eq!(result.tier, ResolutionTier::ImportScoped);
        assert_eq!(result.candidates[0].definition.name.as_str(), "User");
    }

    #[test]
    fn test_binding_chain_resolution() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("User", "src/models/user.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();
        // app.ts imports User from barrel (index.ts), which re-exports from user.ts
        ctx.add_named_binding(
            SmolStr::new("src/app.ts"),
            SmolStr::new("User"),
            SmolStr::new("src/models/index.ts"),
            SmolStr::new("User"),
        );
        ctx.add_named_binding(
            SmolStr::new("src/models/index.ts"),
            SmolStr::new("User"),
            SmolStr::new("src/models/user.ts"),
            SmolStr::new("User"),
        );

        let result = ctx.resolve("User", "src/app.ts", &symbols).unwrap();
        assert_eq!(result.tier, ResolutionTier::ImportScoped);
        assert_eq!(
            result.candidates[0].definition.file_path.as_str(),
            "src/models/user.ts"
        );
    }

    #[test]
    fn test_global_fallback() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("User", "src/models.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();
        // No import edge from app.ts to models.ts
        let result = ctx.resolve("User", "src/app.ts", &symbols).unwrap();
        assert_eq!(result.tier, ResolutionTier::Global);
        assert!((result.candidates[0].confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unresolvable() {
        let symbols = SymbolTable::new();
        let mut ctx = ResolutionContext::new();
        assert!(ctx.resolve("Nonexistent", "src/app.ts", &symbols).is_none());
    }
}
