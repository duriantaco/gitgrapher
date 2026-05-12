use gg_core::config::Config;
use gg_core::types::NodeLabel;
use gg_graph::GraphStore;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DATA_DIR: &str = ".gitgrapher";

pub fn cmd_benchmark(path: &Path, format: &str, sample_file: Option<&Path>) -> anyhow::Result<()> {
    let report = run_benchmark(path, sample_file)?;

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        "text" => print_text_report(&report),
        other => anyhow::bail!("Unknown benchmark format: {other}. Use text or json."),
    }

    Ok(())
}

fn run_benchmark(path: &Path, sample_file: Option<&Path>) -> anyhow::Result<BenchmarkReport> {
    let repo = std::fs::canonicalize(path)?;
    let config = Config::from_env();
    let _guard = IndexRestoreGuard::prepare(&repo)?;

    let cold = run_analyze(&repo, "cold")?;
    let no_change = run_analyze(&repo, "no_change")?;
    let sample = choose_sample_file(&repo, sample_file)?;
    let mut source_guard = SourceRestoreGuard::prepare(&sample)?;
    append_benchmark_marker(&sample)?;
    let one_file_incremental = run_analyze(&repo, "one_file_incremental")?;
    source_guard.restore();

    Ok(BenchmarkReport {
        schema_version: 1,
        generated_at_unix: unix_timestamp(),
        gitgrapher_version: env!("CARGO_PKG_VERSION"),
        machine: MachineInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            logical_cpus: std::thread::available_parallelism()
                .ok()
                .map(|count| count.get()),
            worker_threads: config.worker_threads,
            rustc_version: command_output("rustc", &["--version"]),
            peak_rss_source: peak_rss_source(),
        },
        repository: RepositoryInfo {
            path: repo.to_string_lossy().to_string(),
            git_commit: git_output(&repo, &["rev-parse", "HEAD"]),
        },
        sample_file: sample
            .strip_prefix(&repo)
            .unwrap_or(sample.as_path())
            .to_string_lossy()
            .to_string(),
        runs: vec![cold, no_change, one_file_incremental],
    })
}

fn run_analyze(repo: &Path, name: &'static str) -> anyhow::Result<BenchmarkRun> {
    let start = Instant::now();
    let result = gg_pipeline::analyze(repo.to_string_lossy().as_ref())?;
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    let graph_size_bytes = std::fs::metadata(repo.join(DATA_DIR).join("graph.json"))
        .map(|metadata| metadata.len())
        .ok();

    Ok(BenchmarkRun {
        name,
        duration_ms,
        files_scanned: result.files_scanned,
        total_nodes: result.total_nodes,
        total_edges: result.total_edges,
        graph_size_bytes,
        process_peak_rss_bytes: peak_rss_bytes(),
        node_counts_by_label: node_counts_by_label(&result.store),
        edge_counts_by_type: edge_counts_by_type(&result.store),
    })
}

fn node_counts_by_label(store: &GraphStore) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for node in store.nodes() {
        *counts.entry(node.label.to_string()).or_insert(0) += 1;
    }
    counts
}

fn edge_counts_by_type(store: &GraphStore) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for edge in store.edges() {
        *counts.entry(edge.rel_type.to_string()).or_insert(0) += 1;
    }
    counts
}

fn choose_sample_file(repo: &Path, sample_file: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(sample_file) = sample_file {
        let path = if sample_file.is_absolute() {
            sample_file.to_path_buf()
        } else {
            repo.join(sample_file)
        };
        if !path.is_file() {
            anyhow::bail!("Sample file does not exist: {}", path.display());
        }
        if mutation_marker(&path).is_none() {
            anyhow::bail!(
                "Sample file extension is not supported for safe benchmark mutation: {}",
                path.display()
            );
        }
        return Ok(path);
    }

    let store = load_graph(repo)?;
    let mut files = store.nodes_by_label(NodeLabel::File);
    files.sort_by(|a, b| a.properties.file_path.cmp(&b.properties.file_path));

    for node in files {
        let path = repo.join(node.properties.file_path.as_str());
        if path.is_file() && mutation_marker(&path).is_some() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "No TypeScript, JavaScript, or Python source file found for incremental benchmark"
    )
}

fn load_graph(repo: &Path) -> anyhow::Result<GraphStore> {
    GraphStore::load(&repo.join(DATA_DIR)).map_err(|err| anyhow::anyhow!(err))
}

fn append_benchmark_marker(path: &Path) -> anyhow::Result<()> {
    use std::io::Write;

    let marker = mutation_marker(path).expect("checked by choose_sample_file");
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(marker.as_bytes())?;
    Ok(())
}

fn mutation_marker(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("py" | "pyi") => Some("\n# gitgrapher benchmark mutation\n"),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" | "go" | "rs") => {
            Some("\n// gitgrapher benchmark mutation\n")
        }
        _ => None,
    }
}

