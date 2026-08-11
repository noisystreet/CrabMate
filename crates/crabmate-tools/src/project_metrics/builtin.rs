use std::collections::HashMap;
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use super::{LangStat, WorkspaceCodeStats};

pub fn gather(root: &Path, excluded: &[&str]) -> WorkspaceCodeStats {
    let excluded: HashMap<String, ()> = excluded.iter().map(|d| ((*d).to_string(), ())).collect();
    let mut by_lang: HashMap<String, LangStat> = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if excluded_dir_in_path(path, &excluded) || should_skip_path(path) {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let language = language_from_path(path);
        let lines = content.lines().count();
        let entry = by_lang.entry(language.clone()).or_insert(LangStat {
            language,
            files: 0,
            code: 0,
            comments: 0,
            blanks: 0,
        });
        entry.files += 1;
        entry.code += lines;
    }

    let mut languages: Vec<LangStat> = by_lang.into_values().collect();
    languages.sort_by_key(|l| std::cmp::Reverse(l.code));
    WorkspaceCodeStats { languages }
}

fn excluded_dir_in_path(path: &Path, excluded: &HashMap<String, ()>) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| excluded.contains_key(name))
    })
}

fn path_has_skipped_binary_suffix(lower_path: &str) -> bool {
    const SKIP_SUFFIXES: &[&str] = &[
        ".min.js", ".min.css", ".lock", ".wasm", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico",
        ".woff", ".woff2", ".ttf", ".eot",
    ];
    SKIP_SUFFIXES
        .iter()
        .any(|suffix| lower_path.ends_with(suffix))
}

fn should_skip_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    if name.starts_with('.') {
        return true;
    }
    let lower = path.to_string_lossy().to_lowercase();
    path_has_skipped_binary_suffix(&lower)
}

fn language_from_path(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "Rust".into(),
        "ts" | "tsx" => "TypeScript".into(),
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript".into(),
        "py" | "pyi" => "Python".into(),
        "go" => "Go".into(),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++".into(),
        "c" | "h" => "C".into(),
        "md" | "markdown" => "Markdown".into(),
        "json" => "JSON".into(),
        "toml" => "TOML".into(),
        "yaml" | "yml" => "YAML".into(),
        "css" | "scss" | "sass" => "CSS".into(),
        "html" | "htm" => "HTML".into(),
        "sh" | "bash" | "zsh" => "Shell".into(),
        "sql" => "SQL".into(),
        "java" => "Java".into(),
        "kt" | "kts" => "Kotlin".into(),
        "swift" => "Swift".into(),
        "rb" => "Ruby".into(),
        "php" => "PHP".into(),
        "lua" => "Lua".into(),
        "zig" => "Zig".into(),
        "vue" | "svelte" => ext.to_uppercase(),
        "" => "Other".into(),
        other => other.to_uppercase(),
    }
}
