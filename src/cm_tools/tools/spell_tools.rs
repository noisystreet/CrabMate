//! 文档拼写检查（**typos** / **codespell**），供 `docs_health_sweep` 复用。
//!
//! 均在**工作区根**执行，路径参数须为相对路径且不含 `..`；不传入写文件类参数（如 codespell 的 `-w`）。

use std::path::Path;
use std::process::{Command, Stdio};

use super::output_util;

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

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    output_util::run_command_output_formatted(
        cmd,
        title,
        max_output_len,
        MAX_OUTPUT_LINES,
        output_util::ProcessOutputMerge::StderrElseStdout,
        output_util::CommandSpawnErrorStyle::CannotStartWithPathHint,
    )
}

/// `typos`：默认检查 `README.md` 与 `docs`（若存在）；可传 `paths` 覆盖。只读，不写回文件。
pub fn typos_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let args: TyposCheckArgs = match crate::cm_tools::tools::parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let v = match serde_json::to_value(&args) {
        Ok(v) => v,
        Err(e) => return format!("参数序列化错误: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };
    let paths = match parse_rel_paths_limited(&v, "paths", &["README.md", "docs"], MAX_SPELL_PATHS)
    {
        Ok(p) => p,
        Err(e) => return e,
    };
    let config_path = match parse_optional_rel_path(&v, "config_path") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("typos");
    cmd.arg("--format").arg("brief").current_dir(&base);
    if let Some(cfg) = config_path {
        if !base.join(&cfg).is_file() {
            return format!("错误：config_path 文件不存在：{}", cfg);
        }
        cmd.arg("--config").arg(cfg);
    }
    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "typos")
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
pub fn codespell_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let args: CodespellCheckArgs = match crate::cm_tools::tools::parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let v = match serde_json::to_value(&args) {
        Ok(v) => v,
        Err(e) => return format!("参数序列化错误: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };
    let paths = match parse_rel_paths_limited(&v, "paths", &["README.md", "docs"], MAX_SPELL_PATHS)
    {
        Ok(p) => p,
        Err(e) => return e,
    };
    let dictionary_paths =
        match parse_rel_paths_limited(&v, "dictionary_paths", &[], MAX_SPELL_DICT_PATHS) {
            Ok(p) => p,
            Err(e) => return e,
        };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("codespell");
    cmd.arg("-q").arg("3").current_dir(&base);
    if let Err(e) = codespell_apply_optional_cli_flags(&v, &mut cmd, &base, &dictionary_paths) {
        return e;
    }
    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "codespell")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn typos_check_rejects_bad_config_path() {
        let out = typos_check(r#"{"config_path":"../.typos.toml"}"#, Path::new("."), 4096);
        assert!(out.contains("非法相对路径"), "{}", out);
    }

    #[test]
    fn codespell_check_requires_existing_dictionary_file() {
        let out = codespell_check(
            r#"{"dictionary_paths":["docs/nope.dict"]}"#,
            Path::new("."),
            4096,
        );
        assert!(out.contains("dictionary_paths 文件不存在"), "{}", out);
    }
}
