use gg_core::config::Config;
use smol_str::SmolStr;
use std::collections::HashMap;
use tracing::{debug, info};

/// Result of fixpoint type resolution.
#[derive(Debug)]
pub struct FixpointResult {
    pub iterations: u32,
    pub total_resolved: usize,
    pub converged: bool,
}

/// A pending assignment that needs type resolution.
///
/// Example: `const user = getUser()` → we know `user` is assigned from `getUser()`,
/// but we need to look up `getUser`'s return type to know the type of `user`.
#[derive(Debug, Clone)]
pub struct PendingAssignment {
    pub variable_name: SmolStr,
    pub assigned_from: AssignmentSource,
    pub scope: SmolStr,
    pub file_path: SmolStr,
    pub resolved_type: Option<SmolStr>,
}

#[derive(Debug, Clone)]
pub enum AssignmentSource {
    /// `const x = new Foo()` → type is "Foo"
    Constructor(SmolStr),
    /// `const x = foo()` → type is the return type of `foo`
    FunctionCall(SmolStr),
    /// `const x: Foo = ...` → type is explicitly annotated as "Foo"
    TypeAnnotation(SmolStr),
    /// `const x = y` → type is the type of `y`
    Variable(SmolStr),
}

/// Per-file type environment with scope-aware variable tracking.
///
/// Tracks what type each variable has at each scope level.
#[derive(Debug, Default)]
pub struct TypeEnv {
    /// scope -> variable_name -> type_name
    scopes: HashMap<SmolStr, HashMap<SmolStr, SmolStr>>,
    /// Constructor bindings: variable -> class_name
    constructor_types: HashMap<SmolStr, SmolStr>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a type binding (variable has a known type).
    pub fn set_type(&mut self, scope: &str, variable: &str, type_name: &str) {
        self.scopes
            .entry(SmolStr::new(scope))
            .or_default()
            .insert(SmolStr::new(variable), SmolStr::new(type_name));
    }

    /// Look up the type of a variable in a given scope.
    /// Falls back to file-level scope if not found in the given scope.
    pub fn get_type(&self, scope: &str, variable: &str) -> Option<&SmolStr> {
        // Try the exact scope first
        if let Some(scope_map) = self.scopes.get(scope) {
            if let Some(t) = scope_map.get(variable) {
                return Some(t);
            }
        }

        // Fall back to file-level scope (scope == file_path)
        let file_scope = scope.split("::").next().unwrap_or(scope);
        if file_scope != scope {
            if let Some(scope_map) = self.scopes.get(file_scope) {
                if let Some(t) = scope_map.get(variable) {
                    return Some(t);
                }
            }
        }

        None
    }

    /// Record a constructor binding: `const x = new Foo()`
    pub fn set_constructor_type(&mut self, variable: &str, class_name: &str) {
        self.constructor_types
            .insert(SmolStr::new(variable), SmolStr::new(class_name));
    }

    /// Get the constructor type for a variable.
    pub fn get_constructor_type(&self, variable: &str) -> Option<&SmolStr> {
        self.constructor_types.get(variable)
    }

    /// Get all known type bindings (for export to cross-file resolution).
    pub fn all_bindings(&self) -> impl Iterator<Item = (&SmolStr, &SmolStr, &SmolStr)> {
        self.scopes
            .iter()
            .flat_map(|(scope, vars)| vars.iter().map(move |(var, typ)| (scope, var, typ)))
    }

    pub fn binding_count(&self) -> usize {
        self.scopes.values().map(|m| m.len()).sum()
    }
}

/// Exported type map: file_path → (exported_name → type_name).
///
/// Used for cross-file type propagation. When file A exports `function getUser(): User`,
/// other files importing `getUser` can learn that its return type is `User`.
pub type ExportedTypeMap = HashMap<SmolStr, HashMap<SmolStr, SmolStr>>;

