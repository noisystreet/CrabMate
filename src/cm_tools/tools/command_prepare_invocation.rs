//! `run_command` 调用准备：解析 argv、白名单/工作区可执行、参数安全。
//! 作为 [`super`]（`command`）的子模块，经 `#[path]` 挂载。

use std::path::{Path, PathBuf};

use super::super::command_line_prepare::{
    arg_has_parent_dir_ref, is_arg_safe, merge_dot_slash_with_single_relative_path,
    parse_cd_prefix, peel_workspace_cd_prefix, resolve_cd_prefix_dir,
    split_command_prefix_if_embedded,
};
use super::{
    PreparedRunCommand, RunCommandError, check_shell_variable_references,
    command_shell_script::{
        allowlist_allows_all, argv_needs_posix_shell_wrap, glob_tokens_suppressed_by_quoting,
        is_shell_dash_c_invocation, maybe_wrap_argv_with_posix_shell, posix_shell_on_allowlist,
        ShellWrap,
    },
};

/// 规范化后的 `run_command` 调用参数（纯解析；不含工作目录/白名单裁定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// 原始 args JSON 文本（`workspace_image_chat_hint` 等消费方需要原文）。
    pub args_json: String,
    /// 已 trim，并已做 `./` 合并与整段命令拆分的命令名；`script` 形态为空。
    pub command: String,
    pub args: Vec<String>,
    /// 显式工作目录（工作区内相对路径）；`.args` 中的 `cd` 前缀仍会叠加在其上。
    pub cwd: Option<String>,
    /// 显式脚本正文；非空时经 `bash -c` / `sh -c` 执行。
    pub script: Option<String>,
    /// 解析阶段发生的兼容性改写（仅用于可选回显）。
    pub rewrites: Vec<String>,
}

/// 解析 `run_command` 参数：JSON 修复 → 字段提取 → 规范化。
///
/// `working_dir` 仅用于把工作区内绝对路径改写成相对路径；不做文件系统存在性校验，也不做白名单裁定。
pub fn parse_command_spec(
    args_json: &str,
    working_dir: &Path,
) -> Result<CommandSpec, RunCommandError> {
    let value = super::parse_run_command_value_with_repair(args_json)?;
    let script = read_optional_string_field(&value, "script")?;
    let cwd = read_optional_string_field(&value, "cwd")?;
    // `cwd` 的路径判定**不在这里**：本函数拿不到权限路径范围与允许根集合，早拒会把
    // 「落在 `workspace_allowed_roots` 内的 `../sibling`」当成越界（#1315）。正式判定在
    // `scan_run_command_unsafe_args_spec`（按根集合）与 `prepare_run_command_spec` 的
    // `resolve_cd_prefix_dir`（`skip_arg_safety = false` 时仍用 `is_arg_safe` + 会话根锚）
    // 两处；未接线路径因此只是报错点后移一个阶段，`UnsafeArg` 与错误码不变。
    if let Some(script) = script {
        if script_conflicts_with_command_or_args(&value)? {
            return Err(RunCommandError::ScriptCommandConflict);
        }
        return Ok(CommandSpec {
            args_json: args_json.to_string(),
            command: String::new(),
            args: Vec::new(),
            cwd,
            script: Some(script),
            rewrites: Vec::new(),
        });
    }
    let (command, args, rewrites) = extract_run_command_name_and_args(&value, working_dir)?;
    Ok(CommandSpec {
        args_json: args_json.to_string(),
        command,
        args,
        cwd,
        script: None,
        rewrites,
    })
}

fn read_optional_string_field(
    value: &serde_json::Value,
    field: &'static str,
) -> Result<Option<String>, RunCommandError> {
    match value.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(Some(s.to_string()))
            }
        }
        Some(_) => Err(RunCommandError::FieldNotString { field }),
    }
}

