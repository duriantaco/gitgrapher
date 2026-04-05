use serde::{Deserialize, Serialize};

/// Global configuration for GitGrapher.
///
/// All limits are tunable
/// scattered across the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Maximum file size to parse in bytes (default: 32MB).
    pub max_file_size: usize,

    /// AST cache capacity in number of parsed trees (default: 500).
    pub ast_cache_capacity: usize,

    /// Source code byte budget per processing chunk (default: 100MB).
    pub chunk_byte_budget: usize,

    /// Max files to re-process during cross-file type propagation (default: 10000).
    pub max_cross_file_reprocess: usize,

    /// Maximum fixpoint iterations for type resolution (default: 50).
    pub fixpoint_max_iterations: u32,

    /// Stop fixpoint early when delta falls below this ratio (default: 0.001).
    pub fixpoint_convergence_threshold: f64,

    /// Leiden community detection resolution parameter (default: 1.0).
    pub community_resolution: f64,

    /// Maximum number of processes to detect (default: 200).
    pub max_processes: usize,

    /// Maximum BFS depth for process tracing (default: 15).
    pub max_trace_depth: usize,

    /// Number of worker threads for parallel parsing (default: num_cpus - 1).
    pub worker_threads: usize,

    /// Patterns of files/directories to ignore during scanning.
    pub ignore_patterns: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self {
            max_file_size: 32 * 1024 * 1024, // 32MB
            ast_cache_capacity: 500,
            chunk_byte_budget: 100 * 1024 * 1024,
            max_cross_file_reprocess: 10_000,
            fixpoint_max_iterations: 50,
            fixpoint_convergence_threshold: 0.001,
            community_resolution: 1.0,
            max_processes: 200,
            max_trace_depth: 15,
            worker_threads: cpus.saturating_sub(1).max(1),
            ignore_patterns: default_ignore_patterns(),
        }
    }
}

impl Config {
    /// Load config from environment variables, falling back to defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(v) = std::env::var("GG_MAX_FILE_SIZE") {
            if let Ok(n) = v.parse() {
                config.max_file_size = n;
            }
        }
        if let Ok(v) = std::env::var("GG_AST_CACHE_CAPACITY") {
            if let Ok(n) = v.parse() {
                config.ast_cache_capacity = n;
            }
        }
        if let Ok(v) = std::env::var("GG_WORKER_THREADS") {
            if let Ok(n) = v.parse() {
                config.worker_threads = n;
            }
        }
        if let Ok(v) = std::env::var("GG_FIXPOINT_MAX_ITERATIONS") {
            if let Ok(n) = v.parse() {
                config.fixpoint_max_iterations = n;
            }
        }

        config
    }
}

fn default_ignore_patterns() -> Vec<String> {
    [
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        ".next",
        "__pycache__",
        ".venv",
        "venv",
        ".tox",
        "vendor",
        ".idea",
        ".vscode",
        "coverage",
        ".nyc_output",
        ".cache",
        "*.min.js",
        "*.min.css",
        "*.map",
        "*.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.max_file_size, 32 * 1024 * 1024);
        assert_eq!(config.ast_cache_capacity, 500);
        assert!(config.worker_threads >= 1);
        assert!(!config.ignore_patterns.is_empty());
        assert!(config.ignore_patterns.contains(&"node_modules".to_string()));
    }
}