/// Build the exported type map from the symbol table.
pub fn build_exported_type_map(
    symbols: &crate::symbol_table::SymbolTable,
    file_paths: &[String],
) -> ExportedTypeMap {
    let mut map = ExportedTypeMap::new();

    for file_path in file_paths {
        let file_symbols = symbols.symbols_in_file(file_path);
        let mut file_exports: HashMap<SmolStr, SmolStr> = HashMap::new();

        for sym in file_symbols {
            if !sym.is_exported {
                continue;
            }
            // Cap exports per file to prevent pathological cases
            if file_exports.len() >= 500 {
                break;
            }
            if let Some(ref return_type) = sym.return_type {
                if return_type.len() <= 256 {
                    file_exports.insert(sym.name.clone(), return_type.clone());
                }
            }
        }

        if !file_exports.is_empty() {
            map.insert(SmolStr::new(file_path), file_exports);
        }
    }

    map
}

/// Run fixpoint type resolution with convergence detection.
///
/// Iteratively resolves pending assignments until:
/// - No more assignments can be resolved (convergence)
/// - The convergence threshold is met (delta < threshold)
/// - Maximum iterations reached
/// - Hardcoded 10 iteration cap
/// - No convergence detection
/// - No progress reporting
pub fn resolve_fixpoint(
    pending: &mut [PendingAssignment],
    env: &mut TypeEnv,
    return_types: &HashMap<SmolStr, SmolStr>,
    config: &Config,
) -> FixpointResult {
    let mut iteration = 0;
    let mut prev_resolved = 0;
    let total_pending = pending.len();

    loop {
        iteration += 1;
        let mut _resolved_this_round = 0;

        for assignment in pending.iter_mut() {
            if assignment.resolved_type.is_some() {
                continue;
            }

            let resolved_type = match &assignment.assigned_from {
                AssignmentSource::Constructor(class_name) => Some(class_name.clone()),
                AssignmentSource::TypeAnnotation(type_name) => Some(type_name.clone()),
                AssignmentSource::FunctionCall(fn_name) => {
                    // Look up the return type of the function
                    return_types.get(fn_name.as_str()).cloned().or_else(|| {
                        // Try the type env (might have been resolved in a previous iteration)
                        env.get_type(&assignment.scope, fn_name).cloned()
                    })
                }
                AssignmentSource::Variable(var_name) => {
                    // Look up the type of the source variable
                    env.get_type(&assignment.scope, var_name)
                        .cloned()
                        .or_else(|| env.get_constructor_type(var_name).cloned())
                }
            };

            if let Some(ref type_name) = resolved_type {
                env.set_type(&assignment.scope, &assignment.variable_name, type_name);
                assignment.resolved_type = Some(type_name.clone());
                _resolved_this_round += 1;
            }
        }

        let total_resolved = pending.iter().filter(|a| a.resolved_type.is_some()).count();
        let delta = total_resolved - prev_resolved;
        let convergence_ratio = if total_resolved > 0 {
            delta as f64 / total_resolved as f64
        } else {
            0.0
        };

        debug!(
            "Fixpoint iteration {}: resolved {}/{} (+{}, convergence: {:.4})",
            iteration, total_resolved, total_pending, delta, convergence_ratio
        );

        // Check termination conditions
        if delta == 0 {
            info!(
                "Fixpoint converged after {} iterations: {}/{} resolved",
                iteration, total_resolved, total_pending
            );
            return FixpointResult {
                iterations: iteration,
                total_resolved,
                converged: true,
            };
        }

        if iteration >= config.fixpoint_max_iterations {
            info!(
                "Fixpoint hit max iterations ({}): {}/{} resolved",
                config.fixpoint_max_iterations, total_resolved, total_pending
            );
            return FixpointResult {
                iterations: iteration,
                total_resolved,
                converged: false,
            };
        }

        if convergence_ratio < config.fixpoint_convergence_threshold && iteration > 1 {
            info!(
                "Fixpoint converged by threshold ({:.4} < {:.4}) after {} iterations: {}/{} resolved",
                convergence_ratio, config.fixpoint_convergence_threshold,
                iteration, total_resolved, total_pending
            );
            return FixpointResult {
                iterations: iteration,
                total_resolved,
                converged: true,
            };
        }

        prev_resolved = total_resolved;
    }
}

