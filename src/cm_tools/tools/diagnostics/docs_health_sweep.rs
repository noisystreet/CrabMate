//! 文档与健康聚合：主文档预览 + typos + codespell + Markdown 链接检查（只读）。

use std::path::Path;

use super::super::ToolContext;
use super::super::file::read_file;
use super::super::output_util::truncate_output_bytes;
use super::super::repo_overview;
use super::spell_tools;
use crate::cm_names as tool_names;
use crate::cm_tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::DocsHealthSweepArgs;

/// 阶段参数二次序列化失败（`DocsHealthSweepArgs` → `Value` → `String`）。
///
/// 走到这里的入参已经过一次成功反序列化，因此只有 `serde_json` 自身出错才可能触发——
/// 属内部不变量破坏，不是入参问题，故用 `*_serialize_failed`（归 `Internal`）。
fn phase_args_serialize_failed(phase: &str, e: &serde_json::Error) -> ToolError {
    ToolError::internal_code(
        "docs_health_serialize_failed",
        format!("{phase} 参数序列化失败：{e}"),
    )
}

fn default_doc_preview_paths() -> Vec<String> {
    repo_overview::default_health_sweep_doc_paths()
}

/// 聚合步骤的状态。
///
/// 用枚举而不是 `"passed"` / `"failed"` 字符串：原先各阶段先把状态压成字符串塞进
/// `summary`，末尾再用 `any(|(_, s)| s == "failed")` 把字符串解析回成败——那正是本次
/// 要消灭的「拿渲染产物做判定」。展示词只在 [`StageVerdict::label`] 一处产生。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageVerdict {
    /// 跑完且无问题。
    Passed,
    /// 跑完并发现问题（内容发现，非工具故障）。
    Failed,
    /// 本阶段未运行（参数关掉 / 前置阶段 `fail_fast`）。
    Skipped,
    /// 跑起来了但没有结论（被信号终止）。
    Aborted,
    /// 只读预览类：有产出，无成败概念。
    Done,
}

impl StageVerdict {
    fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Aborted => "aborted",
            Self::Done => "done",
        }
    }

    fn is_failure(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// [`spell_tools::SpellCheckStatus`] → 聚合状态（展示词与 `fail_fast` 口径都在这里定）。
fn spell_stage_verdict(status: spell_tools::SpellCheckStatus) -> StageVerdict {
    match status {
        spell_tools::SpellCheckStatus::Clean => StageVerdict::Passed,
        spell_tools::SpellCheckStatus::FoundIssues => StageVerdict::Failed,
        spell_tools::SpellCheckStatus::Unavailable => StageVerdict::Skipped,
        spell_tools::SpellCheckStatus::Aborted => StageVerdict::Aborted,
    }
}

/// `docs_health_sweep` 各阶段共用的 JSON / 路径 / 输出上限（避免 phase 函数长参数列表）。
struct DocsSweepPhaseEnv<'a> {
    v: &'a serde_json::Value,
    workspace_root: &'a Path,
    max_output_len: usize,
    fail_fast: bool,
    summary_only: bool,
}

struct DocsSweepPhaseIo<'a> {
    sections: &'a mut Vec<String>,
    summary: &'a mut Vec<(String, StageVerdict)>,
}

struct TyposPhaseCtx<'a> {
    env: &'a DocsSweepPhaseEnv<'a>,
    io: DocsSweepPhaseIo<'a>,
    run_typos: bool,
    run_codespell: bool,
    run_markdown_links: bool,
}

struct CodespellPhaseCtx<'a> {
    env: &'a DocsSweepPhaseEnv<'a>,
    io: DocsSweepPhaseIo<'a>,
    run_codespell: bool,
    run_markdown_links: bool,
}

fn push_skipped_after_typos(
    summary: &mut Vec<(String, StageVerdict)>,
    run_codespell: bool,
    run_markdown_links: bool,
) {
    if run_codespell {
        summary.push(("codespell_check".to_string(), StageVerdict::Skipped));
    }
    if run_markdown_links {
        summary.push((
            tool_names::MARKDOWN_CHECK_LINKS.to_string(),
            StageVerdict::Skipped,
        ));
    }
}

