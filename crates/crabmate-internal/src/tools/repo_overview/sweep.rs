//! 仓库概览 sweep 实现与测试。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::context_bootstrap::project_profile;

use super::parse::RepoSweepParams;
use crate::tools::ToolContext;
use crate::tools::file::{canonical_workspace_root, glob_files, list_tree, read_file};
use crate::tools::output_util::truncate_output_bytes;
use crate::workspace::path::{
    absolutize_relative_under_root, ensure_existing_ancestor_within_root,
};

enum RelPathCheck {
    Ok(PathBuf),
    Invalid,
    OutOfBounds,
}

fn classify_rel_path(workspace_root: &Path, rel: &str) -> RelPathCheck {
    let rel = rel.trim();
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.contains("..") {
        return RelPathCheck::Invalid;
    }
    let ws = match canonical_workspace_root(workspace_root) {
        Ok(p) => p,
        Err(_) => return RelPathCheck::Invalid,
    };
    let normalized = match absolutize_relative_under_root(&ws, rel) {
        Ok(p) => p,
        Err(e) if e.is_policy_denied() => return RelPathCheck::OutOfBounds,
        Err(_) => return RelPathCheck::Invalid,
    };
    if ensure_existing_ancestor_within_root(&ws, &normalized).is_err() {
        return RelPathCheck::OutOfBounds;
    }
    RelPathCheck::Ok(normalized)
}

fn rel_path_safe(workspace_root: &Path, rel: &str) -> Option<PathBuf> {
    match classify_rel_path(workspace_root, rel) {
        RelPathCheck::Ok(p) => Some(p),
        _ => None,
    }
}

fn tool_ctx_stub<'a>(workspace_root: &'a Path, max_output_len: usize) -> ToolContext<'a> {
    ToolContext {
        cfg: None,
        codebase_semantic: None,
        command_max_output_len: max_output_len,
        weather_timeout_secs: 0,
        allowed_commands: &[],
        working_dir: workspace_root,
        web_search_timeout_secs: 0,
        web_search_provider: crabmate_config::WebSearchProvider::Brave,
        web_search_api_key: "",
        web_search_max_results: 0,
        http_fetch_allowed_prefixes: &[],
        http_fetch_timeout_secs: 0,
        http_fetch_max_response_bytes: 0,
        command_timeout_secs: 30,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 8,
        long_term_memory: None,
        long_term_memory_scope_id: None,
    }
}

fn sweep_append_project_profile(
    out: &mut String,
    workspace_root: &Path,
    section: &mut usize,
    params: &RepoSweepParams,
) {
    if !params.include_project_profile || params.project_profile_max_chars == 0 {
        return;
    }
    out.push_str(&format!("## {section}) 项目画像（自动生成，只读扫描）\n\n"));
    out.push_str(&project_profile::build_project_profile_markdown(
        workspace_root,
        params.project_profile_max_chars,
    ));
    out.push_str("\n---\n\n");
    *section += 1;
}

fn sweep_append_doc_previews(
    out: &mut String,
    workspace_root: &Path,
    ctx: &ToolContext<'_>,
    section: &mut usize,
    params: &RepoSweepParams,
) {
    out.push_str(&format!("## {section}) 主文档预览（每文件前若干行）\n\n"));
    *section += 1;
    for rel in &params.doc_paths {
        let Some(canon) = rel_path_safe(workspace_root, rel) else {
            let msg = match classify_rel_path(workspace_root, rel) {
                RelPathCheck::OutOfBounds => format!("- `{}`：跳过（路径越界）\n", rel),
                RelPathCheck::Invalid | RelPathCheck::Ok(_) => {
                    format!("- `{}`：跳过（路径非法）\n", rel)
                }
            };
            out.push_str(&msg);
            continue;
        };
        if !canon.is_file() {
            out.push_str(&format!("- `{}`：不存在或非文件，跳过\n", rel));
            continue;
        }
        let args = serde_json::json!({
            "path": rel,
            "start_line": 1,
            "max_lines": params.doc_preview_max_lines,
            "encoding": "utf-8"
        });
        let args_s = match serde_json::to_string(&args) {
            Ok(s) => s,
            Err(e) => {
                out.push_str(&format!("- `{}`：序列化 read_file 参数失败：{}\n", rel, e));
                continue;
            }
        };
        let body = read_file(&args_s, workspace_root, ctx);
        out.push_str(&format!("### `{}`\n", rel));
        out.push_str(&body);
        out.push_str("\n---\n\n");
    }
}

