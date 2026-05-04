#[macro_use]
extern crate napi_derive;

use napi::bindgen_prelude::*;
use serde::{Deserialize, Serialize};

#[napi(object)]
#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzeOptions {
    /// Maximum number of worker threads (default: num_cpus - 1)
    pub threads: Option<u32>,
    /// Enable verbose logging
    pub verbose: Option<bool>,
}

#[napi(object)]
#[derive(Debug, Serialize, Deserialize)]
pub struct AnalyzeResult {
    /// Total files scanned
    pub files_scanned: u32,
    /// Total nodes in graph
    pub total_nodes: u32,
    /// Total edges in graph
    pub total_edges: u32,
    /// Time taken in milliseconds
    pub duration_ms: f64,
    /// Import edges resolved
    pub import_edges: u32,
    /// Call edges resolved
    pub call_edges: u32,
    /// Heritage edges resolved
    pub heritage_edges: u32,
}

#[napi]
pub struct GitGrapher {
    // Will hold pipeline + graph store once implemented
}

#[napi]
impl GitGrapher {
    #[napi(constructor)]
    pub fn new() -> Self {
        GitGrapher {}
    }

    /// Analyze a repository and build its knowledge graph.
    #[napi]
    pub async fn analyze(
        &self,
        repo_path: String,
        options: Option<AnalyzeOptions>,
    ) -> Result<AnalyzeResult> {
        let start = std::time::Instant::now();
        let _opts = options.unwrap_or(AnalyzeOptions {
            threads: None,
            verbose: None,
        });

        // Phase 1: Use the pipeline to analyze
        let result = gg_pipeline::analyze(&repo_path)
            .map_err(|e| Error::from_reason(format!("Analysis failed: {e}")))?;

        Ok(AnalyzeResult {
            files_scanned: result.files_scanned as u32,
            total_nodes: result.total_nodes as u32,
            total_edges: result.total_edges as u32,
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
            import_edges: result.resolution_stats.import_edges_resolved as u32,
            call_edges: result.resolution_stats.call_edges_resolved as u32,
            heritage_edges: result.resolution_stats.heritage_edges_resolved as u32,
        })
    }
}

impl Default for GitGrapher {
    fn default() -> Self {
        Self::new()
    }
}
