//! Development 工具子域标签：按语言栈 / 职责过滤，与 [`super::ToolCategory::Development`] 配合使用。

/// 与工作区、壳命令、编排、元数据等相关，**不绑定**特定语言运行时。
pub const GENERAL: &str = "general";
/// Rust / Cargo 生态工具。
pub const RUST: &str = "rust";
/// 前端（npm/Node）脚本类工具。
pub const FRONTEND: &str = "frontend";
/// Python（ruff/pytest/mypy/pip/uv 等）。
pub const PYTHON: &str = "python";
/// C / C++（clang-format、run_command 白名单中的编译/构建工具等）。
pub const CPP: &str = "cpp";
/// Git 版本控制。
pub const VCS: &str = "vcs";
/// 静态检查、审计、CI 聚合等质量类工具（常与 RUST/FRONTEND 重叠）。
pub const QUALITY: &str = "quality";
/// Go 语言工具链。
pub const GO: &str = "go";
/// JVM（Maven / Gradle）。
pub const JVM: &str = "jvm";
/// 安全分析工具（SAST、漏洞扫描）。
pub const SECURITY: &str = "security";
/// Shell 脚本相关工具。
pub const SHELL: &str = "shell";
/// Docker / 容器化相关工具。
pub const DOCKER: &str = "docker";

