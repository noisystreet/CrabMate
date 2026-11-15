//! 文档拼写检查（**typos** / **codespell**），供 `docs_health_sweep` 复用。
//!
//! 均在**工作区根**执行，路径参数须为相对路径且不含 `..`；不传入写文件类参数（如 codespell 的 `-w`）。

use std::path::Path;
use std::process::{Command, Stdio};

use super::output_util;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::{CodespellCheckArgs, TyposCheckArgs};

const MAX_OUTPUT_LINES: usize = 800;
const MAX_SPELL_PATHS: usize = 24;
const MAX_SPELL_DICT_PATHS: usize = 8;

fn is_safe_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.contains("..")
}

fn parse_rel_paths_limited(
    v: &serde_json::Value,
    key: &str,
    default: &[&str],
    max: usize,
) -> Result<Vec<String>, String> {
    let arr = match v.get(key) {
        Some(serde_json::Value::Array(a)) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        _ => default.iter().map(|s| (*s).to_string()).collect(),
    };
    if arr.len() > max {
        return Err(format!("错误：{} 最多 {} 项", key, max));
    }
    for p in &arr {
        if !is_safe_rel_path(p) {
            return Err(format!(
                "错误：{} 中含非法相对路径（须非空、非绝对、不含 ..）：{}",
                key, p
            ));
        }
    }
    Ok(arr)
}

fn parse_optional_rel_path(v: &serde_json::Value, key: &str) -> Result<Option<String>, String> {
    let Some(raw) = v.get(key).and_then(|x| x.as_str()) else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if !is_safe_rel_path(raw) {
        return Err(format!(
            "错误：{} 中含非法相对路径（须非空、非绝对、不含 ..）：{}",
            key, raw
        ));
    }
    Ok(Some(raw.to_string()))
}

/// 仅保留工作区内存在的路径；若全部不存在则退回 `.`（由调用方决定是否允许）。
fn filter_existing(base: &Path, paths: &[String]) -> Vec<String> {
    let ex: Vec<_> = paths
        .iter()
        .filter(|p| base.join(p).exists())
        .cloned()
        .collect();
    if ex.is_empty() {
        vec![".".to_string()]
    } else {
        ex
    }
}

/// 拼写检查阶段的**结构化**结论。
///
/// 三态刻意分开，因为「跑通了并发现问题」和「本机没装这个 CLI」是两件事：
/// 前者是**内容发现**（调用方自己决定要不要紧），后者只是跳过。两者都不能靠
/// 回扫正文里有没有「无法启动」/`(exit=N)` 来判断——那些字样是渲染产物，
/// 改一次 `output_util` 的文案，判定就会静默失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellCheckStatus {
    /// CLI 跑完且退出码 0：未发现问题。
    Clean,
    /// CLI 跑完但退出码为正：发现拼写问题（内容发现，非工具故障）。
    FoundIssues,
    /// CLI 不在 PATH / 无法启动：本阶段跳过。
    Unavailable,
    /// CLI 起来了但被信号终止（退出码为负）：本阶段**没有结论**。
    ///
    /// 不并进 [`Self::FoundIssues`]——那会谎报「发现了问题」；也不并进
    /// [`Self::Unavailable`]——那会让人以为 CLI 没装。
    Aborted,
}

impl SpellCheckStatus {
    /// 退出码 → 状态。
    ///
    /// `output_util::render_process_output` 用 `status.code().unwrap_or(-1)` 兜底，因此
    /// **负码 = 被信号终止**（OOM / 外部 kill），不是「检查出问题」。
    #[must_use]
    pub fn from_exit_code(code: Option<i32>) -> Self {
        match code {
            None => Self::Unavailable,
            Some(0) => Self::Clean,
            Some(c) if c > 0 => Self::FoundIssues,
            Some(_) => Self::Aborted,
        }
    }
}

/// 一次拼写检查：渲染正文 + 结构化结论。
#[derive(Debug, Clone)]
pub struct SpellCheckReport {
    pub output: String,
    pub status: SpellCheckStatus,
}

impl SpellCheckReport {
    fn from_run(run: output_util::ProcessRunReport) -> Self {
        Self {
            status: SpellCheckStatus::from_exit_code(run.exit_code),
            output: run.output,
        }
    }
}

fn run_spell_cli(mut cmd: Command, max_output_len: usize, title: &str) -> SpellCheckReport {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    SpellCheckReport::from_run(output_util::run_command_output_report(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::StderrElseStdout,
        output_util::CommandSpawnErrorStyle::CannotStartWithPathHint,
    ))
}