fn append_doc_preview_section(
    v: &serde_json::Value,
    workspace_root: &Path,
    ctx: &ToolContext<'_>,
    run_doc_preview: bool,
    summary_only: bool,
    sections: &mut Vec<String>,
    summary: &mut Vec<(String, StageVerdict)>,
) {
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
                sections.push(read_file(&args_s, workspace_root, ctx));
                sections.push("\n---\n".to_string());
            }
        }
        summary.push(("doc_preview".to_string(), StageVerdict::Done));
    } else {
        summary.push(("doc_preview".to_string(), StageVerdict::Skipped));
    }
}

#[allow(clippy::result_large_err)]
fn typos_args_json(v: &serde_json::Value) -> Result<String, ToolError> {
    let spell_paths = v.get("spell_paths").cloned();
    let mut o = serde_json::Map::new();
    if let Some(p) = spell_paths {
        o.insert("paths".to_string(), p);
    }
    if let Some(s) = v.get("typos_config_path").and_then(|x| x.as_str()) {
        o.insert(
            "config_path".to_string(),
            serde_json::Value::String(s.to_string()),
        );
    }
    serde_json::to_string(&serde_json::Value::Object(o))
        .map_err(|e| phase_args_serialize_failed("typos", &e))
}

#[allow(clippy::result_large_err)]
fn run_typos_phase(ctx: TyposPhaseCtx<'_>) -> Result<Option<String>, ToolError> {
    let TyposPhaseCtx {
        env,
        io,
        run_typos,
        run_codespell,
        run_markdown_links,
    } = ctx;
    let DocsSweepPhaseIo { sections, summary } = io;
    if !run_typos {
        summary.push(("typos_check".to_string(), StageVerdict::Skipped));
        return Ok(None);
    }
    let typos_args = typos_args_json(env.v)?;
    let report = spell_tools::typos_check(&typos_args, env.workspace_root, env.max_output_len)?;
    let verdict = spell_stage_verdict(report.status);
    let failed = verdict.is_failure();
    summary.push(("typos_check".to_string(), verdict));
    if !env.summary_only {
        sections.push("## 2) typos_check\n\n".to_string());
        sections.push(report.output);
        sections.push("\n\n".to_string());
    }
    if env.fail_fast && failed {
        push_skipped_after_typos(summary, run_codespell, run_markdown_links);
        return Ok(Some(build_output(
            summary,
            sections,
            env.summary_only,
            env.max_output_len,
        )));
    }
    Ok(None)
}

#[allow(clippy::result_large_err)]
fn codespell_args_json(v: &serde_json::Value) -> Result<String, ToolError> {
    let spell_paths = v.get("spell_paths").cloned();
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
    serde_json::to_string(&serde_json::Value::Object(o))
        .map_err(|e| phase_args_serialize_failed("codespell", &e))
}

#[allow(clippy::result_large_err)]
fn run_codespell_phase(ctx: CodespellPhaseCtx<'_>) -> Result<Option<String>, ToolError> {
    let CodespellPhaseCtx {
        env,
        io,
        run_codespell,
        run_markdown_links,
    } = ctx;
    let DocsSweepPhaseIo { sections, summary } = io;
    if !run_codespell {
        summary.push(("codespell_check".to_string(), StageVerdict::Skipped));
        return Ok(None);
    }
    let codespell_args = codespell_args_json(env.v)?;
    let report =
        spell_tools::codespell_check(&codespell_args, env.workspace_root, env.max_output_len)?;
    let verdict = spell_stage_verdict(report.status);
    let failed = verdict.is_failure();
    summary.push(("codespell_check".to_string(), verdict));
    if !env.summary_only {
        sections.push("## 3) codespell_check\n\n".to_string());
        sections.push(report.output);
        sections.push("\n\n".to_string());
    }
    if env.fail_fast && failed {
        if run_markdown_links {
            summary.push((
                tool_names::MARKDOWN_CHECK_LINKS.to_string(),
                StageVerdict::Skipped,
            ));
        }
        return Ok(Some(build_output(
            summary,
            sections,
            env.summary_only,
            env.max_output_len,
        )));
    }
    Ok(None)
}

