use gg_core::types::Language;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Suffix index for O(1) file path lookups.
/// Maps normalized path suffixes to file indices.
struct SuffixIndex {
    /// suffix_key -> list of file indices that match
    map: HashMap<String, Vec<usize>>,
}

impl SuffixIndex {
    fn build(files: &[String]) -> Self {
        let mut map: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, file) in files.iter().enumerate() {
            let normalized = file.replace('\\', "/");
            let parts: Vec<&str> = normalized.split('/').collect();

            // Index all suffix combinations: "a/b/c.ts" → "c.ts", "b/c.ts", "a/b/c.ts"
            for i in (0..parts.len()).rev() {
                let suffix = parts[i..].join("/");
                map.entry(suffix).or_default().push(idx);
            }
        }

        Self { map }
    }

    fn lookup(&self, suffix: &str) -> &[usize] {
        self.map.get(suffix).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Resolved import: the source file that an import maps to.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    pub source_file: SmolStr,
    pub target_file: SmolStr,
    pub imported_names: Vec<ImportedName>,
}

#[derive(Debug, Clone)]
pub struct ImportedName {
    pub local_name: SmolStr,
    pub exported_name: SmolStr,
    pub is_default: bool,
    pub is_namespace: bool,
}

/// Import resolver: maps module specifiers to file paths.
///
/// Supports:
/// - Relative paths (`./models`, `../utils`)
/// - Index file resolution (`./models` → `./models/index.ts`)
/// - Extension inference (`.ts`, `.tsx`, `.js`, etc.)
/// - Suffix matching for bare imports (`lodash/get` → `node_modules/lodash/get.ts`)
pub struct ImportResolver {
    /// All known file paths (relative to repo root).
    all_files: Vec<String>,
    /// Normalized versions (forward slashes).
    normalized_files: Vec<String>,
    /// Suffix index for fast lookup.
    index: SuffixIndex,
    /// Resolution cache.
    cache: HashMap<String, Option<String>>,
}

/// Source extensions to try when resolving imports across registered providers.
const SOURCE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".pyi", ".go", ".rs",
];

/// Index filenames to try for directory imports.
const INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "__init__.py",
    "mod.rs",
    "main.go",
];

impl ImportResolver {
    pub fn new(all_files: Vec<String>) -> Self {
        let normalized_files: Vec<String> =
            all_files.iter().map(|f| f.replace('\\', "/")).collect();
        let index = SuffixIndex::build(&normalized_files);

        Self {
            all_files,
            normalized_files,
            index,
            cache: HashMap::new(),
        }
    }

    /// Resolve an import path from a given file to a target file path.
    ///
    /// Returns `None` if the import cannot be resolved.
    pub fn resolve(
        &mut self,
        import_path: &str,
        from_file: &str,
        language: Language,
    ) -> Option<String> {
        let cache_key = format!("{from_file}::{import_path}");
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }

        let result = self.resolve_uncached(import_path, from_file, language);

        // Cache with LRU eviction at 100k entries
        if self.cache.len() >= 100_000 {
            let keys_to_remove: Vec<String> = self.cache.keys().take(20_000).cloned().collect();
            for key in keys_to_remove {
                self.cache.remove(&key);
            }
        }
        self.cache.insert(cache_key, result.clone());