fn print_text_report(report: &BenchmarkReport) {
    println!();
    println!("  GitGrapher Benchmark");
    println!("  Repository: {}", report.repository.path);
    if let Some(commit) = &report.repository.git_commit {
        println!("  Commit: {commit}");
    }
    println!("  Platform: {} {}", report.machine.os, report.machine.arch);
    if let Some(logical_cpus) = report.machine.logical_cpus {
        println!("  Logical CPUs: {logical_cpus}");
    }
    println!("  Worker threads: {}", report.machine.worker_threads);
    println!("  Sample file: {}", report.sample_file);
    println!();
    for run in &report.runs {
        println!(
            "  {:>22}: {:>8.1} ms | {:>5} files scanned | {:>8} nodes | {:>8} edges",
            run.name, run.duration_ms, run.files_scanned, run.total_nodes, run.total_edges
        );
    }
    println!();
    println!(
        "  JSON: gitgrapher benchmark --format json {}",
        report.repository.path
    );
    println!("  Existing .gitgrapher index was restored after the benchmark.");
    println!();
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let usage = unsafe { usage.assume_init() };
    let maxrss = usage.ru_maxrss as u64;
    #[cfg(target_os = "macos")]
    {
        Some(maxrss)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(maxrss * 1024)
    }
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

#[cfg(unix)]
fn peak_rss_source() -> Option<&'static str> {
    Some("getrusage(RUSAGE_SELF).ru_maxrss")
}

#[cfg(not(unix))]
fn peak_rss_source() -> Option<&'static str> {
    None
}

struct IndexRestoreGuard {
    data_dir: PathBuf,
    backup_dir: Option<PathBuf>,
}

impl IndexRestoreGuard {
    fn prepare(repo: &Path) -> anyhow::Result<Self> {
        let data_dir = repo.join(DATA_DIR);
        let backup_dir = repo.join(format!(
            ".gitgrapher.benchmark-backup-{}",
            std::process::id()
        ));

        if backup_dir.exists() {
            anyhow::bail!("Benchmark backup already exists: {}", backup_dir.display());
        }

        let backup_dir = if data_dir.exists() {
            std::fs::rename(&data_dir, &backup_dir)?;
            Some(backup_dir)
        } else {
            None
        };

        Ok(Self {
            data_dir,
            backup_dir,
        })
    }
}

impl Drop for IndexRestoreGuard {
    fn drop(&mut self) {
        if self.data_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
        if let Some(backup_dir) = &self.backup_dir {
            if backup_dir.exists() {
                let _ = std::fs::rename(backup_dir, &self.data_dir);
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    schema_version: u32,
    generated_at_unix: u64,
    gitgrapher_version: &'static str,
    machine: MachineInfo,
    repository: RepositoryInfo,
    sample_file: String,
    runs: Vec<BenchmarkRun>,
}

#[derive(Debug, Serialize)]
struct MachineInfo {
    os: &'static str,
    arch: &'static str,
    logical_cpus: Option<usize>,
    worker_threads: usize,
    rustc_version: Option<String>,
    peak_rss_source: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct RepositoryInfo {
    path: String,
    git_commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct BenchmarkRun {
    name: &'static str,
    duration_ms: f64,
    files_scanned: usize,
    total_nodes: usize,
    total_edges: usize,
    graph_size_bytes: Option<u64>,
    process_peak_rss_bytes: Option<u64>,
    node_counts_by_label: BTreeMap<String, usize>,
    edge_counts_by_type: BTreeMap<String, usize>,
}

struct SourceRestoreGuard {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl SourceRestoreGuard {
    fn prepare(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            original: Some(std::fs::read(path)?),
        })
    }

    fn restore(&mut self) {
        if let Some(original) = self.original.take() {
            let _ = std::fs::write(&self.path, original);
        }
    }
}

impl Drop for SourceRestoreGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_restores_source_file_and_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "export function handleLogin() { return true; }\n",
        )
        .unwrap();
        gg_pipeline::analyze(dir.path().to_str().unwrap()).unwrap();
        let original_index = std::fs::read(dir.path().join(DATA_DIR).join("graph.json")).unwrap();
        let original_source = std::fs::read(dir.path().join("app.ts")).unwrap();

        let report = run_benchmark(dir.path(), None).unwrap();

        assert_eq!(report.schema_version, 1);
        assert!(report.generated_at_unix > 0);
        assert_eq!(
            report.machine.worker_threads,
            Config::from_env().worker_threads
        );
        assert_eq!(
            report.runs[0].node_counts_by_label.get("Function"),
            Some(&1)
        );
        assert_eq!(report.runs.len(), 3);
        assert_eq!(
            std::fs::read(dir.path().join("app.ts")).unwrap(),
            original_source
        );
        assert_eq!(
            std::fs::read(dir.path().join(DATA_DIR).join("graph.json")).unwrap(),
            original_index
        );
    }
}