/// Extract a simple type name from a possibly complex return type.
///
/// `Promise<User>` → `User`
/// `Array<string>` → `string`
/// `User | null` → `User`
/// `User[]` → `User`
pub fn simplify_type_name(type_name: &str) -> &str {
    let trimmed = type_name.trim();

    // Handle Promise<T>
    if let Some(inner) = trimmed
        .strip_prefix("Promise<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return simplify_type_name(inner);
    }

    // Handle Array<T>
    if let Some(inner) = trimmed
        .strip_prefix("Array<")
        .and_then(|s| s.strip_suffix('>'))
    {
        return simplify_type_name(inner);
    }

    // Handle T[]
    if let Some(base) = trimmed.strip_suffix("[]") {
        return simplify_type_name(base);
    }

    // Handle T | null / T | undefined (union types — take the non-null part)
    if trimmed.contains('|') {
        for part in trimmed.split('|') {
            let part = part.trim();
            if part != "null" && part != "undefined" && part != "void" {
                return simplify_type_name(part);
            }
        }
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_env_scope_lookup() {
        let mut env = TypeEnv::new();
        env.set_type("src/app.ts::main", "user", "User");

        assert_eq!(
            env.get_type("src/app.ts::main", "user").map(|s| s.as_str()),
            Some("User")
        );
        // Should not be visible in a different scope
        assert!(env.get_type("src/app.ts::other", "user").is_none());
    }

    #[test]
    fn test_type_env_file_scope_fallback() {
        let mut env = TypeEnv::new();
        env.set_type("src/app.ts", "globalVar", "Config");

        // File-level variable should be visible from any scope in the file
        assert_eq!(
            env.get_type("src/app.ts::main", "globalVar")
                .map(|s| s.as_str()),
            Some("Config")
        );
    }

    #[test]
    fn test_fixpoint_resolves_constructors_immediately() {
        let config = Config::default();
        let mut env = TypeEnv::new();
        let return_types = HashMap::new();

        let mut pending = vec![PendingAssignment {
            variable_name: SmolStr::new("user"),
            assigned_from: AssignmentSource::Constructor(SmolStr::new("User")),
            scope: SmolStr::new("src/app.ts::main"),
            file_path: SmolStr::new("src/app.ts"),
            resolved_type: None,
        }];

        let result = resolve_fixpoint(&mut pending, &mut env, &return_types, &config);
        assert!(result.converged);
        assert_eq!(result.total_resolved, 1);
        assert_eq!(result.iterations, 2); // 1 to resolve, 1 to confirm convergence
        assert_eq!(
            env.get_type("src/app.ts::main", "user").map(|s| s.as_str()),
            Some("User")
        );
    }

    #[test]
    fn test_fixpoint_chain_resolution() {
        let config = Config::default();
        let mut env = TypeEnv::new();
        let mut return_types = HashMap::new();
        return_types.insert(SmolStr::new("getUser"), SmolStr::new("User"));

        let mut pending = vec![
            // const user = getUser()
            PendingAssignment {
                variable_name: SmolStr::new("user"),
                assigned_from: AssignmentSource::FunctionCall(SmolStr::new("getUser")),
                scope: SmolStr::new("src/app.ts::main"),
                file_path: SmolStr::new("src/app.ts"),
                resolved_type: None,
            },
            // const name = user (needs user's type to resolve)
            PendingAssignment {
                variable_name: SmolStr::new("name"),
                assigned_from: AssignmentSource::Variable(SmolStr::new("user")),
                scope: SmolStr::new("src/app.ts::main"),
                file_path: SmolStr::new("src/app.ts"),
                resolved_type: None,
            },
        ];

        let result = resolve_fixpoint(&mut pending, &mut env, &return_types, &config);
        assert!(result.converged);
        assert_eq!(result.total_resolved, 2);
        assert_eq!(
            env.get_type("src/app.ts::main", "name").map(|s| s.as_str()),
            Some("User")
        );
    }

    #[test]
    fn test_simplify_type_name() {
        assert_eq!(simplify_type_name("Promise<User>"), "User");
        assert_eq!(simplify_type_name("Array<string>"), "string");
        assert_eq!(simplify_type_name("User[]"), "User");
        assert_eq!(simplify_type_name("User | null"), "User");
        assert_eq!(simplify_type_name("User | undefined"), "User");
        assert_eq!(simplify_type_name("string"), "string");
    }
}
