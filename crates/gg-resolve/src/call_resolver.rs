use crate::resolution::{
    ResolutionContext, ResolutionTier, CONFIDENCE_IMPORT_SCOPED, CONFIDENCE_INTERFACE_DISPATCH,
};
use crate::symbol_table::SymbolTable;
use gg_core::types::*;
use smol_str::SmolStr;
use std::collections::{HashMap, HashSet};

/// A resolved call target with confidence.
#[derive(Debug, Clone)]
pub struct ResolvedCallTarget {
    pub target_node_id: SmolStr,
    pub confidence: f64,
    pub reason: SmolStr,
}

/// The call resolver: resolves function/method calls to their target definitions.
///
/// Handles:
/// - Same-file calls (direct lookup)
/// - Cross-file calls via import resolution
/// - Method calls with receiver-type filtering
/// - Constructor calls (`new Foo()`)
/// - Interface dispatch (call through interface → all implementations)
pub struct CallResolver;

impl CallResolver {
    /// Resolve all calls in a file to their target definitions.
    ///
    /// Returns a list of (caller_id, target_id, confidence, reason) tuples
    /// that should become CALLS edges in the graph.
    pub fn resolve_calls(
        calls: &[ExtractedCall],
        file_path: &str,
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
        implementor_map: &HashMap<SmolStr, HashSet<SmolStr>>,
    ) -> Vec<ResolvedCallTarget> {
        let mut results = Vec::new();

        for call in calls {
            let targets = Self::resolve_single_call(call, file_path, ctx, symbols, implementor_map);
            results.extend(targets);
        }

        results
    }

    fn resolve_single_call(
        call: &ExtractedCall,
        file_path: &str,
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
        implementor_map: &HashMap<SmolStr, HashSet<SmolStr>>,
    ) -> Vec<ResolvedCallTarget> {
        let callee_name = &call.callee_name;

        // For method calls with a receiver (obj.method()), try receiver-type resolution
        if let Some(ref receiver) = call.receiver {
            return Self::resolve_member_call(
                callee_name,
                receiver,
                file_path,
                ctx,
                symbols,
                implementor_map,
            );
        }

        // Plain function call or constructor
        Self::resolve_plain_call(callee_name, file_path, ctx, symbols)
    }

    /// Resolve a plain function call: `foo()` or `new Foo()`
    fn resolve_plain_call(
        name: &str,
        from_file: &str,
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
    ) -> Vec<ResolvedCallTarget> {
        let resolved = match ctx.resolve(name, from_file, symbols) {
            Some(r) => r,
            None => return Vec::new(),
        };

        // For global tier with multiple candidates, refuse (ambiguous)
        if resolved.tier == ResolutionTier::Global && resolved.candidates.len() > 1 {
            return Vec::new();
        }

        resolved
            .candidates
            .into_iter()
            .map(|c| {
                let reason = match c.tier {
                    ResolutionTier::SameFile => "same-file call",
                    ResolutionTier::ImportScoped => "import-scoped call",
                    ResolutionTier::Global => "global call",
                };
                ResolvedCallTarget {
                    target_node_id: c.definition.node_id,
                    confidence: c.confidence,
                    reason: SmolStr::new(reason),
                }
            })
            .collect()
    }

    /// Resolve a member call: `obj.method()`
    ///
    /// Strategy:
    /// 1. Determine the type of `obj` (via named imports, constructor bindings, type annotations)
    /// 2. Find `method` on that type
    /// 3. If the type is an interface, also find implementations
    fn resolve_member_call(
        method_name: &str,
        receiver_name: &str,
        from_file: &str,
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
        implementor_map: &HashMap<SmolStr, HashSet<SmolStr>>,
    ) -> Vec<ResolvedCallTarget> {
        let mut targets = Vec::new();

        // Strategy 1: Receiver is a known imported name → look up its type
        let receiver_type = Self::infer_receiver_type(receiver_name, from_file, ctx, symbols);

        if let Some(ref type_name) = receiver_type {
            // Find the method on the resolved type
            let type_methods = Self::find_methods_on_type(type_name, method_name, symbols);

            for (node_id, confidence) in &type_methods {
                targets.push(ResolvedCallTarget {
                    target_node_id: node_id.clone(),
                    confidence: *confidence,
                    reason: SmolStr::new("receiver-typed method call"),
                });
            }

            // Interface dispatch: if the type is an interface, find implementations
            if let Some(impl_files) = implementor_map.get(type_name.as_str()) {
                for impl_file in impl_files {
                    let impl_methods = symbols.lookup_in_file(impl_file, method_name);
                    for def in impl_methods {
                        // Avoid duplicates
                        if !targets.iter().any(|t| t.target_node_id == def.node_id) {
                            targets.push(ResolvedCallTarget {
                                target_node_id: def.node_id.clone(),
                                confidence: CONFIDENCE_INTERFACE_DISPATCH,
                                reason: SmolStr::new("interface-dispatch"),
                            });
                        }
                    }
                }
            }
        }

        // Strategy 2: Fall back to resolving the method name globally
        if targets.is_empty() {
            // Try resolving method_name directly (might match a standalone function)
            let resolved = ctx.resolve(method_name, from_file, symbols);
            if let Some(r) = resolved {
                if r.is_unique() || r.tier != ResolutionTier::Global {
                    for c in r.candidates {
                        targets.push(ResolvedCallTarget {
                            target_node_id: c.definition.node_id,
                            confidence: c.confidence * 0.8, // Discount for untyped resolution
                            reason: SmolStr::new("untyped method call"),
                        });
                    }
                }
            }
        }

        targets
    }