/// `script` 与旧字段的互斥校验：对字段类型的要求与旧 `command`/`args` 路径一致。
fn script_conflicts_with_command_or_args(
    value: &serde_json::Value,
) -> Result<bool, RunCommandError> {
    let command_non_empty = match value.get("command") {
        Some(serde_json::Value::String(c)) => !c.trim().is_empty(),
        Some(_) => return Err(RunCommandError::MissingCommand),
        None => false,
    };
    let args_non_empty = match value.get("args") {
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(_) => return Err(RunCommandError::ArgsNotArray),
        None => false,
    };
    Ok(command_non_empty || args_non_empty)
}

/// 宽松解析：仅供只读消费方（去重 key 规范化、失败家族分类兜底）。
///
/// 不走 JSON 修复链、不做路径规范化；`args` 非数组时视为空，缺失/非字符串 `command` 返回 `None`。
/// 空 `command` 是否接受由调用方决定。显式 `script` 归一为 `bash -c <script>`。
/// **不用于实际执行**：执行路径一律先 [`parse_command_spec`]。
pub fn parse_run_command_payload_lenient(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    if let Some(script) = v
        .get("script")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(("bash".to_string(), vec!["-c".to_string(), script.to_string()]));
    }
    let command = v.get("command")?.as_str()?.trim().to_string();
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some((command, args))
}

fn normalize_workspace_absolute_arg(arg: &str, working_dir: &Path) -> String {
    let a = arg.trim();
    if !a.starts_with('/') {
        return arg.to_string();
    }
    let Ok(rel) = Path::new(a).strip_prefix(working_dir) else {
        return arg.to_string();
    };
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().to_string()
    }
}

/// 从 JSON 取出 `command`/`args`，并做绝对路径归一化与常见误拆修复。
fn extract_run_command_name_and_args(
    args: &serde_json::Value,
    working_dir: &Path,
) -> Result<(String, Vec<String>, Vec<String>), RunCommandError> {
    let mut cmd_raw = match args.get("command").and_then(|c| c.as_str()) {
        Some(s) => s.trim().to_string(),
        None => return Err(RunCommandError::MissingCommand),
    };

    let mut cmd_args: Vec<String> = match args.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => return Err(RunCommandError::ArgsNotArray),
        None => vec![],
    };
    let mut rewrites = Vec::new();
    let before_norm = render_spec_invocation(&cmd_raw, &cmd_args);
    cmd_args = cmd_args
        .into_iter()
        .map(|a| normalize_workspace_absolute_arg(&a, working_dir))
        .collect();
    let after_norm = render_spec_invocation(&cmd_raw, &cmd_args);
    if after_norm != before_norm {
        rewrites.push(format!("{before_norm} → {after_norm}"));
    }

    merge_dot_slash_with_single_relative_path(&mut cmd_raw, &mut cmd_args);
    split_command_prefix_if_embedded(&mut cmd_raw, &mut cmd_args);
    let after_fix = render_spec_invocation(&cmd_raw, &cmd_args);
    if after_fix != after_norm {
        rewrites.push(format!("{after_norm} → {after_fix}"));
    }
    Ok((cmd_raw, cmd_args, rewrites))
}

fn render_spec_invocation(command: &str, args: &[String]) -> String {
    let cmd = command.trim();
    if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {}", args.join(" "))
    }
}

/// 解析工作区可执行路径；否则要求命令名在白名单内。
fn resolve_run_command_exec_path(
    cmd_raw: &str,
    effective_working_dir: &Path,
    allowed_commands: &[String],
) -> Result<(String, Option<PathBuf>), RunCommandError> {
    let cmd_name = cmd_raw.to_lowercase();
    let is_workspace_executable = cmd_raw.starts_with("./") || cmd_raw.contains('/');
    let exec_path = if is_workspace_executable {
        crate::cm_tools::tools::resolve_workspace_executable(effective_working_dir, cmd_raw).ok()
    } else {
        None
    };

    if exec_path.is_none()
        && !allowlist_allows_all(allowed_commands)
        && !allowed_commands
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd_name))
    {
        return Err(RunCommandError::DisallowedCommand {
            attempted: cmd_name,
            allowed: allowed_commands.join(", "),
        });
    }
    Ok((cmd_name, exec_path))
}

