//! 文档与健康聚合：主文档预览 + typos + codespell + Markdown 链接检查（只读）。

use std::path::Path;

use super::ToolContext;
use super::file::read_file;
use super::output_util::truncate_output_bytes;
use super::repo_overview;
use super::spell_astgrep_tools;

fn default_doc_preview_paths() -> Vec<String> {
    repo_overview::default_health_sweep_doc_paths()
}

fn spell_tool_failed(block: &str) -> bool {
    if block.contains("无法启动") {
        return false;
    }
    let Some(first) = block.lines().next() else {
        return false;
    };
    if first.contains("(exit=0)") {
        return false;
    }
    first.contains("(exit=")
}

fn markdown_links_failed(block: &str) -> bool {
    block.contains("结论: 发现")
}

/// 只读聚合：文档头预览、`typos_check`、`codespell_check`、`markdown_check_links`。
///
/// **外链探测**：`markdown_check_links` 在 `allowed_external_prefixes` 非空时使用内置 HTTP 客户端发 HEAD，
/// **不经过** `http_fetch` / `http_request` 工具与 `http_fetch_allowed_prefixes`，也**无** Web/CLI 审批会话。
pub fn docs_health_sweep(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let run_doc_preview = v
        .get("run_doc_preview")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let run_typos = v.get("run_typos").and_then(|x| x.as_bool()).unwrap_or(true);
    let run_codespell = v
        .get("run_codespell")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let run_markdown_links = v
        .get("run_markdown_links")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    let fail_fast = v
        .get("fail_fast")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let summary_only = v
        .get("summary_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let doc_preview_max_lines = v
        .get("doc_preview_max_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(60)
        .clamp(10, 200);

    let doc_paths: Vec<String> = v
        .get("doc_paths")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .filter(|x: &Vec<String>| !x.is_empty())
        .unwrap_or_else(default_doc_preview_paths);

    let ctx = ToolContext {
        codebase_semantic: None,
        command_max_output_len: max_output_len,
        weather_timeout_secs: 0,
        allowed_commands: &[],
        working_dir: workspace_root,
        web_search_timeout_secs: 0,
        web_search_provider: crate::config::WebSearchProvider::Brave,
        web_search_api_key: "",
        web_search_max_results: 0,
        http_fetch_allowed_prefixes: &[],
        http_fetch_timeout_secs: 0,
        http_fetch_max_response_bytes: 0,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 8,
    };

    let mut sections: Vec<String> = Vec::new();
    let mut summary: Vec<(String, String)> = Vec::new();

    let push_skipped_after_typos = |summary: &mut Vec<(String, String)>| {
        if run_codespell {
            summary.push(("codespell_check".to_string(), "skipped".to_string()));
        }
        if run_markdown_links {
            summary.push(("markdown_check_links".to_string(), "skipped".to_string()));
        }
    };

    sections.push(
        "=== docs_health_sweep（只读）===\n\
         说明：Markdown 外链 HEAD 探测由 markdown_check_links 内置 HTTP 发起，不经过 http_fetch 白名单与审批；\
         仅当 md_allowed_external_prefixes 非空时才会请求外网。\n"
            .to_string(),
    );

    if run_doc_preview {
        sections.push("## 1) 主文档预览\n".to_string());
        if !summary_only {
            for rel in &doc_paths {
                let joined = workspace_root.join(rel);
                if !joined.is_file() {
                    sections.push(format!("- `{}`：不存在或非文件，跳过\n", rel));
                    continue;
                }
                let args = serde_json::json!({
                    "path": rel,
                    "start_line": 1,
                    "max_lines": doc_preview_max_lines,
                    "encoding": "utf-8"
                });
                let args_s = match serde_json::to_string(&args) {
                    Ok(s) => s,
                    Err(e) => {
                        sections.push(format!("- `{}`：序列化失败：{}\n", rel, e));
                        continue;
                    }
                };
                sections.push(format!("### `{}`\n", rel));
                sections.push(read_file(&args_s, workspace_root, &ctx));
                sections.push("\n---\n".to_string());
            }
        }
        summary.push(("doc_preview".to_string(), "done".to_string()));
    } else {
        summary.push(("doc_preview".to_string(), "skipped".to_string()));
    }

    let spell_paths = v.get("spell_paths").cloned();
    let typos_extra = {
        let mut o = serde_json::Map::new();
        if let Some(p) = spell_paths.clone() {
            o.insert("paths".to_string(), p);
        }
        if let Some(s) = v.get("typos_config_path").and_then(|x| x.as_str()) {
            o.insert(
                "config_path".to_string(),
                serde_json::Value::String(s.to_string()),
            );
        }
        serde_json::Value::Object(o)
    };
    let typos_args = match serde_json::to_string(&typos_extra) {
        Ok(s) => s,
        Err(e) => return format!("typos 参数序列化失败：{}", e),
    };

    if run_typos {
        let r = spell_astgrep_tools::typos_check(&typos_args, workspace_root, max_output_len);
        let failed = spell_tool_failed(&r);
        summary.push((
            "typos_check".to_string(),
            if r.contains("无法启动") {
                "skipped".to_string()
            } else if failed {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        ));
        if !summary_only {
            sections.push("## 2) typos_check\n\n".to_string());
            sections.push(r);
            sections.push("\n\n".to_string());
        }
        if fail_fast && failed {
            push_skipped_after_typos(&mut summary);
            return build_output(&summary, &sections, summary_only, max_output_len, true);
        }
    } else {
        summary.push(("typos_check".to_string(), "skipped".to_string()));
    }

    let codespell_extra = {
        let mut o = serde_json::Map::new();
        if let Some(p) = spell_paths {
            o.insert("paths".to_string(), p);
        }
        if let Some(s) = v.get("codespell_skip").and_then(|x| x.as_str()) {
            o.insert("skip".to_string(), serde_json::Value::String(s.to_string()));
        }
        if let Some(a) = v
            .get("codespell_dictionary_paths")
            .and_then(|x| x.as_array())
        {
            o.insert(
                "dictionary_paths".to_string(),
                serde_json::Value::Array(a.clone()),
            );
        }
        if let Some(s) = v
            .get("codespell_ignore_words_list")
            .and_then(|x| x.as_str())
        {
            o.insert(
                "ignore_words_list".to_string(),
                serde_json::Value::String(s.to_string()),
            );
        }
        serde_json::Value::Object(o)
    };
    let codespell_args = match serde_json::to_string(&codespell_extra) {
        Ok(s) => s,
        Err(e) => return format!("codespell 参数序列化失败：{}", e),
    };

    if run_codespell {
        let r =
            spell_astgrep_tools::codespell_check(&codespell_args, workspace_root, max_output_len);
        let failed = spell_tool_failed(&r);
        summary.push((
            "codespell_check".to_string(),
            if r.contains("无法启动") {
                "skipped".to_string()
            } else if failed {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        ));
        if !summary_only {
            sections.push("## 3) codespell_check\n\n".to_string());
            sections.push(r);
            sections.push("\n\n".to_string());
        }
        if fail_fast && failed {
            if run_markdown_links {
                summary.push(("markdown_check_links".to_string(), "skipped".to_string()));
            }
            return build_output(&summary, &sections, summary_only, max_output_len, true);
        }
    } else {
        summary.push(("codespell_check".to_string(), "skipped".to_string()));
    }

    let md_obj = {
        let mut o = serde_json::Map::new();
        if let Some(a) = v.get("md_roots").and_then(|x| x.as_array())
            && !a.is_empty()
        {
            o.insert("roots".to_string(), serde_json::Value::Array(a.clone()));
        }
        if let Some(n) = v.get("md_max_files").and_then(|x| x.as_u64()) {
            o.insert("max_files".to_string(), serde_json::Value::from(n));
        }
        if let Some(n) = v.get("md_max_depth").and_then(|x| x.as_u64()) {
            o.insert("max_depth".to_string(), serde_json::Value::from(n));
        }
        if let Some(a) = v
            .get("md_allowed_external_prefixes")
            .and_then(|x| x.as_array())
        {
            o.insert(
                "allowed_external_prefixes".to_string(),
                serde_json::Value::Array(a.clone()),
            );
        }
        if let Some(n) = v.get("md_external_timeout_secs").and_then(|x| x.as_u64()) {
            o.insert(
                "external_timeout_secs".to_string(),
                serde_json::Value::from(n),
            );
        }
        if let Some(b) = v.get("md_check_fragments").and_then(|x| x.as_bool()) {
            o.insert("check_fragments".to_string(), serde_json::Value::Bool(b));
        }
        if let Some(s) = v.get("md_output_format").and_then(|x| x.as_str()) {
            o.insert(
                "output_format".to_string(),
                serde_json::Value::String(s.to_string()),
            );
        }
        serde_json::Value::Object(o)
    };
    let md_args = match serde_json::to_string(&md_obj) {
        Ok(s) => s,
        Err(e) => return format!("markdown_check_links 参数序列化失败：{}", e),
    };

    if run_markdown_links {
        let r = super::markdown_links::markdown_check_links(&md_args, workspace_root);
        let failed = markdown_links_failed(&r);
        summary.push((
            "markdown_check_links".to_string(),
            if failed {
                "failed".to_string()
            } else {
                "passed".to_string()
            },
        ));
        if !summary_only {
            sections.push("## 4) markdown_check_links\n\n".to_string());
            sections.push(r);
            sections.push("\n".to_string());
        }
    } else {
        summary.push(("markdown_check_links".to_string(), "skipped".to_string()));
    }

    let any_failed = summary.iter().any(|(_, s)| s == "failed");
    build_output(
        &summary,
        &sections,
        summary_only,
        max_output_len,
        any_failed,
    )
}

fn build_output(
    summary: &[(String, String)],
    sections: &[String],
    summary_only: bool,
    max_output_len: usize,
    any_failed: bool,
) -> String {
    let mut out = String::new();
    out.push_str("### 步骤汇总\n");
    for (name, st) in summary {
        out.push_str(&format!("- {}: {}\n", name, st));
    }
    out.push_str(&format!(
        "\n整体: {}\n\n",
        if any_failed {
            "存在失败项（见上）"
        } else {
            "未发现失败项（CLI 未安装的步骤记为 skipped）"
        }
    ));
    if !summary_only {
        for s in sections {
            out.push_str(s);
        }
    }
    truncate_output_bytes(&out, max_output_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sweep_readme_only_markdown() {
        let root =
            std::env::temp_dir().join(format!("crabmate_docs_health_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("README.md"), "# Hi\n\n[me](./README.md)\n").expect("w");

        let arg = r#"{"run_doc_preview":false,"run_typos":false,"run_codespell":false,"run_markdown_links":true,"md_roots":["README.md"]}"#;
        let out = docs_health_sweep(arg, &root, 80_000);
        let _ = fs::remove_dir_all(&root);

        assert!(out.contains("docs_health_sweep"));
        assert!(out.contains("markdown_check_links"));
        assert!(out.contains("passed") || out.contains("未发现"));
    }
}