#[allow(clippy::result_large_err)]
fn markdown_check_links_args_json(v: &serde_json::Value) -> Result<String, ToolError> {
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
    serde_json::to_string(&serde_json::Value::Object(o))
        .map_err(|e| phase_args_serialize_failed(tool_names::MARKDOWN_CHECK_LINKS, &e))
}

/// 运行 Markdown 链接检查阶段。
///
/// 两种「失败」必须分开：`markdown_check_links_report_try` 返回 `Err` 是**执行故障**
/// （入参不合法 / 工作区解析 / 扫描 IO），上抛给调用方；`verdict.problem_count > 0` 是
/// **内容发现**，只记进汇总，整体仍是 `Ok`——工具确实跑完了。
///
/// 判据取结构化 `verdict`，**不**扫正文：`output_format=json` / `sarif` 时正文里
/// 根本没有「结论: 发现 N 处问题」这句话，按正文判会恒为「通过」。
#[allow(clippy::result_large_err)]
fn run_markdown_links_phase(
    v: &serde_json::Value,
    workspace_root: &Path,
    run_markdown_links: bool,
    summary_only: bool,
    sections: &mut Vec<String>,
    summary: &mut Vec<(String, StageVerdict)>,
) -> Result<(), ToolError> {
    if !run_markdown_links {
        summary.push((
            tool_names::MARKDOWN_CHECK_LINKS.to_string(),
            StageVerdict::Skipped,
        ));
        return Ok(());
    }
    let md_args = markdown_check_links_args_json(v)?;
    let report =
        super::super::markdown_links::markdown_check_links_report_try(&md_args, workspace_root)?;
    summary.push((
        tool_names::MARKDOWN_CHECK_LINKS.to_string(),
        if report.verdict.is_clean() {
            StageVerdict::Passed
        } else {
            StageVerdict::Failed
        },
    ));
    if !summary_only {
        sections.push("## 4) markdown_check_links\n\n".to_string());
        sections.push(report.output);
        sections.push("\n".to_string());
    }
    Ok(())
}

/// 只读聚合：文档头预览、`typos_check`、`codespell_check`、`markdown_check_links`。
///
/// **外链探测**：`markdown_check_links` 在 `allowed_external_prefixes` 非空时使用内置 HTTP 客户端发 HEAD，
/// **不经过** `http_fetch` / `http_request` 工具与 `http_fetch_allowed_prefixes`，也**无** Web SSE 审批会话。
/// **失败语义**（迁 `Typed` 时定下）：只有「入参不合法」与「某阶段执行故障」返回 `Err`；
/// 各阶段**检查出问题**（typos 有拼写、codespell 有错词、坏链接）只写进正文的
/// `### 步骤汇总` / `整体: 存在失败项`，整体仍是 `Ok` —— 工具确实跑完了，
/// 查到的问题交给模型判断，不该表现为工具故障（那会让调用方去重试一个已经成功的扫查）。
#[allow(clippy::result_large_err)]
pub fn docs_health_sweep_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let args: DocsHealthSweepArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let v = serde_json::to_value(&args).map_err(|e| phase_args_serialize_failed("sweep", &e))?;

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

    let ctx = ToolContext::stub(workspace_root, max_output_len);

    let mut sections: Vec<String> = Vec::new();
    let mut summary: Vec<(String, StageVerdict)> = Vec::new();

    sections.push(
        "=== docs_health_sweep（只读）===\n\
         说明：Markdown 外链 HEAD 探测由 markdown_check_links 内置 HTTP 发起，不经过 http_fetch 白名单与审批；\
         仅当 md_allowed_external_prefixes 非空时才会请求外网。\n"
            .to_string(),
    );

    append_doc_preview_section(
        &v,
        workspace_root,
        &ctx,
        run_doc_preview,
        summary_only,
        &mut sections,
        &mut summary,
    );

    let sweep_env = DocsSweepPhaseEnv {
        v: &v,
        workspace_root,
        max_output_len,
        fail_fast,
        summary_only,
    };

    if let Some(out) = run_typos_phase(TyposPhaseCtx {
        env: &sweep_env,
        io: DocsSweepPhaseIo {
            sections: &mut sections,
            summary: &mut summary,
        },
        run_typos,
        run_codespell,
        run_markdown_links,
    })? {
        return Ok(out);
    }

    if let Some(out) = run_codespell_phase(CodespellPhaseCtx {
        env: &sweep_env,
        io: DocsSweepPhaseIo {
            sections: &mut sections,
            summary: &mut summary,
        },
        run_codespell,
        run_markdown_links,
    })? {
        return Ok(out);
    }

    run_markdown_links_phase(
        &v,
        workspace_root,
        run_markdown_links,
        summary_only,
        &mut sections,
        &mut summary,
    )?;

    Ok(build_output(
        &summary,
        &sections,
        summary_only,
        max_output_len,
    ))
}