fn sweep_append_source_trees(
    out: &mut String,
    workspace_root: &Path,
    section: &mut usize,
    params: &RepoSweepParams,
) {
    out.push_str(&format!("## {section}) 源码/结构目录树（list_tree）\n\n"));
    *section += 1;
    for root in &params.source_roots {
        let Some(canon) = rel_path_safe(workspace_root, root) else {
            let msg = match classify_rel_path(workspace_root, root) {
                RelPathCheck::OutOfBounds => format!("### `{}`\n路径越界，跳过\n\n", root),
                RelPathCheck::Invalid | RelPathCheck::Ok(_) => {
                    format!("### `{}`\n路径非法，跳过\n\n", root)
                }
            };
            out.push_str(&msg);
            continue;
        };
        if !canon.is_dir() {
            out.push_str(&format!("### `{}`\n目录不存在，跳过\n\n", root));
            continue;
        }
        let tree_args = serde_json::json!({
            "path": root,
            "max_depth": params.list_tree_max_depth,
            "max_entries": params.list_tree_max_entries,
            "include_hidden": params.list_include_hidden
        });
        let tree_s = match serde_json::to_string(&tree_args) {
            Ok(s) => s,
            Err(e) => {
                out.push_str(&format!("### `{}`\n序列化 list_tree 失败：{}\n\n", root, e));
                continue;
            }
        };
        out.push_str(&format!("### 根：`{}`\n", root));
        out.push_str(&list_tree(&tree_s, workspace_root));
        out.push_str("\n\n");
    }
}

fn sweep_collect_build_paths(workspace_root: &Path, params: &RepoSweepParams) -> BTreeSet<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for pattern in &params.build_globs {
        let gargs = serde_json::json!({
            "pattern": pattern,
            "path": ".",
            "max_depth": params.build_glob_max_depth,
            "max_results": params.build_glob_max_results,
            "include_hidden": false
        });
        let gstr = match serde_json::to_string(&gargs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let block = glob_files(&gstr, workspace_root);
        let mut after_first_sep = false;
        for line in block.lines() {
            let t = line.trim();
            if t == "---" {
                if !after_first_sep {
                    after_first_sep = true;
                } else {
                    break;
                }
                continue;
            }
            if !after_first_sep {
                continue;
            }
            if t.starts_with("匹配 ") {
                break;
            }
            if !t.is_empty() {
                found.insert(t.to_string());
            }
        }
    }
    found
}

fn sweep_append_build_section(
    out: &mut String,
    workspace_root: &Path,
    section: &mut usize,
    params: &RepoSweepParams,
) {
    out.push_str(&format!(
        "## {section}) 构建与清单文件（glob 汇总，去重排序）\n\n"
    ));
    *section += 1;
    let found = sweep_collect_build_paths(workspace_root, params);
    if found.is_empty() {
        out.push_str("（未匹配到常见构建/清单文件；可检查 build_globs 或仓库布局。）\n\n");
    } else {
        for p in found.iter().take(500) {
            out.push_str(p);
            out.push('\n');
        }
        if found.len() > 500 {
            out.push_str(&format!("\n… 共 {} 条路径，仅列出前 500 条\n", found.len()));
        }
        out.push('\n');
    }
}

