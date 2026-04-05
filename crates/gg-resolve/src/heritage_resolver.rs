use crate::resolution::{ResolutionContext, ResolutionTier};
use crate::symbol_table::SymbolTable;
use gg_core::types::*;
use smol_str::SmolStr;

/// A resolved heritage (extends/implements) edge.
#[derive(Debug, Clone)]
pub struct ResolvedHeritage {
    pub child_id: SmolStr,
    pub parent_id: SmolStr,
    pub rel_type: RelationType,
    pub confidence: f64,
    pub reason: SmolStr,
}

/// Heritage resolver: links child classes/interfaces to their parent definitions.
pub struct HeritageResolver;

impl HeritageResolver {
    /// Resolve all heritage declarations to graph edges.
    pub fn resolve_heritage(
        heritage: &[ExtractedHeritage],
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
    ) -> Vec<ResolvedHeritage> {
        let mut results = Vec::new();

        for h in heritage {
            if let Some(resolved) = Self::resolve_single(h, ctx, symbols) {
                results.push(resolved);
            }
        }

        results
    }

    fn resolve_single(
        h: &ExtractedHeritage,
        ctx: &mut ResolutionContext,
        symbols: &SymbolTable,
    ) -> Option<ResolvedHeritage> {
        let resolved = ctx.resolve(&h.parent_name, &h.file_path, symbols);

        let (parent_id, parent_confidence) = match resolved {
            Some(ref r) => {
                // Refuse ambiguous global matches (better uncertain than wrong)
                if r.tier == ResolutionTier::Global && r.candidates.len() > 1 {
                    let synthetic_id =
                        SmolStr::new(format!("{}::{}", label_prefix(h.kind), h.parent_name));
                    (synthetic_id, 0.5)
                } else if let Some(best) = r.best() {
                    (best.definition.node_id.clone(), best.confidence)
                } else {
                    return None;
                }
            }
            None => {
                // Unresolved: create synthetic ID with low confidence
                let synthetic_id =
                    SmolStr::new(format!("{}::{}", label_prefix(h.kind), h.parent_name));
                (synthetic_id, 0.5)
            }
        };

        // Determine actual edge type from resolution
        let rel_type = match h.kind {
            HeritageKind::Implements => RelationType::Implements,
            HeritageKind::Extends => {
                // Check if the resolved parent is an interface — if so, use IMPLEMENTS
                if let Some(ref r) = resolved {
                    if let Some(best) = r.best() {
                        if best.definition.label == NodeLabel::Interface {
                            RelationType::Implements
                        } else {
                            RelationType::Extends
                        }
                    } else {
                        RelationType::Extends
                    }
                } else {
                    RelationType::Extends
                }
            }
        };

        // Confidence = sqrt(child_confidence * parent_confidence)
        // Child confidence is 0.95 (it's a declaration, we're certain about the child)
        let child_confidence = 0.95;
        let combined = (child_confidence * parent_confidence).sqrt();

        Some(ResolvedHeritage {
            child_id: h.child_id.clone(),
            parent_id,
            rel_type,
            confidence: combined,
            reason: SmolStr::new(format!(
                "heritage: {} {} {}",
                h.child_id,
                if rel_type == RelationType::Extends {
                    "extends"
                } else {
                    "implements"
                },
                h.parent_name
            )),
        })
    }
}

fn label_prefix(kind: HeritageKind) -> &'static str {
    match kind {
        HeritageKind::Extends => "Class",
        HeritageKind::Implements => "Interface",
    }
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
    fn test_resolve_extends_same_file() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def("Base", "src/app.ts", NodeLabel::Class));

        let mut ctx = ResolutionContext::new();

        let heritage = vec![ExtractedHeritage {
            child_id: SmolStr::new("src/app.ts::Child::10"),
            parent_name: SmolStr::new("Base"),
            kind: HeritageKind::Extends,
            file_path: SmolStr::new("src/app.ts"),
        }];

        let resolved = HeritageResolver::resolve_heritage(&heritage, &mut ctx, &symbols);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rel_type, RelationType::Extends);
        assert!(resolved[0].confidence > 0.9);
    }

    #[test]
    fn test_resolve_implements_cross_file() {
        let symbols = SymbolTable::new();
        symbols.insert(make_def(
            "Serializable",
            "src/interfaces.ts",
            NodeLabel::Interface,
        ));

        let mut ctx = ResolutionContext::new();
        ctx.add_named_binding(
            SmolStr::new("src/user.ts"),
            SmolStr::new("Serializable"),
            SmolStr::new("src/interfaces.ts"),
            SmolStr::new("Serializable"),
        );

        let heritage = vec![ExtractedHeritage {
            child_id: SmolStr::new("src/user.ts::User::1"),
            parent_name: SmolStr::new("Serializable"),
            kind: HeritageKind::Implements,
            file_path: SmolStr::new("src/user.ts"),
        }];

        let resolved = HeritageResolver::resolve_heritage(&heritage, &mut ctx, &symbols);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rel_type, RelationType::Implements);
        assert_eq!(
            resolved[0].parent_id.as_str(),
            "src/interfaces.ts::Serializable::1"
        );
    }

    #[test]
    fn test_unresolved_heritage_gets_synthetic_id() {
        let symbols = SymbolTable::new();
        let mut ctx = ResolutionContext::new();

        let heritage = vec![ExtractedHeritage {
            child_id: SmolStr::new("src/user.ts::User::1"),
            parent_name: SmolStr::new("ExternalBase"),
            kind: HeritageKind::Extends,
            file_path: SmolStr::new("src/user.ts"),
        }];

        let resolved = HeritageResolver::resolve_heritage(&heritage, &mut ctx, &symbols);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].parent_id.as_str(), "Class::ExternalBase");
        assert!(resolved[0].confidence < 0.75);
    }
}