fn arg_is_unsafe_for_cmd(cmd_name: &str, arg: &str, has_workspace_exec: bool) -> bool {
    if has_workspace_exec {
        arg_has_parent_dir_ref(arg) || arg.trim_start().starts_with('/')
    } else {
        !is_arg_safe(cmd_name, arg)
    }
}

fn validate_run_command_args_safety(
    cmd_name: &str,
    cmd_args: &[String],
    has_workspace_exec: bool,
) -> Result<(), RunCommandError> {
    for a in cmd_args {
        if arg_is_unsafe_for_cmd(cmd_name, a, has_workspace_exec) {
            return Err(RunCommandError::UnsafeArg);
        }
    }
    Ok(())
}

/// 命令轴路径判定上下文（S7-cmd）：把「会话根 + 权限路径范围 + 允许根集合」收成一个载体。
///
/// 根集合只在 `AllowedRoots` **实际生效**（非空）时参与判定；否则退回「canonical 后
/// `starts_with(会话根)`」的既有语义——空根 fail-closed，与 `#1149` 一致。
struct CommandPathScope<'a> {
    access: crate::cm_tools::tools::PathAccess<'a>,
    /// canonical 后的会话根（既有判定锚）。
    anchor: PathBuf,
    rooted: bool,
}

impl<'a> CommandPathScope<'a> {
    fn new(
        workspace_root: &Path,
        access: crate::cm_tools::tools::PathAccess<'a>,
    ) -> Result<Self, RunCommandError> {
        let anchor = workspace_root
            .canonicalize()
            .map_err(|e| RunCommandError::SpawnOther {
                cmd: "canonicalize(workspace)".to_string(),
                source: e,
            })?;
        let rooted = access.allowed_roots_active();
        Ok(Self {
            access,
            anchor,
            rooted,
        })
    }

    /// 已 canonicalize 的目标是否在允许范围内。
    fn within_canonical(&self, canon: &Path) -> bool {
        if self.rooted {
            self.access.is_arg_within_allowed_roots(canon)
        } else {
            canon.starts_with(&self.anchor)
        }
    }

    /// 形态危险的参数是否**已证明**落在允许范围内。
    ///
    /// 根集合未生效时恒 `false`——与改动前逐字等价：扫描只判形态，形态危险即不安全。
    /// 抽不出候选路径时同样按「未证明」处理，方向保守。
    fn arg_within(&self, arg: &str, effective_working_dir: &Path) -> bool {
        if !self.rooted {
            return false;
        }
        let Some(candidate) = candidate_path_of_arg(arg) else {
            return false;
        };
        self.access
            .is_arg_within_allowed_roots(&effective_working_dir.join(candidate))
    }
}

/// 从「形态危险」的参数里抽出候选路径片段。
///
/// 只认两种形态：整 token 即路径（`../x`、`/abs/x`），以及 `--opt=../x` 的 `=` 右侧。
/// 粘写形态（`-C../sibling`）不在候选之列——`arg_has_parent_dir_ref` 本来就不把它当穿越，
/// 保持现状（见 issue「已知偏差 A」）。
fn candidate_path_of_arg(arg: &str) -> Option<&str> {
    let a = arg.trim();
    if a.is_empty() {
        return None;
    }
    if !a.starts_with('-') {
        return Some(a);
    }
    let (_, value) = a.split_once('=')?;
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// 参数扫描结果（S7-cmd）。
///
/// 两个字段回答两个不同问题，缺一不可：
///
/// - `had_pattern_unsafe`：**有没有**形态危险参数（`..` 穿越 / 绝对路径）。用来把
///   「压根没危险参数」（必须 `skip_arg_safety = false`）与「危险参数全在允许根内」区分开。
/// - `escaping`：哪些参数**没能证明**落在允许根集合内。非空 ⇒ L2 硬拒 / L3 交互审批，
///   且拒因与审批正文只列这几个（不再笼统喊「工作区外路径参数」）。
///
/// 根集合未生效时 `escaping` == 全部形态危险参数，与改动前逐字等价。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UnsafeArgScan {
    /// 形态危险且**未证明**在允许根集合内的参数（去重、保持出现顺序）。
    pub escaping: Vec<String>,
    /// 是否存在形态危险参数（不论落点）。
    pub had_pattern_unsafe: bool,
}

