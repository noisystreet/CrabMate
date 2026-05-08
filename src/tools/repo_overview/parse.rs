//! `repo_overview_sweep` JSON 参数解析。

use serde_json::Value;

pub(crate) struct RepoSweepParams {
    pub doc_paths: Vec<String>,
    pub source_roots: Vec<String>,
    pub build_globs: Vec<String>,
    pub doc_preview_max_lines: usize,
    pub list_tree_max_depth: usize,
    pub list_tree_max_entries: usize,
    pub list_include_hidden: bool,
    pub build_glob_max_results: usize,
    pub build_glob_max_depth: usize,
    pub include_project_profile: bool,
    pub project_profile_max_chars: usize,
}

fn default_source_roots() -> Vec<String> {
    vec!["src".to_string()]
}

fn default_build_globs() -> Vec<String> {
    vec![
        "**/Cargo.toml".to_string(),
        "**/package.json".to_string(),
        "**/Makefile".to_string(),
        "**/CMakeLists.txt".to_string(),
        "**/build.gradle".to_string(),
        "**/build.gradle.kts".to_string(),
        "**/pyproject.toml".to_string(),
        "**/go.mod".to_string(),
        "**/.pre-commit-config.yaml".to_string(),
        "**/.github/workflows/*.yml".to_string(),
    ]
}

fn non_empty_trimmed_strings(arr: &[Value]) -> Vec<String> {
    arr.iter()
        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

fn string_list_or(v: &Value, key: &str, default: impl FnOnce() -> Vec<String>) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|arr| non_empty_trimmed_strings(arr))
        .filter(|x: &Vec<String>| !x.is_empty())
        .unwrap_or_else(default)
}

fn u64_clamped_field(v: &Value, key: &str, default: u64, lo: u64, hi: u64) -> usize {
    v.get(key)
        .and_then(|n| n.as_u64())
        .unwrap_or(default)
        .clamp(lo, hi) as usize
}

impl RepoSweepParams {
    pub(crate) fn from_json(v: &Value) -> Self {
        Self {
            doc_paths: string_list_or(v, "doc_paths", super::default_health_sweep_doc_paths),
            source_roots: string_list_or(v, "source_roots", default_source_roots),
            build_globs: string_list_or(v, "build_globs", default_build_globs),
            doc_preview_max_lines: u64_clamped_field(v, "doc_preview_max_lines", 80, 10, 500),
            list_tree_max_depth: u64_clamped_field(v, "list_tree_max_depth", 4, 1, 20),
            list_tree_max_entries: u64_clamped_field(v, "list_tree_max_entries", 400, 50, 5000),
            list_include_hidden: v
                .get("list_tree_include_hidden")
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            build_glob_max_results: u64_clamped_field(v, "build_glob_max_results", 120, 10, 2000),
            build_glob_max_depth: u64_clamped_field(v, "build_glob_max_depth", 25, 1, 100),
            include_project_profile: v
                .get("include_project_profile")
                .and_then(|b| b.as_bool())
                .unwrap_or(true),
            project_profile_max_chars: u64_clamped_field(
                v,
                "project_profile_max_chars",
                6_000,
                0,
                50_000,
            ),
        }
    }
}
