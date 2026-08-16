//! 文档拼写检查（**typos** / **codespell**）与 **ast-grep** 结构化搜索。
//!
//! 均在**工作区根**执行，路径参数须为相对路径且不含 `..`；不传入写文件类参数（如 codespell 的 `-w`）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::output_util;

use crate::cm_tools::tools::tool_param_types::{
    AstGrepRewriteArgs, AstGrepRunArgs, CodespellCheckArgs, TyposCheckArgs,
};

const MAX_OUTPUT_LINES: usize = 800;
const MAX_SPELL_PATHS: usize = 24;
const MAX_SPELL_DICT_PATHS: usize = 8;
const MAX_AST_PATHS: usize = 8;
const MAX_AST_GLOBS: usize = 10;
const MAX_PATTERN_LEN: usize = 4096;

fn is_safe_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.contains("..")
}

/// 额外传给 ast-grep 的 `--globs`：禁止 `..`、控制长度，避免注入。
fn is_safe_glob_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && !s.contains("..")
        && !s
            .chars()
            .any(|c| matches!(c, '\n' | '\r' | '\0' | '`' | '$'))
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

fn is_safe_ast_pattern(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_PATTERN_LEN && !s.chars().any(|c| matches!(c, '\r' | '\0'))
}

/// 将 `lang` 规范为 ast-grep 接受的短名（小写）。
fn normalize_ast_lang(raw: &str) -> Result<&'static str, String> {
    let s = raw.trim().to_lowercase();
    let s = s.as_str();
    Ok(match s {
        "rust" | "rs" => "rust",
        "c" => "c",
        "cpp" | "c++" | "cxx" | "cc" => "cpp",
        "python" | "py" => "python",
        "javascript" | "js" => "javascript",
        "typescript" | "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "go" | "golang" => "go",
        "java" => "java",
        "kotlin" | "kt" => "kotlin",
        "bash" | "sh" | "shell" => "bash",
        "html" => "html",
        "css" => "css",
        _ => {
            return Err(format!(
                "不支持的 lang：{raw}（支持 rust、c/cpp、python、javascript、typescript、tsx、jsx、go、java、kotlin、bash、html、css）"
            ));
        }
    })
}

/// `typos`：默认检查 `README.md` 与 `docs`（若存在）；可传 `paths` 覆盖。只读，不写回文件。
pub fn typos_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let parsed = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: TyposCheckArgs = match serde_json::from_value(parsed) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
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

fn codespell_apply_optional_cli_flags(
    v: &serde_json::Value,
    cmd: &mut Command,
    base: &Path,
    dictionary_paths: &[String],
) -> Result<(), String> {
    if let Some(skip) = v.get("skip").and_then(|x| x.as_str()).map(str::trim) {
        if skip.len() > 512 || skip.contains("..") || skip.contains('\n') {
            return Err("错误：skip 过长或含非法字符".to_string());
        }
        if !skip.is_empty() {
            cmd.arg("--skip").arg(skip);
        }
    }
    if let Some(list) = v
        .get("ignore_words_list")
        .and_then(|x| x.as_str())
        .map(str::trim)
    {
        if list.len() > 512 || list.contains('\n') {
            return Err("错误：ignore_words_list 过长或含非法字符".to_string());
        }
        if !list.is_empty() {
            cmd.arg("-L").arg(list);
        }
    }
    for dict in dictionary_paths {
        if !base.join(dict).is_file() {
            return Err(format!("错误：dictionary_paths 文件不存在：{}", dict));
        }
        cmd.arg("-I").arg(dict);
    }
    Ok(())
}

/// `codespell`：默认路径同 typos；**禁止**传入写回参数。使用 `-q 3` 减少噪音。
pub fn codespell_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let parsed = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CodespellCheckArgs = match serde_json::from_value(parsed) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
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

fn extract_ast_pattern_and_lang(v: &serde_json::Value) -> Result<(String, &'static str), String> {
    let pattern = match v.get("pattern").and_then(|x| x.as_str()) {
        Some(s) if is_safe_ast_pattern(s) => s.to_string(),
        Some(_) => return Err("错误：pattern 为空或过长或含非法字符".to_string()),
        None => return Err("错误：缺少 pattern（ast-grep 模式串）".to_string()),
    };
    let lang_raw = match v.get("lang").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        Some(_) => return Err("错误：lang 不能为空（如 rust、typescript、python）".to_string()),
        None => return Err("错误：缺少 lang（如 rust、typescript、python）".to_string()),
    };
    let lang = normalize_ast_lang(lang_raw)?;
    Ok((pattern, lang))
}

const AST_GREP_DEFAULT_GLOBS: &[&str] = &[
    "!**/target/**",
    "!**/node_modules/**",
    "!**/.git/**",
    "!**/vendor/**",
    "!**/dist/**",
    "!**/build/**",
];

fn ast_grep_append_default_globs(cmd: &mut Command) {
    for g in AST_GREP_DEFAULT_GLOBS {
        cmd.arg("--globs").arg(*g);
    }
}

