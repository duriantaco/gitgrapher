use gg_core::config::Config;
use gg_core::types::Language;
use std::path::{Path, PathBuf};
use tracing::debug;

/// A source file discovered during scanning.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub language: Language,
    pub size: u64,
}

/// Scan a repository directory and collect all parseable source files.
pub fn scan_repository(root: &Path, config: &Config) -> Vec<SourceFile> {
    let mut files = Vec::new();
    scan_dir(root, root, config, &mut files);
    debug!("Scanned {} source files", files.len());
    files
}

fn scan_dir(dir: &Path, root: &Path, config: &Config, files: &mut Vec<SourceFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip ignored patterns
        if config.ignore_patterns.iter().any(|p| {
            if let Some(suffix) = p.strip_prefix('*') {
                name.ends_with(suffix)
            } else {
                name == p.as_str()
            }
        }) {
            continue;
        }

        if path.is_dir() {
            scan_dir(&path, root, config, files);
        } else if path.is_file() {
            // Check extension for language support
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Some(lang) = Language::from_extension(ext) {
                    let metadata = match std::fs::metadata(&path) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let size = metadata.len();

                    // Skip files that are too large
                    if size > config.max_file_size as u64 {
                        debug!("Skipping large file: {} ({} bytes)", path.display(), size);
                        continue;
                    }

                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();

                    files.push(SourceFile {
                        path,
                        relative_path: relative,
                        language: lang,
                        size,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_scan_ignores_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir(&nm).unwrap();
        fs::write(nm.join("dep.ts"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("main.ts"), "const y = 2;").unwrap();

        let config = Config::default();
        let files = scan_repository(dir.path(), &config);

        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("main.ts"));
    }
}