impl UnsafeArgScan {
    /// 记一个形态危险参数；`within` = 「已证明落在允许根集合内」。
    fn note(&mut self, raw: &str, within: bool) {
        self.had_pattern_unsafe = true;
        if within || self.escaping.iter().any(|u| u == raw) {
            return;
        }
        self.escaping.push(raw.to_string());
    }

    /// 「有形态危险参数，但全部落在允许根内」——等价于 `skip_arg_safety = true` 的那一档。
    ///
    /// `had_pattern_unsafe == false`（压根没有危险参数）刻意**不**算：那条路必须走
    /// `NotNeeded`，`skip_arg_safety` 保持 `false`。
    #[must_use]
    pub fn all_within_allowed_roots(&self) -> bool {
        self.had_pattern_unsafe && self.escaping.is_empty()
    }
}

/// `cd` / `cwd` 目标的落点判定；`false` = 逃出允许范围。
///
/// 与改动前的差别只在判据：`AllowedRoots` 生效时按根集合判，否则仍是
/// `canonical.starts_with(会话根)`。目标不是目录时不做逃逸判定（交给执行侧的
/// `resolve_cd_prefix_dir` 报 `CdPrefixInvalid`）。
fn scan_cd_join_escape(
    candidate: &Path,
    scope: &CommandPathScope<'_>,
    pattern_unsafe: bool,
    effective_working_dir: &mut PathBuf,
) -> Result<bool, RunCommandError> {
    if !candidate.is_dir() {
        return Ok(true);
    }
    let canon_cand = candidate
        .canonicalize()
        .map_err(|e| RunCommandError::SpawnOther {
            cmd: format!("canonicalize({})", candidate.display()),
            source: e,
        })?;
    let within = scope.within_canonical(&canon_cand);
    if within && !pattern_unsafe {
        *effective_working_dir = canon_cand;
    }
    Ok(within)
}

/// 扫描用：推进 `cd … &&` 前缀；**未能证明在允许范围内**的 `cd` 目录记入 `scan` 后仍合成推进，
/// 以便继续扫后续 argv——`cd ../crabmate-client && cat /etc/shadow` 里两个目标各自定论，
/// 不会因为 `cd` 合法就整体放过。
fn advance_cd_prefixes_collecting_unsafe(
    scope: &CommandPathScope<'_>,
    effective_working_dir: &mut PathBuf,
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    scan: &mut UnsafeArgScan,
) -> Result<(), RunCommandError> {
    loop {
        if !cmd_raw.eq_ignore_ascii_case("cd") {
            break;
        }
        let Some((dir, rest)) = parse_cd_prefix(cmd_args) else {
            // 与 peel 一致：不完整前缀交给后续 peel/prepare 报 CdPrefixInvalid
            break;
        };
        let dir = dir.to_string();
        let mut rest = rest.to_vec();
        let pattern_unsafe = !is_arg_safe("cd", &dir);
        let candidate = effective_working_dir.join(&dir);
        let within = scan_cd_join_escape(&candidate, scope, pattern_unsafe, effective_working_dir)?;
        if pattern_unsafe || !within {
            scan.note(&dir, within);
        }
        *cmd_raw = rest.remove(0);
        *cmd_args = rest;
        split_command_prefix_if_embedded(cmd_raw, cmd_args);
    }
    Ok(())
}

/// `cwd` 字段的越界判定；未能证明在允许范围内时记入 `scan`。
fn scan_explicit_cwd_unsafe(
    cwd: &str,
    scope: &CommandPathScope<'_>,
    effective_working_dir: &mut PathBuf,
    scan: &mut UnsafeArgScan,
) -> Result<(), RunCommandError> {
    let pattern_unsafe = !is_arg_safe("cd", cwd);
    let candidate = effective_working_dir.join(cwd);
    let within = scan_cd_join_escape(&candidate, scope, pattern_unsafe, effective_working_dir)?;
    if pattern_unsafe || !within {
        scan.note(cwd, within);
    }
    Ok(())
}