fn sweep_append_conclusion_prompt(out: &mut String, section: usize) {
    out.push_str(&format!(
        "## {section}) 请模型据此撰写的分析结论（提纲）\n\n"
    ));
    out.push_str(
        "请用自然语言输出结构化结论，建议包含：\n\
         - **项目定位**：本仓库解决什么问题、主要用户/运行形态（依据 README/AGENTS 等）。\n\
         - **技术栈与入口**：语言、主程序入口、Web/CLI 等（依据文档与目录树）。\n\
         - **模块/分层**：`src/`（或其它根）下大致分层与职责（依据树与 DEVELOPMENT 等）。\n\
         - **构建与质量**：如何从清单文件推断构建、测试、CI（依据第 3 节路径 + 文档）。\n\
         - **风险与缺口**：文档未覆盖处、单测/类型检查是否可从路径推断、后续建议深挖路径。\n\n",
    );
}

/// 只读：汇总文档头、目录树与构建相关路径，便于模型一次性获得仓库骨架上下文。
pub fn repo_overview_sweep(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let params = RepoSweepParams::from_json(&v);
    let ctx = tool_ctx_stub(workspace_root, max_output_len);

    let mut out = String::new();
    out.push_str("=== repo_overview_sweep（只读聚合）===\n");
    out.push_str("说明：下列为自动收集的仓库骨架材料；**分析结论须由模型根据本节内容在对话中撰写**，本工具不代替推理。\n\n");

    let mut section = 1usize;
    sweep_append_project_profile(&mut out, workspace_root, &mut section, &params);
    sweep_append_doc_previews(&mut out, workspace_root, &ctx, &mut section, &params);
    sweep_append_source_trees(&mut out, workspace_root, &mut section, &params);
    sweep_append_build_section(&mut out, workspace_root, &mut section, &params);
    sweep_append_conclusion_prompt(&mut out, section);

    truncate_output_bytes(&out, max_output_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sweep_minimal_workspace() {
        let root =
            std::env::temp_dir().join(format!("crabmate_repo_overview_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(root.join("README.md"), "# T\n\nhello\n").expect("write");
        fs::write(root.join("src/lib.rs"), "// x\n").expect("write");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .expect("write");

        let out = repo_overview_sweep("{}", &root, 50_000);
        let _ = fs::remove_dir_all(&root);

        assert!(out.contains("README.md"));
        assert!(out.contains("hello"));
        assert!(out.contains("Cargo.toml"));
        assert!(out.contains("repo_overview_sweep"));
        assert!(out.contains("请模型据此撰写"));
        assert!(out.contains("项目画像"));
        assert!(out.contains("工程类型") || out.contains("Rust"));

        let root2 =
            std::env::temp_dir().join(format!("crabmate_repo_overview2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root2);
        fs::create_dir_all(root2.join("src")).expect("mkdir");
        fs::write(root2.join("README.md"), "# T\n").expect("write");
        fs::write(root2.join("src/lib.rs"), "// x\n").expect("write");
        fs::write(
            root2.join("Cargo.toml"),
            "[package]\nname = \"y\"\nversion = \"0.1.0\"\n",
        )
        .expect("write");
        let out2 = repo_overview_sweep(r#"{"include_project_profile":false}"#, &root2, 50_000);
        let _ = fs::remove_dir_all(&root2);
        assert!(!out2.contains("CrabMate 项目画像"));
    }

    #[test]
    fn sweep_reports_missing_paths_without_out_of_bounds_label() {
        let root = std::env::temp_dir().join(format!(
            "crabmate_repo_overview_missing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("README.md"), "# T\n").expect("write");

        let out = repo_overview_sweep(
            r#"{"doc_paths":["AGENTS.md"],"source_roots":["src"],"include_project_profile":false}"#,
            &root,
            50_000,
        );
        let _ = fs::remove_dir_all(&root);

        assert!(out.contains("AGENTS.md") && out.contains("不存在"));
        assert!(out.contains("src") && out.contains("目录不存在"));
        assert!(!out.contains("路径非法或越界"));
        assert!(!out.contains("路径越界"));
    }
}