    /// Infer the type of a receiver variable.
    ///
    /// Checks:
    /// 1. Named import bindings (most reliable)
    /// 2. Module alias map
    fn infer_receiver_type(
        receiver_name: &str,
        from_file: &str,
        ctx: &ResolutionContext,
        symbols: &SymbolTable,
    ) -> Option<SmolStr> {
        // Check named import: if `receiver_name` was imported, get its type
        if let Some(bindings) = ctx.named_bindings(from_file) {
            if let Some(binding) = bindings.get(receiver_name) {
                // The imported name IS a type (class/interface)
                let defs = symbols.lookup_in_file(&binding.source_path, &binding.exported_name);
                for def in &defs {
                    if def.label.is_type_def() {
                        return Some(def.name.clone());
                    }
                }
                // If it's an instance, check the declared type
                for def in &defs {
                    if let Some(ref dt) = def.return_type {
                        return Some(dt.clone());
                    }
                }
            }
        }

        // Check same-file definitions for type annotation
        let local_defs = symbols.lookup_in_file(from_file, receiver_name);
        for def in &local_defs {
            if let Some(ref dt) = def.return_type {
                return Some(dt.clone());
            }
        }

        None
    }

    /// Find methods with a given name on a type (class/interface/struct).
    fn find_methods_on_type(
        type_name: &str,
        method_name: &str,
        symbols: &SymbolTable,
    ) -> Vec<(SmolStr, f64)> {
        let mut results = Vec::new();

        // Find all definitions of the type
        let type_defs = symbols.lookup_exported(type_name);

        for type_def in &type_defs {
            // Find methods in the same file that might belong to this type
            let file_symbols = symbols.symbols_in_file(&type_def.file_path);
            for sym in &file_symbols {
                if sym.name.as_str() == method_name
                    && matches!(sym.label, NodeLabel::Method | NodeLabel::Function)
                {
                    // Check if this method is within the type's line range
                    if let (Some(type_start), Some(type_end), Some(method_start)) =
                        (type_def.start_line, type_def.end_line, sym.start_line)
                    {
                        if method_start >= type_start && method_start <= type_end {
                            results.push((sym.node_id.clone(), CONFIDENCE_IMPORT_SCOPED));
                        }
                    }
                }
            }
        }

        results
    }
}

/// Build the implementor map from heritage data.
///
/// Maps interface_name → set of files that implement it.
pub fn build_implementor_map(
    heritage: &[(SmolStr, ExtractedHeritage)],
) -> HashMap<SmolStr, HashSet<SmolStr>> {
    let mut map: HashMap<SmolStr, HashSet<SmolStr>> = HashMap::new();

    for (_, h) in heritage {
        if h.kind == HeritageKind::Implements {
            map.entry(h.parent_name.clone())
                .or_default()
                .insert(h.file_path.clone());
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_table::SymbolDefinition;

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
            end_line: Some(50),
        }
    }

    #[test]
    fn test_resolve_same_file_call() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("greet", "src/app.ts", NodeLabel::Function));

        let mut ctx = ResolutionContext::new();

        let calls = vec![ExtractedCall {
            caller_id: SmolStr::new("src/app.ts::main::1"),
            callee_name: SmolStr::new("greet"),
            receiver: None,
            file_path: SmolStr::new("src/app.ts"),
            line: 5,
            arguments_count: 1,
        }];

        let targets =
            CallResolver::resolve_calls(&calls, "src/app.ts", &mut ctx, &symbols, &HashMap::new());

        assert_eq!(targets.len(), 1);
        assert!((targets[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_resolve_cross_file_call() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def(
            "getUser",
            "src/user-service.ts",
            NodeLabel::Function,
        ));

        let mut ctx = ResolutionContext::new();
        ctx.add_named_binding(
            SmolStr::new("src/app.ts"),
            SmolStr::new("getUser"),
            SmolStr::new("src/user-service.ts"),
            SmolStr::new("getUser"),
        );

        let calls = vec![ExtractedCall {
            caller_id: SmolStr::new("src/app.ts::main::1"),
            callee_name: SmolStr::new("getUser"),
            receiver: None,
            file_path: SmolStr::new("src/app.ts"),
            line: 5,
            arguments_count: 1,
        }];

        let targets =
            CallResolver::resolve_calls(&calls, "src/app.ts", &mut ctx, &symbols, &HashMap::new());

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].target_node_id.as_str(),
            "src/user-service.ts::getUser::1"
        );
    }
}