/// 命令名解析 + argv 危险参数收集（扫描阶段；命令未在白名单时仍收集）。
fn collect_unsafe_after_resolve(
    cmd_raw: &str,
    cmd_args: &[String],
    effective_working_dir: &Path,
    allowed_commands: &[String],
    scope: &CommandPathScope<'_>,
    scan: &mut UnsafeArgScan,
) -> Result<(), RunCommandError> {
    let is_workspace_executable = cmd_raw.starts_with("./") || cmd_raw.contains('/');
    let workspace_exec = is_workspace_executable
        && crate::cm_tools::tools::resolve_workspace_executable(effective_working_dir, cmd_raw)
            .is_ok();
    let fallback_name = cmd_raw.to_lowercase();
    let (cmd_name, has_exec) =
        match resolve_run_command_exec_path(cmd_raw, effective_working_dir, allowed_commands) {
            Ok((name, exec)) => (name, exec.is_some()),
            Err(RunCommandError::DisallowedCommand { .. }) => (fallback_name, workspace_exec),
            Err(e) => return Err(e),
        };
    for a in cmd_args {
        if !arg_is_unsafe_for_cmd(&cmd_name, a, has_exec) {
            continue;
        }
        let within = scope.arg_within(a, effective_working_dir);
        scan.note(a, within);
    }
    Ok(())
}

/// 与 `prepare_run_command_spec` 同源解析后，给出参数级的路径判定（S7-cmd）。
///
/// `access` 携带**权限投影出的路径范围 + 允许根集合**：`AllowedRoots` 生效时，形态危险参数
/// 再判一次落点（落在任一允许根内即安全）；否则退回改动前的「只判形态」。
///
/// 与 `prepare_run_command_spec` 的分工：`skip_arg_safety` 仍只由调用方的三态 gate 派生，
/// 本函数不自己决定放行——它只回答「哪些参数没被证明在根内」。
pub fn scan_run_command_unsafe_args_spec(
    spec: &CommandSpec,
    working_dir: &Path,
    allowed_commands: &[String],
    access: crate::cm_tools::tools::PathAccess<'_>,
) -> Result<UnsafeArgScan, RunCommandError> {
    let scope = CommandPathScope::new(working_dir, access)?;
    let mut cmd_raw = spec.command.clone();
    let mut cmd_args = spec.args.clone();
    let mut effective_working_dir = working_dir.to_path_buf();
    let mut scan = UnsafeArgScan::default();
    if let Some(cwd) = spec.cwd.as_deref() {
        scan_explicit_cwd_unsafe(cwd, &scope, &mut effective_working_dir, &mut scan)?;
    }
    // 显式 `script` 的正文由 shell 审批覆盖，不再逐 argv 扫描。`AllowedRoots` 下**不放宽**：
    // 正文不受参数级根判定约束（issue「难点 4」的定论）。
    if spec.script.is_some() {
        return Ok(scan);
    }
    advance_cd_prefixes_collecting_unsafe(
        &scope,
        &mut effective_working_dir,
        &mut cmd_raw,
        &mut cmd_args,
        &mut scan,
    )?;
    // 扫描阶段不拦截 glob/`$VAR`：展开改由 bash -c 或异步审批处理。
    // 会改变后续参数锚点的选项（`git -C`、`cargo --manifest-path`、`tar -C`）**不推进**
    // `effective_working_dir`：结果只会多拒 / 多弹一次窗，不会偏松（issue「已知偏差 B」）。
    collect_unsafe_after_resolve(
        &cmd_raw,
        &cmd_args,
        &effective_working_dir,
        allowed_commands,
        &scope,
        &mut scan,
    )?;
    Ok(scan)
}

fn skip_shell_variable_check(cmd_raw: &str, cmd_args: &[String], allowed: &[String]) -> bool {
    is_shell_dash_c_invocation(cmd_raw, cmd_args)
        || (posix_shell_on_allowlist(allowed).is_some()
            && argv_needs_posix_shell_wrap(cmd_raw, cmd_args))
}

