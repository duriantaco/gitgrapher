use gg_core::config::Config;
use gg_core::types::Language;
use std::collections::HashSet;
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
    scan_repository_with_languages(root, config, Language::all())
}

/// Scan a repository directory and collect source files for registered providers.
pub fn scan_repository_with_languages(
    root: &Path,
    config: &Config,
    languages: &[Language],
) -> Vec<SourceFile> {
    let supported = languages.iter().copied().collect::<HashSet<_>>();
    let mut scanner = Scanner {
        root,
        config,
        supported,
        files: Vec::new(),
    };
    scanner.scan_dir(root, Vec::new());
    let mut files = scanner.files;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    debug!("Scanned {} source files", files.len());
    files
}

struct Scanner<'a> {
    root: &'a Path,
    config: &'a Config,
    supported: HashSet<Language>,
    files: Vec<SourceFile>,
}

impl Scanner<'_> {
    fn scan_dir(&mut self, dir: &Path, mut ignore_rules: Vec<IgnoreRule>) {
        ignore_rules.extend(load_gitignore(dir, self.root));

        let mut entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(_) => return,
        };
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let file_type = metadata.file_type();

            if file_type.is_symlink() {
                debug!("Skipping symlink: {}", path.display());
                continue;
            }

            let relative = relative_path(self.root, &path);
            let is_dir = file_type.is_dir();

            if is_ignored(
                &name,
                &relative,
                is_dir,
                self.config,
                ignore_rules.as_slice(),
            ) {
                continue;
            }

            if is_dir {
                self.scan_dir(&path, ignore_rules.clone());
            } else if file_type.is_file() {
                self.scan_file(path, relative, metadata.len());
            }
        }
    }

    fn scan_file(&mut self, path: PathBuf, relative_path: String, size: u64) {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return;
        };
        let Some(lang) = Language::from_extension(ext) else {
            return;
        };
        if !self.supported.contains(&lang) {
            return;
        }

        if size > self.config.max_file_size as u64 {
            debug!("Skipping large file: {} ({} bytes)", path.display(), size);
            return;
        }

        self.files.push(SourceFile {
            path,
            relative_path,
            language: lang,
            size,
        });
    }
}

#[derive(Clone, Debug)]
struct IgnoreRule {
    base: String,
    pattern: String,
    directory_only: bool,
    negated: bool,
}

fn load_gitignore(dir: &Path, root: &Path) -> Vec<IgnoreRule> {
    let path = dir.join(".gitignore");
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let base = relative_path(root, dir);

    contents
        .lines()
        .filter_map(|line| parse_gitignore_line(line, &base))
        .collect()
}

fn parse_gitignore_line(line: &str, base: &str) -> Option<IgnoreRule> {
    let mut line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    if let Some(unescaped) = line.strip_prefix("\\#") {
        line = unescaped;
    }

    let negated = line.starts_with('!');
    if negated {
        line = line[1..].trim_start();
    }

    let directory_only = line.ends_with('/');
    if directory_only {
        line = line.trim_end_matches('/');
    }

    line = line.trim_start_matches('/');
    if line.is_empty() {
        return None;
    }

    Some(IgnoreRule {
        base: base.to_string(),
        pattern: normalize_pattern(line),
        directory_only,
        negated,
    })
}

fn is_ignored(
    name: &str,
    relative_path: &str,
    is_dir: bool,
    config: &Config,
    rules: &[IgnoreRule],
) -> bool {
    let config_ignored = config
        .ignore_patterns
        .iter()
        .any(|pattern| config_pattern_matches(pattern, name, relative_path, is_dir));
    if config_ignored {
        return true;
    }

    let mut ignored = None;
    for rule in rules {
        if rule_matches(rule, name, relative_path, is_dir) {
            ignored = Some(!rule.negated);
        }
    }

    ignored.unwrap_or(false)
}

fn config_pattern_matches(pattern: &str, name: &str, relative_path: &str, is_dir: bool) -> bool {
    let directory_only = pattern.ends_with('/');
    if directory_only && !is_dir {
        return false;
    }
    let pattern = normalize_pattern(pattern.trim_end_matches('/'));

    if pattern.contains('/') {
        glob_match(&pattern, relative_path)
    } else {
        glob_match(&pattern, name)
    }
}

fn rule_matches(rule: &IgnoreRule, name: &str, relative_path: &str, is_dir: bool) -> bool {
    if rule.directory_only && !is_dir {
        return false;
    }

    let Some(path_under_base) = strip_base(relative_path, &rule.base) else {
        return false;
    };
    let candidate = if rule.pattern.contains('/') {
        path_under_base
    } else {
        name
    };

    glob_match(&rule.pattern, candidate)
}

fn strip_base<'a>(path: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(path);
    }
    if path == base {
        return Some("");
    }
    path.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_pattern(pattern: &str) -> String {
    pattern.replace('\\', "/")
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == text {
        return true;
    }
    if !pattern.contains('*') {
        return false;
    }

    let mut remainder = text;
    let mut parts = pattern.split('*').peekable();
    let mut first = true;

    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        if first && !pattern.starts_with('*') {
            let Some(rest) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = rest;
        } else if parts.peek().is_none() && !pattern.ends_with('*') {
            return remainder.ends_with(part);
        } else if let Some(idx) = remainder.find(part) {
            remainder = &remainder[idx + part.len()..];
        } else {
            return false;
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
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

    #[test]
    fn test_scan_respects_gitignore_and_negation() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".gitignore"),
            "generated/\n*.gen.ts\n!keep.gen.ts\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("generated")).unwrap();
        fs::write(dir.path().join("generated/skip.ts"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("skip.gen.ts"), "export const y = 1;").unwrap();
        fs::write(dir.path().join("keep.gen.ts"), "export const z = 1;").unwrap();
        fs::write(dir.path().join("main.ts"), "export const ok = 1;").unwrap();

        let config = Config::default();
        let files = scan_repository(dir.path(), &config);
        let paths = files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["keep.gen.ts", "main.ts"]);
    }

    #[test]
    fn test_scan_respects_nested_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/.gitignore"), "skip.ts\n").unwrap();
        fs::write(dir.path().join("src/keep.ts"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("src/skip.ts"), "export const y = 1;").unwrap();

        let config = Config::default();
        let files = scan_repository(dir.path(), &config);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "src/keep.ts");
    }

    #[test]
    fn test_scan_filters_registered_languages() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.ts"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("main.py"), "x = 1").unwrap();

        let config = Config::default();
        let files = scan_repository_with_languages(dir.path(), &config, &[Language::Python]);

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "main.py");
    }

    #[cfg(unix)]
    #[test]
    fn test_scan_does_not_follow_symlink_dirs() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("main.ts"), "export const x = 1;").unwrap();
        symlink(&real, dir.path().join("linked")).unwrap();

        let config = Config::default();
        let files = scan_repository(dir.path(), &config);
        let paths = files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["real/main.ts"]);
    }
}
