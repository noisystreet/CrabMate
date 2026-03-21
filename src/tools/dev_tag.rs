//! Development 工具子域标签：按语言栈 / 职责过滤，与 [`super::ToolCategory::Development`] 配合使用。

/// 与工作区、壳命令、编排、元数据等相关，**不绑定**特定语言运行时。
pub const GENERAL: &str = "general";
/// Rust / Cargo 生态工具。
pub const RUST: &str = "rust";
/// 前端（npm/Node）脚本类工具。
pub const FRONTEND: &str = "frontend";
/// Python（ruff/pytest/mypy/pip/uv 等）。
pub const PYTHON: &str = "python";
/// Git 版本控制。
pub const VCS: &str = "vcs";
/// 静态检查、审计、CI 聚合等质量类工具（常与 RUST/FRONTEND 重叠）。
pub const QUALITY: &str = "quality";

/// 返回 `Development` 工具的标签切片；**非 Development 工具名**应返回空（调用方按分类跳过）。
pub fn tags_for_tool_name(name: &str) -> &'static [&'static str] {
    match name {
        // --- 语言无关 / 工作区 ---
        "run_command" | "run_executable" | "workflow_execute" => &[GENERAL],
        "diagnostic_summary" | "changelog_draft" | "license_notice" => &[GENERAL],
        "rust_backtrace_analyze" => &[GENERAL, RUST],
        "create_file"
        | "modify_file"
        | "copy_file"
        | "move_file"
        | "read_file"
        | "read_dir"
        | "glob_files"
        | "list_tree"
        | "file_exists"
        | "read_binary_meta"
        | "hash_file"
        | "extract_in_file"
        | "apply_patch"
        | "search_in_files"
        | "markdown_check_links" => &[GENERAL],
        "structured_validate" | "structured_query" | "structured_diff" => &[GENERAL],

        // --- Git ---
        "git_status" | "git_diff" | "git_clean_check" | "git_diff_stat" | "git_diff_names"
        | "git_log" | "git_show" | "git_diff_base" | "git_blame" | "git_file_history"
        | "git_branch_list" | "git_remote_status" | "git_stage_files" | "git_commit"
        | "git_fetch" | "git_remote_list" | "git_remote_set_url" | "git_apply" | "git_clone" => {
            &[GENERAL, VCS]
        }

        // --- Rust / Cargo ---
        "cargo_metadata"
        | "cargo_tree"
        | "cargo_clean"
        | "cargo_doc"
        | "cargo_run"
        | "cargo_nextest"
        | "cargo_publish_dry_run"
        | "cargo_fix"
        | "rust_test_one"
        | "rust_analyzer_goto_definition"
        | "rust_analyzer_find_references" => &[GENERAL, RUST],
        "cargo_check" | "cargo_test" | "cargo_clippy" | "cargo_fmt_check" | "cargo_outdated"
        | "rust_compiler_json" | "cargo_audit" | "cargo_deny" => &[GENERAL, RUST, QUALITY],
        "find_symbol" | "find_references" | "rust_file_outline" => &[GENERAL, RUST],

        // --- 前端 ---
        "frontend_build" | "frontend_test" => &[GENERAL, FRONTEND],
        "frontend_lint" => &[GENERAL, FRONTEND, QUALITY],

        // --- Python ---
        "ruff_check" | "mypy_check" => &[GENERAL, PYTHON, QUALITY],
        "pytest_run" | "python_install_editable" | "uv_sync" | "uv_run" => &[GENERAL, PYTHON],

        // --- pre-commit（跨语言）---
        "pre_commit_run" => &[GENERAL, QUALITY],

        // --- 质量聚合（跨栈）---
        "ci_pipeline_local" | "release_ready_check" => &[GENERAL, QUALITY],
        "run_lints" | "quality_workspace" => &[GENERAL, RUST, FRONTEND, PYTHON, QUALITY],

        // --- 格式化（多语言由实现按扩展名分流）---
        "format_file" | "format_check_file" => &[GENERAL, RUST, FRONTEND, PYTHON],

        _ => &[GENERAL],
    }
}

/// 根据工作区根目录推测应启用的开发工具标签（去重）。始终包含 `general` 与 `vcs`。
pub fn suggest_dev_tags_for_workspace(root: &std::path::Path) -> Vec<&'static str> {
    let mut out = vec![GENERAL, VCS];
    if root.join("Cargo.toml").is_file() {
        out.push(RUST);
    }
    if root.join("frontend").join("package.json").is_file() || root.join("package.json").is_file() {
        out.push(FRONTEND);
    }
    if root.join("pyproject.toml").is_file()
        || root.join("setup.py").is_file()
        || root.join("setup.cfg").is_file()
        || root.join("requirements.txt").is_file()
    {
        out.push(PYTHON);
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_check_is_rust_and_quality() {
        let t = tags_for_tool_name("cargo_check");
        assert!(t.contains(&RUST) && t.contains(&QUALITY));
    }

    #[test]
    fn git_status_is_vcs() {
        let t = tags_for_tool_name("git_status");
        assert!(t.contains(&VCS));
    }

    #[test]
    fn suggest_tags_includes_python_when_pyproject() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_py_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&PYTHON));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_rust_when_cargo_toml() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_dev_tag_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&RUST));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