fn build_output(
    summary: &[(String, StageVerdict)],
    sections: &[String],
    summary_only: bool,
    max_output_len: usize,
) -> String {
    // 成败从枚举读，不再从「刚写进去的显示词」里反解析回来。
    let any_failed = summary.iter().any(|(_, v)| v.is_failure());
    let mut out = String::new();
    out.push_str("### 步骤汇总\n");
    for (name, verdict) in summary {
        out.push_str(&format!("- {}: {}\n", name, verdict.label()));
    }
    out.push_str(&format!(
        "\n整体: {}\n\n",
        if any_failed {
            "存在失败项（见上）"
        } else {
            "未发现失败项（CLI 未安装记为 skipped，被中断记为 aborted）"
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

    /// 展示词与「算不算失败」是输出契约：改它们等于改 `### 步骤汇总` 的内容。
    #[test]
    fn stage_verdict_labels_and_failure_flag() {
        assert_eq!(StageVerdict::Passed.label(), "passed");
        assert_eq!(StageVerdict::Failed.label(), "failed");
        assert_eq!(StageVerdict::Skipped.label(), "skipped");
        assert_eq!(StageVerdict::Aborted.label(), "aborted");
        assert_eq!(StageVerdict::Done.label(), "done");
        assert!(StageVerdict::Failed.is_failure());
        assert!(!StageVerdict::Passed.is_failure());
        assert!(!StageVerdict::Skipped.is_failure());
        assert!(
            !StageVerdict::Aborted.is_failure(),
            "被信号终止是本阶段没结论，不该让整体报「存在失败项」"
        );
        assert!(!StageVerdict::Done.is_failure());
    }

    /// 拼写检查四态 → 聚合状态。
    #[test]
    fn spell_status_maps_to_stage_verdict() {
        use super::spell_tools::SpellCheckStatus;
        assert_eq!(
            spell_stage_verdict(SpellCheckStatus::Clean),
            StageVerdict::Passed
        );
        assert_eq!(
            spell_stage_verdict(SpellCheckStatus::FoundIssues),
            StageVerdict::Failed
        );
        assert_eq!(
            spell_stage_verdict(SpellCheckStatus::Unavailable),
            StageVerdict::Skipped
        );
        assert_eq!(
            spell_stage_verdict(SpellCheckStatus::Aborted),
            StageVerdict::Aborted
        );
    }

    /// 每个用例独立临时根（并行跑测试时不会互相删目录）。
    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("crabmate_docs_health_{}_{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        root
    }

    /// 只跑 Markdown 链接阶段：其余阶段关掉，避免依赖本机是否装了 typos / codespell。
    const ONLY_MARKDOWN: &str = r#"{"run_doc_preview":false,"run_typos":false,"run_codespell":false,"run_markdown_links":true,"md_roots":["README.md"]}"#;

    #[test]
    fn sweep_readme_only_markdown() {
        let root = temp_root("readme");
        fs::write(root.join("README.md"), "# Hi\n\n[me](./README.md)\n").expect("w");

        let out = docs_health_sweep_try(ONLY_MARKDOWN, &root, 80_000).expect("干净扫查应成功");
        let _ = fs::remove_dir_all(&root);

        assert!(out.contains("docs_health_sweep"));
        assert!(out.contains("markdown_check_links"));
        assert!(out.contains("passed") || out.contains("未发现"));
    }

    /// 内容问题 ≠ 工具失败：坏链接只写进汇总，返回值仍是 `Ok`。
    ///
    /// 迁移前正文含「存在失败项」会被 `parse_tool_output` 的 `contains("失败")` 规则
    /// 判成失败 —— 工具明明跑完了，调用方却收到「失败」，接着可能去重试一次已经成功的扫查。
    #[test]
    fn content_findings_stay_ok() {
        let root = temp_root("findings");
        fs::write(root.join("README.md"), "# Hi\n\n[gone](./missing.md)\n").expect("w");

        let out = docs_health_sweep_try(ONLY_MARKDOWN, &root, 80_000)
            .expect("发现坏链接是内容问题，不是工具故障");
        let _ = fs::remove_dir_all(&root);

        assert!(out.contains("markdown_check_links: failed"), "{out}");
        assert!(out.contains("存在失败项"), "{out}");
    }

    /// `md_output_format=json` 时正文里**没有**「结论: 发现 N 处问题」这句话。
    ///
    /// 旧实现按 `block.contains("结论: 发现")` 判失败，于是只要调用方把输出格式换成
    /// json / sarif，坏链接就会被报成 `passed`——判定依赖渲染排版，是静默失效的典型。
    /// 现在判据是结构化 `verdict`，与输出格式无关。
    #[test]
    fn json_output_format_still_reports_findings() {
        const JSON_ONLY: &str = r#"{"run_doc_preview":false,"run_typos":false,"run_codespell":false,"run_markdown_links":true,"md_roots":["README.md"],"md_output_format":"json"}"#;
        let root = temp_root("json_findings");
        fs::write(root.join("README.md"), "# Hi\n\n[gone](./missing.md)\n").expect("w");

        let out = docs_health_sweep_try(JSON_ONLY, &root, 80_000).expect("内容发现不是工具故障");
        let _ = fs::remove_dir_all(&root);

        assert!(
            !out.contains("结论: 发现"),
            "json 正文不该有 text 报告文案：{out}"
        );
        assert!(out.contains("markdown_check_links: failed"), "{out}");
        assert!(out.contains("存在失败项"), "{out}");
    }

    /// 阶段真的执行不了（`markdown_check_links_try` 返回 `Err`）⇒ 整个 sweep 失败。
    ///
    /// 迁移前这里是「把 `e.message` 拼进正文、判 `passed`」——执行故障被吞成成功。
    /// 用「工作区根不存在」触发 `markdown_check_collect_files` 解析失败；
    /// `md_roots` 写成 `../outside` 这类**非法根**不会返回 `Err`（那是内容问题，只记一条本地问题）。
    #[test]
    fn markdown_phase_error_fails_the_sweep() {
        let root = std::env::temp_dir().join(format!(
            "crabmate_docs_health_{}_missing_root",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);

        let e = docs_health_sweep_try(ONLY_MARKDOWN, &root, 80_000)
            .expect_err("工作区不可用应是执行故障");

        assert_eq!(
            e.code, "markdown_check_links_workspace_failed",
            "{}",
            e.code
        );
    }

    /// 入参类型不对仍是 `invalid_args`（不让「容忍」把真参数错误也吞掉）。
    #[test]
    fn bad_args_is_invalid_args() {
        let root = temp_root("bad_args");
        let e = docs_health_sweep_try(r#"{"md_roots":"not-an-array"}"#, &root, 1_000)
            .expect_err("md_roots 类型不对应失败");
        let _ = fs::remove_dir_all(&root);

        assert_eq!(e.code, "invalid_args");
    }
}
