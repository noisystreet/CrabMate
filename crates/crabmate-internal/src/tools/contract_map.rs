//! 仓库内「契约与入口」只读地图：HTTP/SSE、前后端对齐锚点、默认配置入口等，供模型少猜路径。
//!
//! 不调用外部命令、不联网；仅在工作区内 `read` 有限行数的文本文件并做轻量行筛选。

use std::fs;
use std::path::{Path, PathBuf};

use super::ToolContext;
use super::file::canonical_workspace_root;
use super::output_util::truncate_output_bytes;

fn rel_path_ok(workspace_root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() || Path::new(rel).is_absolute() || rel.contains("..") {
        return None;
    }
    let joined = workspace_root.join(rel);
    let ws = canonical_workspace_root(workspace_root).ok()?;
    let canon = joined.canonicalize().ok()?;
    if !canon.starts_with(&ws) {
        return None;
    }
    Some(canon)
}

fn read_head_lines(path: &Path, max_lines: usize) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let take = max_lines.max(1);
    let mut out = String::new();
    for (i, line) in raw.lines().take(take).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    Some(out)
}

fn collect_matching_lines(path: &Path, keywords: &[&str], max_hits: usize) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let mut hits = Vec::new();
    for line in raw.lines() {
        if keywords.iter().any(|k| line.contains(k)) {
            hits.push(line.trim_end());
            if hits.len() >= max_hits {
                break;
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(hits.join("\n"))
}

fn section_file(
    workspace_root: &Path,
    title: &str,
    rel: &str,
    head_lines: usize,
    keywords: &[&str],
    keyword_hits: usize,
    out: &mut String,
) {
    out.push_str(&format!("### {title}\n"));
    out.push_str(&format!("路径：`{rel}`\n\n"));
    let Some(canon) = rel_path_ok(workspace_root, rel) else {
        out.push_str("（跳过：路径非法、不存在或越出工作区。）\n\n");
        return;
    };
    if !canon.is_file() {
        out.push_str("（跳过：非文件或不存在。）\n\n");
        return;
    }
    if let Some(head) = read_head_lines(&canon, head_lines) {
        out.push_str("**文件头预览**\n```\n");
        out.push_str(&head);
        out.push_str("\n```\n\n");
    }
    if !keywords.is_empty() {
        if let Some(block) = collect_matching_lines(&canon, keywords, keyword_hits) {
            out.push_str("**关键字匹配行（节选）**\n```\n");
            out.push_str(&block);
            out.push_str("\n```\n\n");
        } else {
            out.push_str("**关键字匹配行**：无匹配（可扩大检索或手工 `read_file`）。\n\n");
        }
    }
}

/// 只读：输出 Markdown 形式的「契约地图」，锚定 HTTP/SSE/配置/金样等路径，降低模型幻觉。
pub fn crate_contract_map(args_json: &str, ctx: &ToolContext<'_>) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let head_lines = v
        .get("head_lines_per_file")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(24)
        .clamp(5, 120);

    let keyword_hits = v
        .get("keyword_hits_per_file")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(40)
        .clamp(5, 200);

    let extra_paths: Vec<String> = v
        .get("extra_paths")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let max_extra = v
        .get("max_extra_paths")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(12)
        .clamp(0, 40);

    let mut out = String::new();
    out.push_str("=== crate_contract_map（只读契约地图）===\n\n");
    out.push_str("schema: `crabmate_contract_map_v1`\n\n");
    out.push_str(
        "说明：本节仅列出**工作区内**与 HTTP/SSE/配置/前后端对齐相关的**锚点文件**及短预览；\
         **不包含密钥**；**不调用**外部命令。复杂改动请继续 `read_file` / `search_in_files` / `repo_overview_sweep`。\n\n",
    );

    out.push_str("## 1) 后端路由与入口（Rust）\n\n");
    section_file(
        ctx.working_dir,
        "Axum 入口与路由（常见挂载点）",
        "src/lib.rs",
        head_lines,
        &["Router", ".route(", "merge(", "/chat", "serve"],
        keyword_hits,
        &mut out,
    );

    out.push_str("## 2) SSE / 聊天协议（双端对齐）\n\n");
    section_file(
        ctx.working_dir,
        "SSE 协议文档（权威）",
        "docs/SSE协议.md",
        head_lines,
        &["错误", "code", "SSE", "协议"],
        keyword_hits.min(30),
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "Leptos 前端 API 与流式请求",
        "frontend/src/api/mod.rs",
        head_lines,
        &["chat", "stream", "SSE", "/chat"],
        keyword_hits,
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "Leptos SSE 控制面分发",
        "frontend/src/sse_dispatch/dispatch.rs",
        head_lines,
        &["dispatch", "staged_plan", "Handled", "control"],
        keyword_hits,
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "Rust 控制面分类（与前端分支顺序同源）",
        "crates/crabmate-sse-protocol/control_classify.rs",
        head_lines,
        &["classify", "handled", "plain", "stop"],
        keyword_hits.min(40),
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "SSE 控制面金样（回归）",
        "fixtures/sse_control_golden.jsonl",
        head_lines.min(40),
        &["plain", "handled", "stop"],
        20,
        &mut out,
    );

    out.push_str("## 3) 配置与运维入口\n\n");
    section_file(
        ctx.working_dir,
        "默认 Agent 配置（键入口）",
        "config/default_config.toml",
        head_lines,
        &["[", "staged", "tool", "web_", "llm_"],
        keyword_hits,
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "配置说明（用户可见键与 CM_ 变量）",
        "docs/配置说明.md",
        head_lines,
        &["CM_", "环境", "变量", "默认"],
        keyword_hits.min(35),
        &mut out,
    );
    section_file(
        ctx.working_dir,
        "命令行与子命令 / 路由索引",
        "docs/命令行与路由.md",
        head_lines,
        &["serve", "POST", "GET", "/chat"],
        keyword_hits.min(35),
        &mut out,
    );

    if !extra_paths.is_empty() {
        out.push_str("## 4) 调用方附加路径\n\n");
        for rel in extra_paths.iter().take(max_extra) {
            section_file(
                ctx.working_dir,
                rel.as_str(),
                rel.as_str(),
                head_lines,
                &[],
                0,
                &mut out,
            );
        }
        if extra_paths.len() > max_extra {
            out.push_str(&format!(
                "\n（仅处理前 {} 条 `extra_paths`，共 {} 条。）\n",
                max_extra,
                extra_paths.len()
            ));
        }
    }

    truncate_output_bytes(&out, ctx.command_max_output_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn contract_map_minimal_workspace() {
        let root =
            std::env::temp_dir().join(format!("crabmate_contract_map_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::write(
            root.join("src/lib.rs"),
            "use axum::Router;\nfn x() { let _ = Router::new().route(\"/chat\", get(|| async {})); }\n",
        )
        .expect("write");
        fs::create_dir_all(root.join("docs")).expect("mkdir");
        fs::write(root.join("docs/SSE协议.md"), "# SSE\n\n错误码 LLM_\n").expect("write");
        fs::create_dir_all(root.join("config")).expect("mkdir");
        fs::write(root.join("config/default_config.toml"), "[agent]\nx = 1\n").expect("write");

        let ctx = ToolContext {
            cfg: None,
            codebase_semantic: None,
            command_max_output_len: 30_000,
            weather_timeout_secs: 0,
            allowed_commands: &[],
            working_dir: &root,
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
        };

        let body = crate_contract_map(
            r#"{"extra_paths":["src/lib.rs","docs/SSE协议.md","config/default_config.toml"]}"#,
            &ctx,
        );
        let _ = fs::remove_dir_all(&root);

        assert!(body.contains("crate_contract_map"));
        assert!(body.contains("crabmate_contract_map_v1"));
        assert!(body.contains("Router"));
        assert!(body.contains("SSE"));
    }
}