/// `typos`：默认检查 `README.md` 与 `docs`（若存在）；可传 `paths` 覆盖。只读，不写回文件。
///
/// 入参非法 / 工作区不可解析 → `Err`（调用方上抛）；CLI 缺失 → `Unavailable`，
/// **不**算失败也不上抛——那是「本机没装」，不是「检查出问题」。
#[allow(clippy::result_large_err)]
pub fn typos_check(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<SpellCheckReport, ToolError> {
    let args: TyposCheckArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let v = serde_json::to_value(&args).map_err(|e| {
        ToolError::internal_code("spell_args_serialize_failed", format!("参数序列化错误: {e}"))
    })?;
    let base = workspace_root.canonicalize().map_err(|e| {
        ToolError::external_code("spell_workspace_failed", format!("工作区根目录无法解析: {e}"))
    })?;
    let paths = parse_rel_paths_limited(&v, "paths", &["README.md", "docs"], MAX_SPELL_PATHS)
        .map_err(ToolError::invalid_args)?;
    let config_path =
        parse_optional_rel_path(&v, "config_path").map_err(ToolError::invalid_args)?;
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("typos");
    cmd.arg("--format").arg("brief").current_dir(&base);
    if let Some(cfg) = config_path {
        if !base.join(&cfg).is_file() {
            return Err(ToolError::invalid_args(format!(
                "错误：config_path 文件不存在：{}",
                cfg
            )));
        }
        cmd.arg("--config").arg(cfg);
    }
    for p in &paths {
        cmd.arg(p);
    }
    Ok(run_spell_cli(cmd, max_output_len, "typos"))
}

fn push_codespell_skip_flag(v: &serde_json::Value, cmd: &mut Command) -> Result<(), String> {
    let Some(skip) = v.get("skip").and_then(|x| x.as_str()).map(str::trim) else {
        return Ok(());
    };
    if skip.len() > 512 || skip.contains("..") || skip.contains('\n') {
        return Err("错误：skip 过长或含非法字符".to_string());
    }
    if !skip.is_empty() {
        cmd.arg("--skip").arg(skip);
    }
    Ok(())
}

fn push_codespell_ignore_words(v: &serde_json::Value, cmd: &mut Command) -> Result<(), String> {
    let Some(list) = v
        .get("ignore_words_list")
        .and_then(|x| x.as_str())
        .map(str::trim)
    else {
        return Ok(());
    };
    if list.len() > 512 || list.contains('\n') {
        return Err("错误：ignore_words_list 过长或含非法字符".to_string());
    }
    if !list.is_empty() {
        cmd.arg("-L").arg(list);
    }
    Ok(())
}

fn push_codespell_dictionaries(
    cmd: &mut Command,
    base: &Path,
    dictionary_paths: &[String],
) -> Result<(), String> {
    for dict in dictionary_paths {
        if !base.join(dict).is_file() {
            return Err(format!("错误：dictionary_paths 文件不存在：{}", dict));
        }
        cmd.arg("-I").arg(dict);
    }
    Ok(())
}

fn codespell_apply_optional_cli_flags(
    v: &serde_json::Value,
    cmd: &mut Command,
    base: &Path,
    dictionary_paths: &[String],
) -> Result<(), String> {
    push_codespell_skip_flag(v, cmd)?;
    push_codespell_ignore_words(v, cmd)?;
    push_codespell_dictionaries(cmd, base, dictionary_paths)
}

/// `codespell`：默认路径同 typos；**禁止**传入写回参数。使用 `-q 3` 减少噪音。
///
/// 失败语义同 [`typos_check`]。
#[allow(clippy::result_large_err)]
pub fn codespell_check(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<SpellCheckReport, ToolError> {
    let args: CodespellCheckArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let v = serde_json::to_value(&args).map_err(|e| {
        ToolError::internal_code("spell_args_serialize_failed", format!("参数序列化错误: {e}"))
    })?;
    let base = workspace_root.canonicalize().map_err(|e| {
        ToolError::external_code("spell_workspace_failed", format!("工作区根目录无法解析: {e}"))
    })?;
    let paths = parse_rel_paths_limited(&v, "paths", &["README.md", "docs"], MAX_SPELL_PATHS)
        .map_err(ToolError::invalid_args)?;
    let dictionary_paths =
        parse_rel_paths_limited(&v, "dictionary_paths", &[], MAX_SPELL_DICT_PATHS)
            .map_err(ToolError::invalid_args)?;
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("codespell");
    cmd.arg("-q").arg("3").current_dir(&base);
    codespell_apply_optional_cli_flags(&v, &mut cmd, &base, &dictionary_paths)
        .map_err(ToolError::invalid_args)?;
    for p in &paths {
        cmd.arg(p);
    }
    Ok(run_spell_cli(cmd, max_output_len, "codespell"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn typos_check_rejects_bad_config_path() {
        let e = typos_check(r#"{"config_path":"../.typos.toml"}"#, Path::new("."), 4096)
            .expect_err("非法相对路径应是入参错误");
        assert_eq!(e.code, "invalid_args", "{}", e.code);
        assert!(e.message.contains("非法相对路径"), "{}", e.message);
    }

    #[test]
    fn codespell_check_requires_existing_dictionary_file() {
        let e = codespell_check(
            r#"{"dictionary_paths":["docs/nope.dict"]}"#,
            Path::new("."),
            4096,
        )
        .expect_err("字典文件不存在应是入参错误");
        assert_eq!(e.code, "invalid_args", "{}", e.code);
        assert!(
            e.message.contains("dictionary_paths 文件不存在"),
            "{}",
            e.message
        );
    }

    /// 退出码 → 四态是本次改动的核心映射：**负码（被信号终止）不能算「发现问题」**。
    ///
    /// `render_process_output` 用 `status.code().unwrap_or(-1)` 兜底，所以 OOM / 外部 kill
    /// 会得到 `-1`；旧实现（以及 `Some(_) => FoundIssues` 的写法）会把它报成
    /// `typos_check: failed`，让人以为查出了拼写错误。
    #[test]
    fn exit_code_maps_to_states() {
        assert_eq!(
            SpellCheckStatus::from_exit_code(None),
            SpellCheckStatus::Unavailable,
            "子进程没起来 = CLI 未安装"
        );
        assert_eq!(SpellCheckStatus::from_exit_code(Some(0)), SpellCheckStatus::Clean);
        assert_eq!(
            SpellCheckStatus::from_exit_code(Some(1)),
            SpellCheckStatus::FoundIssues
        );
        assert_eq!(
            SpellCheckStatus::from_exit_code(Some(2)),
            SpellCheckStatus::FoundIssues,
            "typos / codespell 用各种正码表示「找到问题」"
        );
        assert_eq!(
            SpellCheckStatus::from_exit_code(Some(-1)),
            SpellCheckStatus::Aborted,
            "被信号终止没有结论，不能冒充 failed"
        );
    }
}