/// 经 `bash -c` 包装后的改写回显文案。
fn shell_wrap_rewrite_label(before: &str, wrap: &ShellWrap) -> String {
    format!(
        "{before} → {} -c '{}'（shell 包装：{}）",
        wrap.shell, wrap.script, wrap.reason
    )
}

/// 含空白 + glob 的 token 会被引用成字面量，glob 不展开；给出可见提示而不是静默降级。
fn quoted_glob_rewrite_label(tokens: &[String]) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    Some(format!(
        "以下参数含空白，glob 不会展开（如需展开请用 script）：{}",
        tokens.join("、")
    ))
}

fn wrap_posix_shell_reresolve(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    cmd_name: &mut String,
    exec_path: &mut Option<PathBuf>,
    working_dir: &Path,
    allowed_commands: &[String],
) -> Result<Option<ShellWrap>, RunCommandError> {
    let Some(wrap) = maybe_wrap_argv_with_posix_shell(cmd_raw, cmd_args, allowed_commands) else {
        return Ok(None);
    };
    let wrapped = resolve_run_command_exec_path(cmd_raw, working_dir, allowed_commands)?;
    *cmd_name = wrapped.0;
    *exec_path = wrapped.1;
    Ok(Some(wrap))
}

/// 初始 argv：显式 `script` 走 `shell -c`，否则沿用 `command` + `args`。
fn initial_invocation(
    spec: &CommandSpec,
    allowed_commands: &[String],
) -> Result<(String, Vec<String>), RunCommandError> {
    match spec.script.as_deref() {
        Some(script) => {
            let shell = posix_shell_on_allowlist(allowed_commands)
                .ok_or(RunCommandError::ScriptShellUnavailable)?;
            Ok((shell.to_string(), vec!["-c".to_string(), script.to_string()]))
        }
        None => Ok((spec.command.clone(), spec.args.clone())),
    }
}

/// `cd` 前缀剥离的改写回显文案。
fn cd_rewrite_label(before: &str, after: &str, anchor: &Path, effective: &Path) -> String {
    let cwd_label = effective
        .strip_prefix(anchor)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| effective.display().to_string());
    format!("{before} → {after} (cwd={cwd_label})")
}

/// 最终解析命令名/可执行路径、参数安全校验与 shell 重解析。
fn finalize_invocation(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
    validate_args: bool,
) -> Result<(String, Option<PathBuf>, Option<ShellWrap>), RunCommandError> {
    let (mut cmd_name, mut exec_path) =
        resolve_run_command_exec_path(cmd_raw, working_dir, allowed_commands)?;
    if validate_args && !skip_arg_safety {
        validate_run_command_args_safety(&cmd_name, cmd_args, exec_path.is_some())?;
    }
    let wrap = wrap_posix_shell_reresolve(
        cmd_raw,
        cmd_args,
        &mut cmd_name,
        &mut exec_path,
        working_dir,
        allowed_commands,
    )?;
    Ok((cmd_name, exec_path, wrap))
}

/// [`finalize_invocation`] + 把 shell 包装相关的改写写入 `rewrites`。
///
/// 单独成函数，避免把这些分支堆进 `prepare_run_command_spec`（Rust CCN 棘轮 ≤10）。
fn finalize_invocation_and_record(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
    validate_args: bool,
    rewrites: &mut Vec<String>,
) -> Result<(String, Option<PathBuf>), RunCommandError> {
    let before_finalize = super::format_invocation_for_display(cmd_raw, cmd_args);
    let cmd_before_wrap = cmd_raw.clone();
    let args_before_wrap = cmd_args.clone();
    let (cmd_name, exec_path, wrap) = finalize_invocation(
        cmd_raw,
        cmd_args,
        working_dir,
        allowed_commands,
        skip_arg_safety,
        validate_args,
    )?;
    if let Some(wrap) = wrap.as_ref() {
        rewrites.push(shell_wrap_rewrite_label(&before_finalize, wrap));
        if let Some(note) = quoted_glob_rewrite_label(&glob_tokens_suppressed_by_quoting(
            &cmd_before_wrap,
            &args_before_wrap,
        )) {
            rewrites.push(note);
        }
    }
    Ok((cmd_name, exec_path))
}