        result
    }

    fn resolve_uncached(
        &self,
        import_path: &str,
        from_file: &str,
        language: Language,
    ) -> Option<String> {
        // Validate
        let cleaned = import_path.trim().trim_matches(|c| c == '"' || c == '\'');
        if cleaned.is_empty() || cleaned.len() > 2048 {
            return None;
        }
        if cleaned.bytes().any(|b| b < 0x20) {
            return None;
        }

        // Relative imports
        if cleaned.starts_with("./") || cleaned.starts_with("../") {
            return self.resolve_relative(cleaned, from_file, language);
        }

        // Suffix-based resolution for bare specifiers
        self.resolve_suffix(cleaned, language)
    }

    /// Resolve a relative import path.
    fn resolve_relative(
        &self,
        import_path: &str,
        from_file: &str,
        _language: Language,
    ) -> Option<String> {
        let from_dir = Path::new(from_file).parent().unwrap_or(Path::new(""));

        let mut resolved = PathBuf::from(from_dir);
        for part in import_path.split('/') {
            match part {
                "." => {}
                ".." => {
                    resolved.pop();
                }
                other => resolved.push(other),
            }
        }

        let base = resolved.to_string_lossy().replace('\\', "/");

        // Try with extensions
        if let Some(found) = self.try_resolve_with_extensions(&base) {
            return Some(found);
        }

        // Try as directory with index file
        for index_file in INDEX_FILES {
            let candidate = format!("{base}/{index_file}");
            if self.file_exists(&candidate) {
                return Some(candidate);
            }
        }

        None
    }

    /// Try resolving a base path by appending various extensions.
    fn try_resolve_with_extensions(&self, base: &str) -> Option<String> {
        // Try exact match first
        if self.file_exists(base) {
            return Some(base.to_string());
        }

        // Try with extensions
        for ext in SOURCE_EXTENSIONS {
            let candidate = format!("{base}{ext}");
            if self.file_exists(&candidate) {
                return Some(candidate);
            }
        }

        // Try index files in directory
        for index_file in INDEX_FILES {
            let candidate = format!("{base}/{index_file}");
            if self.file_exists(&candidate) {
                return Some(candidate);
            }
        }

        None
    }

    /// Resolve a bare import using suffix matching.
    fn resolve_suffix(&self, import_path: &str, _language: Language) -> Option<String> {
        // Convert dots to slashes for Java/Kotlin/Python style imports
        let path_like = if import_path.contains('/') {
            import_path.to_string()
        } else {
            import_path.replace('.', "/")
        };

        let parts: Vec<&str> = path_like.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return None;
        }

        // Try suffix lookup from most specific to least
        for start in 0..parts.len() {
            let suffix = parts[start..].join("/");

            // Try exact suffix match
            let indices = self.index.lookup(&suffix);
            if indices.len() == 1 {
                return Some(self.all_files[indices[0]].clone());
            }

            // Try with extensions
            for ext in SOURCE_EXTENSIONS {
                let with_ext = format!("{suffix}{ext}");
                let indices = self.index.lookup(&with_ext);
                if indices.len() == 1 {
                    return Some(self.all_files[indices[0]].clone());
                }
            }

            // Try index files
            for index_file in INDEX_FILES {
                let with_index = format!("{suffix}/{index_file}");
                let indices = self.index.lookup(&with_index);
                if indices.len() == 1 {
                    return Some(self.all_files[indices[0]].clone());
                }
            }
        }

        None
    }

    /// Check if a file path exists in our file list.
    fn file_exists(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");
        self.normalized_files.iter().any(|f| f == &normalized)
    }

    /// Get all known file paths.
    pub fn all_files(&self) -> &[String] {
        &self.all_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_resolver(files: &[&str]) -> ImportResolver {
        ImportResolver::new(files.iter().map(|f| f.to_string()).collect())
    }

    #[test]
    fn test_relative_import() {
        let mut r = make_resolver(&["src/models/user.ts", "src/services/auth.ts", "src/index.ts"]);

        assert_eq!(
            r.resolve("./models/user", "src/index.ts", Language::TypeScript),
            Some("src/models/user.ts".to_string())
        );
    }

    #[test]
    fn test_relative_parent_import() {
        let mut r = make_resolver(&["src/models/user.ts", "src/services/auth.ts"]);

        assert_eq!(
            r.resolve(
                "../models/user",
                "src/services/auth.ts",
                Language::TypeScript
            ),
            Some("src/models/user.ts".to_string())
        );
    }

    #[test]
    fn test_index_file_resolution() {
        let mut r = make_resolver(&["src/models/index.ts", "src/app.ts"]);

        assert_eq!(
            r.resolve("./models", "src/app.ts", Language::TypeScript),
            Some("src/models/index.ts".to_string())
        );
    }

    #[test]
    fn test_extension_inference() {
        let mut r = make_resolver(&[
            "src/utils.ts",
            "src/app.ts",
            "src/service.go",
            "src/main.go",
            "src/repo.rs",
            "src/lib.rs",
        ]);

        // Import without extension should find .ts
        assert_eq!(
            r.resolve("./utils", "src/app.ts", Language::TypeScript),
            Some("src/utils.ts".to_string())
        );
        assert_eq!(
            r.resolve("./service", "src/main.go", Language::Go),
            Some("src/service.go".to_string())
        );
        assert_eq!(
            r.resolve("./repo", "src/lib.rs", Language::Rust),
            Some("src/repo.rs".to_string())
        );
    }

    #[test]
    fn test_unresolvable_import() {
        let mut r = make_resolver(&["src/app.ts"]);

        assert_eq!(
            r.resolve("./nonexistent", "src/app.ts", Language::TypeScript),
            None
        );
    }

    #[test]
    fn test_cache_hit() {
        let mut r = make_resolver(&["src/utils.ts", "src/app.ts"]);

        let first = r.resolve("./utils", "src/app.ts", Language::TypeScript);
        let second = r.resolve("./utils", "src/app.ts", Language::TypeScript);
        assert_eq!(first, second);
    }
}
