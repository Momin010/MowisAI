//! Workspace Management — Multi-project support, git worktrees, context tracking
//!
//! Manages project workspaces with git integration, file watching,
//! and context-aware file selection for agents.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Workspace configuration for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub root: PathBuf,
    pub git_repo: bool,
    pub branch: Option<String>,
    pub languages: Vec<String>,
    pub build_system: Option<String>,
    pub test_command: Option<String>,
    pub lint_command: Option<String>,
    pub file_count: usize,
    pub total_lines: usize,
    pub ignore_patterns: Vec<String>,
}

/// Detect workspace properties from a directory
pub fn detect_workspace(root: &Path) -> Workspace {
    let git_repo = root.join(".git").exists();
    let branch = if git_repo {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .arg("branch")
            .arg("--show-current")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    let languages = detect_languages(root);
    let build_system = detect_build_system(root);
    let test_command = detect_test_command(root);
    let lint_command = detect_lint_command(root);
    let (file_count, total_lines) = count_files_and_lines(root);
    let ignore_patterns = load_gitignore(root);

    Workspace {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string()),
        root: root.to_path_buf(),
        git_repo,
        branch,
        languages,
        build_system,
        test_command,
        lint_command,
        file_count,
        total_lines,
        ignore_patterns,
    }
}

fn detect_languages(root: &Path) -> Vec<String> {
    let mut languages = Vec::new();

    if root.join("Cargo.toml").exists() {
        languages.push("rust".to_string());
    }
    if root.join("package.json").exists() {
        languages.push("javascript".to_string());
    }
    if root.join("tsconfig.json").exists() {
        languages.push("typescript".to_string());
    }
    if root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
        || root.join("requirements.txt").exists()
    {
        languages.push("python".to_string());
    }
    if root.join("go.mod").exists() {
        languages.push("go".to_string());
    }
    if root.join("pom.xml").exists() || root.join("build.gradle").exists() {
        languages.push("java".to_string());
    }
    if root.join("Gemfile").exists() {
        languages.push("ruby".to_string());
    }
    if root.join("composer.json").exists() {
        languages.push("php".to_string());
    }
    if root.join("CMakeLists.txt").exists() {
        languages.push("cpp".to_string());
    }
    if root.join("Makefile").exists() && !languages.contains(&"rust".to_string()) {
        languages.push("c".to_string());
    }
    if root.join("Dockerfile").exists() {
        languages.push("docker".to_string());
    }

    languages
}

fn detect_build_system(root: &Path) -> Option<String> {
    if root.join("Cargo.toml").exists() {
        return Some("cargo".to_string());
    }
    if root.join("package.json").exists() {
        return Some("npm".to_string());
    }
    if root.join("Makefile").exists() {
        return Some("make".to_string());
    }
    if root.join("CMakeLists.txt").exists() {
        return Some("cmake".to_string());
    }
    if root.join("pom.xml").exists() {
        return Some("maven".to_string());
    }
    if root.join("build.gradle").exists() {
        return Some("gradle".to_string());
    }
    if root.join("pyproject.toml").exists() {
        return Some("poetry".to_string());
    }
    None
}

fn detect_test_command(root: &Path) -> Option<String> {
    if root.join("Cargo.toml").exists() {
        return Some("cargo test".to_string());
    }
    if root.join("package.json").exists() {
        let pkg = std::fs::read_to_string(root.join("package.json")).unwrap_or_default();
        if pkg.contains("\"test\"") {
            return Some("npm test".to_string());
        }
    }
    if root.join("pyproject.toml").exists() || root.join("pytest.ini").exists() {
        return Some("pytest".to_string());
    }
    if root.join("go.mod").exists() {
        return Some("go test ./...".to_string());
    }
    None
}

fn detect_lint_command(root: &Path) -> Option<String> {
    if root.join("Cargo.toml").exists() {
        return Some("cargo clippy".to_string());
    }
    if root.join(".eslintrc.json").exists() || root.join(".eslintrc.js").exists() {
        return Some("npx eslint .".to_string());
    }
    if root.join("pyproject.toml").exists() {
        return Some("ruff check .".to_string());
    }
    None
}

fn count_files_and_lines(root: &Path) -> (usize, usize) {
    let mut file_count = 0usize;
    let mut total_lines = 0usize;

    if let Ok(entries) = walkdir_lite(root, 1000) {
        for entry in entries {
            if entry.is_file() {
                file_count += 1;
                if let Ok(content) = std::fs::read_to_string(&entry) {
                    total_lines += content.lines().count();
                }
            }
        }
    }

    (file_count, total_lines)
}

fn walkdir_lite(root: &Path, max: usize) -> anyhow::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let ignore = [
        ".git",
        "node_modules",
        "target",
        "__pycache__",
        ".next",
        "dist",
        "build",
    ];

    while let Some(dir) = stack.pop() {
        if results.len() >= max {
            break;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if ignore.contains(&name.as_str()) {
                    continue;
                }

                if path.is_dir() {
                    stack.push(path);
                } else {
                    results.push(path);
                }

                if results.len() >= max {
                    break;
                }
            }
        }
    }

    Ok(results)
}

fn load_gitignore(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(".gitignore"))
        .map(|content| {
            content
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Get a directory tree string for display
pub fn get_dir_tree(root: &Path, max_depth: usize, max_entries: usize) -> String {
    let mut output = String::new();
    build_tree(root, &mut output, 0, max_depth, max_entries, "");
    output
}

fn build_tree(
    dir: &Path,
    output: &mut String,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    prefix: &str,
) {
    if depth >= max_depth || output.len() > 100_000 {
        return;
    }

    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by_key(|e| {
        (
            e.file_type().map(|t| !t.is_dir()).unwrap_or(true),
            e.file_name(),
        )
    });

    let ignore = [".git", "node_modules", "target", "__pycache__"];
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            !ignore.contains(&name.as_str())
        })
        .take(max_entries)
        .collect();

    for (i, entry) in filtered.iter().enumerate() {
        let is_last = i == filtered.len() - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = entry.file_name().to_string_lossy();

        output.push_str(&format!("{}{}{}\n", prefix, connector, name));

        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            build_tree(
                &entry.path(),
                output,
                depth + 1,
                max_depth,
                max_entries,
                &new_prefix,
            );
        }
    }
}