/// 返回 `Development` 工具的标签切片；**非 Development 工具名**应返回空（调用方按分类跳过）。
pub fn tags_for_tool_name(name: &str) -> &'static [&'static str] {
    match name {
        // --- 语言无关 / 工作区 ---
        "run_command" | "run_executable" | "workflow_execute" => &[GENERAL, CPP],
        "package_query" => &[GENERAL],
        "diagnostic_summary"
        | "error_output_playbook"
        | "playbook_run_commands"
        | "changelog_draft"
        | "license_notice" => &[GENERAL],
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
        | "codebase_semantic_search"
        | "markdown_check_links"
        | "delete_file"
        | "delete_dir"
        | "append_file"
        | "create_dir"
        | "search_replace"
        | "chmod_file"
        | "symlink_info" => &[GENERAL],
        "structured_validate"
        | "structured_query"
        | "structured_diff"
        | "structured_patch"
        | "text_diff"
        | "table_text" => &[GENERAL],

        // --- Git ---
        "git_status" | "git_diff" | "git_clean_check" | "git_diff_stat" | "git_diff_names"
        | "git_log" | "git_show" | "git_diff_base" | "git_blame" | "git_file_history"
        | "git_branch_list" | "git_remote_status" | "git_stage_files" | "git_commit"
        | "git_fetch" | "git_remote_list" | "git_remote_set_url" | "git_apply" | "git_clone"
        | "git_checkout" | "git_branch_create" | "git_branch_delete" | "git_push" | "git_merge"
        | "git_rebase" | "git_stash" | "git_tag" | "git_reset" | "git_cherry_pick"
        | "git_revert" => &[GENERAL, VCS],
        "gh_pr_list" | "gh_pr_view" | "gh_pr_diff" | "gh_issue_list" | "gh_issue_view"
        | "gh_run_list" | "gh_run_view" | "gh_release_list" | "gh_release_view" | "gh_search"
        | "gh_api" => &[GENERAL, VCS],

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
        | "rust_analyzer_find_references"
        | "rust_analyzer_hover"
        | "rust_analyzer_document_symbol" => &[GENERAL, RUST],
        "cargo_check" | "cargo_test" | "cargo_clippy" | "cargo_fmt_check" | "cargo_outdated"
        | "cargo_machete" | "cargo_udeps" | "rust_compiler_json" | "cargo_audit" | "cargo_deny" => {
            &[GENERAL, RUST, QUALITY]
        }
        "find_symbol" | "find_references" | "rust_file_outline" | "call_graph_sketch" => {
            &[GENERAL, RUST]
        }

        // --- 前端 / Node.js ---
        "frontend_build" | "frontend_test" => &[GENERAL, FRONTEND],
        "frontend_lint" => &[GENERAL, FRONTEND, QUALITY],
        "npm_install" | "npm_run" | "npx_run" => &[GENERAL, FRONTEND],
        "tsc_check" => &[GENERAL, FRONTEND, QUALITY],

        // --- Go ---
        "go_build" | "go_test" | "go_mod_tidy" => &[GENERAL, GO],
        "go_vet" | "go_fmt_check" | "golangci_lint" => &[GENERAL, GO, QUALITY],

        // --- JVM ---
        "maven_compile" | "maven_test" | "gradle_compile" | "gradle_test" => {
            &[GENERAL, JVM, QUALITY]
        }

        // --- Python ---
        "ruff_check" | "mypy_check" => &[GENERAL, PYTHON, QUALITY],
        "pytest_run" | "python_install_editable" | "uv_sync" | "uv_run" | "python_snippet_run" => {
            &[GENERAL, PYTHON]
        }

        // --- pre-commit（跨语言）---
        "pre_commit_run" => &[GENERAL, QUALITY],
        "typos_check" | "codespell_check" | "ast_grep_run" | "ast_grep_rewrite" => {
            &[GENERAL, QUALITY]
        }

        // --- TODO/标记扫描 ---
        "todo_scan" => &[GENERAL, QUALITY],

        // --- 源码分析工具 ---
        "shellcheck_check" => &[GENERAL, SHELL, QUALITY],
        "cppcheck_analyze" => &[GENERAL, CPP, QUALITY],
        "semgrep_scan" => &[GENERAL, SECURITY, QUALITY],
        "hadolint_check" => &[GENERAL, DOCKER, QUALITY],
        "docker_build" | "docker_compose_ps" | "podman_images" => &[GENERAL, DOCKER, QUALITY],
        "bandit_scan" => &[GENERAL, PYTHON, SECURITY, QUALITY],
        "lizard_complexity" => &[GENERAL, QUALITY],

        // --- 质量聚合（跨栈）---
        "ci_pipeline_local"
        | "release_ready_check"
        | "repo_overview_sweep"
        | "docs_health_sweep" => &[GENERAL, QUALITY],
        "run_lints" => &[GENERAL, RUST, FRONTEND, PYTHON, QUALITY],
        "quality_workspace" => &[GENERAL, RUST, FRONTEND, PYTHON, JVM, DOCKER, QUALITY],

        // --- 格式化（多语言由实现按扩展名分流）---
        "format_file" | "format_check_file" => &[GENERAL, RUST, FRONTEND, PYTHON, CPP],

        // --- 进程与端口 ---
        "port_check" | "process_list" => &[GENERAL],

        // --- 代码度量与分析 ---
        "code_stats" => &[GENERAL],
        "dependency_graph" => &[GENERAL, RUST, FRONTEND, GO, JVM],
        "coverage_report" => &[GENERAL, RUST, FRONTEND, PYTHON, GO, QUALITY],

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
    if root.join("CMakeLists.txt").is_file()
        || root.join("Makefile").is_file()
        || root.join("meson.build").is_file()
        || root.join("configure.ac").is_file()
        || root.join("configure.in").is_file()
    {
        out.push(CPP);
    }
    if root.join("go.mod").is_file() {
        out.push(GO);
    }
    if root.join("pom.xml").is_file()
        || root.join("build.gradle").is_file()
        || root.join("build.gradle.kts").is_file()
        || root.join("settings.gradle").is_file()
        || root.join("settings.gradle.kts").is_file()
    {
        out.push(JVM);
    }
    if root.join("Dockerfile").is_file()
        || root.join("docker-compose.yml").is_file()
        || root.join("docker-compose.yaml").is_file()
        || root.join(".dockerignore").is_file()
    {
        out.push(DOCKER);
    }
    let has_shell_scripts = root.join("scripts").is_dir()
        || std::fs::read_dir(root)
            .ok()
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| matches!(ext, "sh" | "bash"))
                })
            })
            .unwrap_or(false);
    if has_shell_scripts {
        out.push(SHELL);
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
    fn package_query_is_general() {
        let t = tags_for_tool_name("package_query");
        assert!(t.contains(&GENERAL));
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

    #[test]
    fn suggest_tags_includes_cpp_when_configure_ac() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_ac_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("configure.ac"), "AC_INIT([x],[1.0])\n").unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&CPP));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_jvm_when_pom() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_jvm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pom.xml"), "<project/>").unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&JVM));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggest_tags_includes_cpp_when_cmake() {
        let dir = std::env::temp_dir().join(format!("crabmate_dev_tag_cpp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.16)\n",
        )
        .unwrap();
        let s = suggest_dev_tags_for_workspace(&dir);
        assert!(s.contains(&CPP));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_file_tag_includes_cpp() {
        let t = tags_for_tool_name("format_file");
        assert!(t.contains(&CPP));
    }
}