fn ast_grep_append_custom_globs(v: &serde_json::Value, cmd: &mut Command) -> Result<(), String> {
    let Some(arr) = v.get("globs").and_then(|x| x.as_array()) else {
        return Ok(());
    };
    if arr.len() > MAX_AST_GLOBS {
        return Err(format!("错误：globs 最多 {} 项", MAX_AST_GLOBS));
    }
    for x in arr {
        let Some(s) = x.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            return Err("错误：globs 须为非空字符串数组".to_string());
        };
        if !is_safe_glob_token(s) {
            return Err(format!("错误：非法 glob：{}", s));
        }
        cmd.arg("--globs").arg(s);
    }
    Ok(())
}

fn typed_args_json_to_value<T>(args_json: &str) -> Result<serde_json::Value, String>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let parsed = crate::cm_tools::tools::parse_args_json(args_json)?;
    let args: T = serde_json::from_value(parsed).map_err(|e| format!("参数解析错误: {e}"))?;
    serde_json::to_value(&args).map_err(|e| format!("参数序列化错误: {e}"))
}

struct AstGrepPrepared {
    v: serde_json::Value,
    pattern: String,
    lang: &'static str,
    base: PathBuf,
    paths: Vec<String>,
}

fn prepare_ast_grep(
    v: serde_json::Value,
    workspace_root: &Path,
) -> Result<AstGrepPrepared, String> {
    let (pattern, lang) = extract_ast_pattern_and_lang(&v)?;
    let base = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {}", e))?;
    let paths = parse_rel_paths_limited(&v, "paths", &["src"], MAX_AST_PATHS)?;
    let paths = filter_existing(&base, &paths);
    Ok(AstGrepPrepared {
        v,
        pattern,
        lang,
        base,
        paths,
    })
}

/// `rewrite`：可选；`update_all`：写盘时加 `--update-all`。
fn build_ast_grep_cmd(
    prep: &AstGrepPrepared,
    rewrite: Option<&str>,
    update_all: bool,
) -> Result<Command, String> {
    let mut cmd = Command::new("ast-grep");
    cmd.args(["run", "--color", "never"])
        .arg("-p")
        .arg(&prep.pattern);
    if let Some(r) = rewrite {
        cmd.arg("-r").arg(r);
    }
    cmd.arg("-l").arg(prep.lang).current_dir(&prep.base);
    ast_grep_append_default_globs(&mut cmd);
    ast_grep_append_custom_globs(&prep.v, &mut cmd)?;
    if update_all {
        cmd.arg("--update-all");
    }
    for p in &prep.paths {
        cmd.arg(p);
    }
    Ok(cmd)
}

fn extract_rewrite_policy(v: &serde_json::Value) -> Result<(String, bool), String> {
    let rewrite = match v.get("rewrite").and_then(|x| x.as_str()) {
        Some(s) if is_safe_ast_pattern(s) => s.to_string(),
        Some(_) => return Err("错误：rewrite 为空或过长或含非法字符".to_string()),
        None => return Err("错误：缺少 rewrite（目标替换模板）".to_string()),
    };
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(true);
    let confirm = v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false);
    if !dry_run && !confirm {
        return Err(
            "错误：ast_grep_rewrite 写盘需 confirm=true；建议先 dry_run=true 预览".to_string(),
        );
    }
    Ok((rewrite, dry_run))
}

/// `ast-grep run`：结构化搜索。默认路径 `["src"]`；内置排除 `target`、`node_modules` 等 glob。
pub fn ast_grep_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match typed_args_json_to_value::<AstGrepRunArgs>(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let prep = match prepare_ast_grep(v, workspace_root) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let cmd = match build_ast_grep_cmd(&prep, None, false) {
        Ok(c) => c,
        Err(e) => return e,
    };
    run_and_format(cmd, max_output_len, "ast-grep run")
}

/// `ast-grep run --rewrite`：结构化改写。默认 dry-run；写盘需 `confirm=true`。
pub fn ast_grep_rewrite(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match typed_args_json_to_value::<AstGrepRewriteArgs>(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // 与历史一致：先 pattern/lang 与路径，再 rewrite/confirm 门闩。
    let prep = match prepare_ast_grep(v, workspace_root) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (rewrite, dry_run) = match extract_rewrite_policy(&prep.v) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let cmd = match build_ast_grep_cmd(&prep, Some(&rewrite), !dry_run) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let title = if dry_run {
        "ast-grep rewrite (dry-run)"
    } else {
        "ast-grep rewrite (update-all)"
    };
    run_and_format(cmd, max_output_len, title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_lang_accepts_aliases() {
        assert_eq!(normalize_ast_lang("RS").unwrap(), "rust");
        assert_eq!(normalize_ast_lang("TypeScript").unwrap(), "typescript");
    }

    #[test]
    fn safe_glob_rejects_dotdot() {
        assert!(!is_safe_glob_token("**/../**"));
    }

    #[test]
    fn rewrite_requires_confirm_when_not_dry_run() {
        let out = ast_grep_rewrite(
            r#"{"pattern":"foo($A)","rewrite":"bar($A)","lang":"rust","dry_run":false}"#,
            Path::new("."),
            4096,
        );
        assert!(out.contains("confirm=true"));
    }

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