pub(super) fn prepare_run_command_spec(
    spec: &CommandSpec,
    working_dir: &Path,
    allowed_commands: &[String],
    skip_arg_safety: bool,
) -> Result<PreparedRunCommand, RunCommandError> {
    let mut rewrites = spec.rewrites.clone();
    let anchor = working_dir
        .canonicalize()
        .map_err(|e| RunCommandError::SpawnOther {
            cmd: "canonicalize(workspace)".to_string(),
            source: e,
        })?;
    let mut effective_working_dir = working_dir.to_path_buf();
    if let Some(cwd) = spec.cwd.as_deref() {
        effective_working_dir =
            resolve_cd_prefix_dir(&effective_working_dir, cwd, &anchor, skip_arg_safety)?;
    }
    let explicit_script = spec.script.as_deref();
    let (mut cmd_raw, mut cmd_args) = initial_invocation(spec, allowed_commands)?;

    let before_peel = super::format_invocation_for_display(&cmd_raw, &cmd_args);
    peel_workspace_cd_prefix(
        working_dir,
        &mut effective_working_dir,
        &mut cmd_raw,
        &mut cmd_args,
        skip_arg_safety,
    )?;
    let after_peel = super::format_invocation_for_display(&cmd_raw, &cmd_args);
    if after_peel != before_peel {
        rewrites.push(cd_rewrite_label(
            &before_peel,
            &after_peel,
            &anchor,
            &effective_working_dir,
        ));
    }

    let inject_gh_token = crate::cm_tools::github_token::command_basename_is_gh(&cmd_raw);
    if !skip_shell_variable_check(&cmd_raw, &cmd_args, allowed_commands) {
        check_shell_variable_references(&cmd_raw, &cmd_args)?;
    }

    let (cmd_name, exec_path) = finalize_invocation_and_record(
        &mut cmd_raw,
        &mut cmd_args,
        &effective_working_dir,
        allowed_commands,
        skip_arg_safety,
        explicit_script.is_none(),
        &mut rewrites,
    )?;

    Ok(PreparedRunCommand {
        cmd_raw,
        cmd_name,
        exec_path,
        cmd_args,
        effective_working_dir,
        inject_gh_token,
        rewrites,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::RunCommandError;
    use super::{
        parse_command_spec, prepare_run_command_spec, scan_run_command_unsafe_args_spec,
    };


    /// 未配多根的 `PathAccess`（`SessionRoot` + 空根）：命令轴退回改动前的「只判形态」。
    fn session_access() -> crate::cm_tools::tools::PathAccess<'static> {
        crate::cm_tools::tools::PathAccess::new(Path::new("."))
    }
    fn test_allowed() -> Vec<String> {
        vec![
            "cat".into(),
            "cmake".into(),
            "git".into(),
            "cd".into(),
            "ls".into(),
        ]
    }

    fn spec(args_json: &str) -> super::CommandSpec {
        parse_command_spec(args_json, Path::new(".")).expect("spec")
    }

    #[test]
    fn scan_lists_external_absolute_path_not_cmake_dotdot() {
        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cat","args":["/etc/passwd"]}"#),
            Path::new("."),
            &test_allowed(),
            session_access(),
        )
        .expect("scan");
        assert_eq!(u.escaping, vec!["/etc/passwd".to_string()]);
        assert!(u.had_pattern_unsafe);

        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cmake","args":[".."]}"#),
            Path::new("."),
            &test_allowed(),
            session_access(),
        )
        .expect("scan");
        assert!(u.escaping.is_empty(), "{u:?}");
        assert!(!u.had_pattern_unsafe, "cmake .. 不算形态危险：{u:?}");

        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"git","args":["log","main..HEAD"]}"#),
            Path::new("."),
            &test_allowed(),
            session_access(),
        )
        .expect("scan");
        assert!(
            u.escaping.is_empty(),
            "git A..B must not look like path traversal: {u:?}"
        );
        assert!(!u.had_pattern_unsafe, "{u:?}");
    }

    #[test]
    fn scan_lists_cd_parent_dir_prefix() {
        let u = scan_run_command_unsafe_args_spec(
            &spec(r#"{"command":"cd","args":["..","&&","ls"]}"#),
            Path::new("."),
            &test_allowed(),
            session_access(),
        )
        .expect("scan");
        assert!(u.escaping.iter().any(|a| a == ".."), "{u:?}");
        assert!(u.had_pattern_unsafe);
    }

    #[test]
    fn star_sentinel_allows_arbitrary_command_name() {
        let args_json = r#"{"command":"whoami","args":[]}"#;
        let spec = parse_command_spec(args_json, Path::new(".")).expect("spec");
        let star = vec!["*".to_string()];
        let prepared = prepare_run_command_spec(&spec, Path::new("."), &star, false).expect("star");
        assert_eq!(prepared.cmd_name, "whoami");

        let denied = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false);
        assert!(matches!(
            denied,
            Err(RunCommandError::DisallowedCommand { .. })
        ));
    }

    #[test]
    fn explicit_script_conflicts_with_command_and_args() {
        let err = parse_command_spec(
            r#"{"script":"cargo build","command":"cargo","args":["test"]}"#,
            Path::new("."),
        )
        .expect_err("conflict");
        assert!(matches!(err, RunCommandError::ScriptCommandConflict));
    }

    #[test]
    fn explicit_script_validates_legacy_field_types() {
        let err = parse_command_spec(
            r#"{"script":"cargo build","args":"--release"}"#,
            Path::new("."),
        )
        .expect_err("args not array");
        assert!(matches!(&err, RunCommandError::ArgsNotArray));
        assert_eq!(err.into_tool_error().code, "args_not_array");

        let err = parse_command_spec(r#"{"script":"cargo build","command":5}"#, Path::new("."))
            .expect_err("command not string");
        assert!(matches!(err, RunCommandError::MissingCommand));
    }

    #[test]
    fn explicit_script_requires_shell_on_allowlist() {
        let err = prepare_run_command_spec(
            &spec(r#"{"script":"cargo build"}"#),
            Path::new("."),
            &test_allowed(),
            false,
        )
        .err()
        .expect("no shell");
        assert!(matches!(err, RunCommandError::ScriptShellUnavailable));
    }

    #[test]
    fn explicit_script_runs_via_shell_dash_c() {
        let mut allowed = test_allowed();
        allowed.push("bash".into());
        let prepared = prepare_run_command_spec(
            &spec(r#"{"script":"cargo build"}"#),
            Path::new("."),
            &allowed,
            false,
        )
        .expect("prepared");
        assert_eq!(prepared.cmd_name, "bash");
        assert_eq!(
            prepared.cmd_args,
            vec!["-c".to_string(), "cargo build".to_string()]
        );
    }

    #[test]
    fn explicit_cwd_must_be_relative_and_within_workspace() {
        // `parse_command_spec` 不判 `cwd` 越界（它拿不到根集合，见 `#1315`）：解析仍成功，
        // 由 `skip_arg_safety = false` 的 prepare 阶段报 `UnsafeArg`。
        for cwd in ["../x", "/tmp"] {
            let args_json = format!(r#"{{"command":"ls","cwd":"{cwd}"}}"#);
            let spec = parse_command_spec(&args_json, Path::new(".")).expect("parse 不判 cwd 越界");
            let Err(err) = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false)
            else {
                panic!("unsafe cwd 应被拒：{cwd}");
            };
            assert!(
                matches!(err, RunCommandError::UnsafeArg),
                "cwd {cwd}: {err:?}"
            );
        }

        let prepared = prepare_run_command_spec(
            &spec(r#"{"command":"ls","cwd":"src"}"#),
            Path::new("."),
            &test_allowed(),
            false,
        )
        .expect("prepared");
        assert!(prepared.effective_working_dir.ends_with("src"));
    }
}
