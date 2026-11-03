//! 将 `run_command` 的 command+args 拼成一行脚本，并在白名单含 `bash`/`sh` 时经 `bash -c` 执行。

use super::super::command_line_prepare::parse_cd_prefix;

/// POSIX 登录/非登录 shell 名（小写比较）。
pub fn is_posix_shell_name(cmd: &str) -> bool {
    matches!(cmd.trim().to_ascii_lowercase().as_str(), "bash" | "sh")
}

/// `bash`/`sh` 且 argv 含 `-c`（含 `-lc` 等带 `c` 的短选项）。
#[must_use]
pub fn is_shell_dash_c_invocation(cmd: &str, args: &[String]) -> bool {
    if !is_posix_shell_name(cmd) {
        return false;
    }
    args.iter().any(|a| arg_is_shell_c_flag(a))
}

fn arg_is_shell_c_flag(a: &str) -> bool {
    let a = a.trim();
    if a == "-c" || a == "-lc" || a == "-cl" {
        return true;
    }
    a.starts_with('-') && !a.starts_with("--") && a.contains('c')
}

/// 白名单是否为「全放行」哨兵：含 `*` 表示不校验命令名（`serve --yes` / `CM_ALLOWED_COMMANDS=*`）。
#[must_use]
pub fn allowlist_allows_all(allowed: &[String]) -> bool {
    allowed.iter().any(|c| c.trim() == "*")
}

/// 白名单中优先 `bash`，否则 `sh`；`*`（全放行）视为含 `bash`。
#[must_use]
pub fn posix_shell_on_allowlist(allowed: &[String]) -> Option<&'static str> {
    if allowlist_allows_all(allowed) {
        return Some("bash");
    }
    let has_bash = allowed.iter().any(|c| c.eq_ignore_ascii_case("bash"));
    if has_bash {
        return Some("bash");
    }
    let has_sh = allowed.iter().any(|c| c.eq_ignore_ascii_case("sh"));
    if has_sh {
        return Some("sh");
    }
    None
}

fn token_is_shell_operator(t: &str) -> bool {
    matches!(
        t.trim(),
        "&&" | "||" | "|" | ";" | ">" | ">>" | "<" | "2>" | "2>>" | "&"
    )
}

/// 剥掉 `cd <dir> && …` 前缀后再做 glob/操作符判断（不访问文件系统）。
#[must_use]
pub fn peel_cd_prefix_argv_for_shell_policy(cmd: &str, args: &[String]) -> (String, Vec<String>) {
    let mut cmd = cmd.trim().to_string();
    let mut args = args.to_vec();
    loop {
        if !cmd.eq_ignore_ascii_case("cd") {
            break;
        }
        let Some((_, rest)) = parse_cd_prefix(&args) else {
            break;
        };
        let mut rest = rest.to_vec();
        cmd = rest.remove(0);
        args = rest;
    }
    (cmd, args)
}

fn argv_tokens<'a>(cmd: &'a str, args: &'a [String]) -> impl Iterator<Item = &'a str> {
    std::iter::once(cmd).chain(args.iter().map(String::as_str))
}

fn token_needs_shell_expansion(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty() && detect_shell_expansion_token(t).is_some()
}

/// glob / `$VAR` / `~`（不含整段 `&&`/`|` 等操作符；命令替换不算展开）。
#[must_use]
pub fn argv_needs_shell_expansion(cmd: &str, args: &[String]) -> bool {
    if is_shell_dash_c_invocation(cmd, args) {
        return false;
    }
    argv_tokens(cmd, args).any(token_needs_shell_expansion)
}

/// 独立 argv 词是 shell 操作符（会绕过「单命令」白名单）。
#[must_use]
pub fn argv_has_shell_operators(cmd: &str, args: &[String]) -> bool {
    if is_shell_dash_c_invocation(cmd, args) {
        return false;
    }
    argv_tokens(cmd, args).any(token_is_shell_operator)
}

/// 需要经 `bash -c` 包装（展开或操作符）。
#[must_use]
pub fn argv_needs_posix_shell_wrap(cmd: &str, args: &[String]) -> bool {
    argv_needs_shell_expansion(cmd, args) || argv_has_shell_operators(cmd, args)
}

/// argv 中是否已经存在内层 shell 调用（`bash`/`sh` 后跟 `-c`）。
///
/// 这类调用自带完整的 shell 语义：glob / `$VAR` / `$(…)` 都由内层自己解释。
/// 外层再包装一次只会**抢先**把 `$VAR` 展开——`env FOO=inner bash -c "echo FOO=$FOO"`
/// 会被拼成 `env FOO=inner bash -c "echo FOO="`，内层永远看不到 `inner`。
/// 因此命中它时不包装，让 argv 原样交给内层。
///
/// 扫描**整个** argv 而不是只看第一个元素：包装器可以是 `env FOO=1`（赋值前缀），
/// 也可以是 `env -u HOME` / `timeout 10` / `nice -n 5`（带选项或前置数字）。
/// 只认「首个非赋值元素」时后一类会漏判，`$VAR` 仍被外层抢先展开
/// （`env -u HOME bash -c "echo H=$HOME"` 曾输出真实路径而非空）。
///
/// 代价是有意宽松：`["cat", "bash", "-c"]` 这类把 shell 名当普通参数、
/// 或 `["bash", "run.sh", "-c"]`（`-c` 实际传给 `run.sh`）也会被判为内层 shell。
/// 代价方向是「少包装 → `$VAR` 不展开」，比静默错值轻。
#[must_use]
pub fn argv_invokes_nested_shell(cmd: &str, args: &[String]) -> bool {
    if is_shell_dash_c_invocation(cmd, args) {
        return true;
    }
    args.iter().enumerate().any(|(i, a)| {
        is_posix_shell_name(a) && args[i + 1..].iter().any(|b| arg_is_shell_c_flag(b))
    })
}

/// 返回第一个命中的模式标签（供错误/审批文案）。
///
/// 反引号与 `$(` 属于**命令替换**，不在此列出：它们不触发 shell 包装，
/// 且在包装时会被双引号包裹成字面量（见 [`quote_script_token`]）。
#[must_use]
pub fn detect_shell_expansion_token(s: &str) -> Option<&'static str> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'$' if i + 1 < len => {
                let n = bytes[i + 1];
                if n == b'(' {
                    // 命令替换：跳过 `$(`，继续扫描其余部分（若另有 `$VAR` 仍会命中）。
                    i += 2;
                    continue;
                }
                if n == b'{' {
                    return Some("${...}");
                }
                if n.is_ascii_alphanumeric() || matches!(n, b'_' | b'?' | b'@' | b'*' | b'#' | b'!')
                {
                    return Some("$VAR");
                }
                i += 2;
            }
            b'~' if i == 0 || bytes[i.saturating_sub(1)] == b'=' => return Some("~"),
            b'*' => return Some("glob"),
            b'?' => {
                if token_looks_like_path_glob(s) && (i == 0 || bytes[i - 1] != b'-') {
                    return Some("glob");
                }
                i += 1;
            }
            b'[' => {
                if token_looks_like_path_glob(s) {
                    return Some("glob");
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// `?`/`[` 仅当 token 像路径 glob（`file?.c`、`src/[ab].rs`）；`--flag=…` 与 `foo?` 正则不算。
fn token_looks_like_path_glob(t: &str) -> bool {
    let t = t.trim();
    if t.starts_with('-') {
        return false;
    }
    t.contains('/') || t.contains('.')
}

/// token 是否含命令替换（未转义的反引号或 `$(`）。
fn contains_command_substitution(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i = i.saturating_add(2),
            '`' => return true,
            '$' if chars.get(i + 1) == Some(&'(') => return true,
            _ => i += 1,
        }
    }
    false
}

/// 用双引号包裹 token，尽量保持其 shell 原文语义。
///
/// - 已有的 `\X` 转义（如 `\$`、`\"`、`\\`）原样保留，避免二次转义把 `\$` 变成 `\\$`（后者会先还原成 `\` 再展开变量）；
/// - 未转义的 `"` 补 `\`，防止提前闭合；
/// - 未转义的反引号与 `$(` 补 `\`，避免命令替换被执行；
/// - 结尾的 `\` 需自身转义，避免吃掉收尾引号。
fn double_quote_token(arg: &str) -> String {
    let chars: Vec<char> = arg.chars().collect();
    let mut s = String::with_capacity(arg.len().saturating_add(2));
    s.push('"');
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            s.push('\\');
            match chars.get(i + 1) {
                Some(next) => {
                    s.push(*next);
                    i += 2;
                }
                None => {
                    s.push('\\');
                    i += 1;
                }
            }
            continue;
        }
        if ch == '"' || ch == '`' {
            s.push('\\');
            s.push(ch);
            i += 1;
            continue;
        }
        if ch == '$' && chars.get(i + 1) == Some(&'(') {
            s.push('\\');
            s.push('$');
            s.push('(');
            i += 2;
            continue;
        }
        s.push(ch);
        i += 1;
    }
    s.push('"');
    s
}

fn quote_script_token(arg: &str) -> String {
    let arg = arg.trim();
    if arg.is_empty() {
        return "''".to_string();
    }
    if token_is_shell_operator(arg) {
        return arg.to_string();
    }
    let has_expansion = detect_shell_expansion_token(arg).is_some();
    let has_substitution = contains_command_substitution(arg);
    if has_expansion || has_substitution {
        // 命令替换一律走双引号：双引号内 `\$` 是字面 `$`、`(` 无特殊性；
        // 而裸改写 `\$(` 会留下未引用的 `(`，整行变成 bash 语法错误。
        if has_substitution || arg.chars().any(char::is_whitespace) {
            return double_quote_token(arg);
        }
        return arg.to_string();
    }
    let safe_unquoted = arg.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@')
    }) && !arg.contains('\'');
    if safe_unquoted {
        return arg.to_string();
    }
    double_quote_token(arg)
}

/// `run_command` 的 `args` 数组拼成参数串（不含 argv0；供 SSE `command_approval_request.args`）。
#[must_use]
pub fn join_run_command_argv_tail(args: &[String]) -> String {
    let mut out = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&quote_script_token(a));
    }
    out
}

/// 拼成一行脚本（供 `bash -c` 与审批 **cli_detail**）。
#[must_use]
pub fn join_run_command_shell_script(cmd: &str, args: &[String]) -> String {
    let cmd = cmd.trim();
    let tail = join_run_command_argv_tail(args);
    if tail.is_empty() {
        return cmd.to_string();
    }
    format!("{cmd} {tail}")
}

/// `-c` 后的脚本正文（用于工具卡「命令：」展示）。
#[must_use]
pub fn dash_c_script_body(args: &[String]) -> Option<&str> {
    let mut take_next = false;
    for a in args {
        if take_next {
            return Some(a.as_str());
        }
        if arg_is_shell_c_flag(a) {
            take_next = true;
        }
    }
    None
}

/// `bash -c` 包装的决策结果（供 `PreparedRunCommand::rewrites` 回显）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellWrap {
    /// 实际使用的 shell（`bash` / `sh`）。
    pub shell: &'static str,
    /// 首个触发包装的模式标签（供回显 / 排障）。
    pub reason: &'static str,
    /// 拼成一行后的脚本正文。
    pub script: String,
}

/// 首个触发包装的标签：优先展开类（`glob` / `$VAR` / `~`），其次 shell 操作符。
fn wrap_reason(cmd: &str, args: &[String]) -> &'static str {
    for t in argv_tokens(cmd, args) {
        if let Some(label) = detect_shell_expansion_token(t.trim()) {
            return label;
        }
    }
    for t in argv_tokens(cmd, args) {
        if token_is_shell_operator(t) {
            return "shell operator";
        }
    }
    "shell"
}

/// 含空白且含 glob 的 token 会被双引号包裹，其 glob **不会**展开（供回显提示）。
#[must_use]
pub fn glob_tokens_suppressed_by_quoting(cmd: &str, args: &[String]) -> Vec<String> {
    argv_tokens(cmd, args)
        .filter(|t| {
            let t = t.trim();
            !t.is_empty()
                && t.chars().any(char::is_whitespace)
                && matches!(detect_shell_expansion_token(t), Some("glob"))
        })
        .map(str::to_string)
        .collect()
}

/// 若白名单含 bash/sh 且 argv 需要展开或含操作符，则改写为 `bash -c` / `sh -c`。
///
/// 返回包装信息（`None` = 未改写），供调用方写入 `rewrites` 显式回显——
/// 包装本身会改变展开语义，静默改写会让调用方看不出差异。
///
/// argv 里已有内层 shell 调用时一律不包装（见 [`argv_invokes_nested_shell`]）：
/// 那种情况下展开应全部交给内层，外层插手只会破坏 `$VAR` 的值。
/// 注意 `argv_needs_posix_shell_wrap` 仍返回 `true`，因此变量检查豁免与审批
/// `needs_shell` 保持原样——只有「实际包装」这一步被切断。
pub fn maybe_wrap_argv_with_posix_shell(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    allowed_commands: &[String],
) -> Option<ShellWrap> {
    if argv_invokes_nested_shell(cmd_raw, cmd_args) {
        return None;
    }
    let shell = posix_shell_on_allowlist(allowed_commands)?;
    if !argv_needs_posix_shell_wrap(cmd_raw, cmd_args) {
        return None;
    }
    let reason = wrap_reason(cmd_raw, cmd_args);
    let script = join_run_command_shell_script(cmd_raw, cmd_args);
    *cmd_raw = shell.to_string();
    *cmd_args = vec!["-c".to_string(), script.clone()];
    Some(ShellWrap {
        shell,
        reason,
        script,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_leaves_glob_and_dollar_unquoted() {
        let s = join_run_command_shell_script("ls", &["*.rs".into()]);
        assert_eq!(s, "ls *.rs");
        let s = join_run_command_shell_script("echo", &["$HOME".into()]);
        assert_eq!(s, "echo $HOME");
        let s = join_run_command_shell_script("ls", &["src".into(), "&&".into(), "pwd".into()]);
        assert_eq!(s, "ls src && pwd");
    }

    #[test]
    fn join_quotes_spaces_without_expansion() {
        let s = join_run_command_shell_script("echo", &["hello world".into()]);
        assert_eq!(s, r#"echo "hello world""#);
    }

    #[test]
    fn join_preserves_shell_escapes_inside_double_quotes() {
        let s = join_run_command_shell_script("echo", &[r"a \$HOME b".into()]);
        assert_eq!(s, r#"echo "a \$HOME b""#);
        let s = join_run_command_shell_script("echo", &[r#"say "hi" now"#.into()]);
        assert_eq!(s, r#"echo "say \"hi\" now""#);
        let s = join_run_command_shell_script("echo", &["end\\".into()]);
        assert_eq!(s, r#"echo "end\\""#);
    }

    #[test]
    fn command_substitution_is_not_a_wrap_trigger() {
        assert!(detect_shell_expansion_token("`id`").is_none());
        assert!(detect_shell_expansion_token("$(id)").is_none());
        assert!(!argv_needs_posix_shell_wrap(
            "git",
            &["commit".into(), "-m".into(), "fix `id`".into()]
        ));
        let mut cmd = "git".to_string();
        let mut args = vec!["commit".to_string(), "-m".to_string(), "fix `id`".to_string()];
        let allowed = vec!["git".to_string(), "bash".to_string()];
        assert!(
            maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed).is_none(),
            "命令替换不应触发 shell 包装"
        );
        assert_eq!(cmd, "git");
    }

    #[test]
    fn join_escapes_command_substitution_when_wrapping() {
        let s = join_run_command_shell_script(
            "git",
            &["commit".into(), "-m".into(), "fix `id`".into()],
        );
        assert_eq!(s, r#"git commit -m "fix \`id\`""#);
        let s = join_run_command_shell_script("echo", &["$(id)".into()]);
        // 双引号内 `\$` 仍是字面 `$`、`(` 无特殊性，因此不会再出现裸 `\$(` 的语法错误。
        assert_eq!(s, r#"echo "\$(id)""#);
        let s = join_run_command_shell_script("echo", &[r"a \`id\`".into()]);
        assert_eq!(s, r#"echo "a \`id\`""#);
        // 既有展开行为不变
        let s = join_run_command_shell_script("echo", &["$HOME".into()]);
        assert_eq!(s, "echo $HOME");
    }

    #[test]
    fn command_substitution_inside_expanding_token_stays_literal() {
        let s = join_run_command_shell_script("echo", &["$HOME/`id`".into()]);
        // 命令替换走双引号：双引号内 `\`` 是字面反引号，`$HOME` 仍展开。
        assert_eq!(s, r#"echo "$HOME/\`id\`""#);
    }

    #[test]
    fn argv_tail_omits_argv0() {
        assert_eq!(join_run_command_argv_tail(&[]), "");
        assert_eq!(
            join_run_command_argv_tail(&["-s".into(), "-L".into()]),
            "-s -L"
        );
        assert_eq!(
            join_run_command_argv_tail(&["hello world".into()]),
            r#""hello world""#
        );
    }

    #[test]
    fn wrap_when_bash_allowed_and_glob() {
        let mut cmd = "ls".to_string();
        let mut args = vec!["*.rs".to_string()];
        let allowed = vec!["ls".into(), "bash".into()];
        let wrap = maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed)
            .expect("glob 应触发 bash -c 包装");
        assert_eq!(cmd, "bash");
        assert_eq!(args, vec!["-c".to_string(), "ls *.rs".to_string()]);
        assert_eq!(wrap.shell, "bash");
        assert_eq!(wrap.reason, "glob");
        assert_eq!(wrap.script, "ls *.rs");
    }

    #[test]
    fn wrap_reports_reason_for_variable_and_operator() {
        let mut cmd = "echo".to_string();
        let mut args = vec!["$HOME".to_string()];
        let allowed = vec!["echo".into(), "bash".into()];
        let wrap = maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed)
            .expect("$VAR 应触发包装");
        assert_eq!(wrap.reason, "$VAR");
        assert_eq!(wrap.script, "echo $HOME");

        let mut cmd = "ls".to_string();
        let mut args = vec!["src".to_string(), "&&".to_string(), "pwd".to_string()];
        let allowed = vec!["ls".into(), "bash".into()];
        let wrap = maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed)
            .expect("独立操作符应触发包装");
        assert_eq!(wrap.reason, "shell operator");
        assert_eq!(wrap.script, "ls src && pwd");
    }

    #[test]
    fn glob_with_whitespace_is_reported_as_suppressed() {
        // 含空白 + glob 会被双引号包裹 → glob 不展开，必须能检测出来给提示。
        let got = glob_tokens_suppressed_by_quoting("echo", &["src/*.rs count:".into()]);
        assert_eq!(got, vec!["src/*.rs count:".to_string()]);
        // 无空白的 glob 正常展开，不应误报。
        assert!(glob_tokens_suppressed_by_quoting("ls", &["*.rs".into()]).is_empty());
        // 纯文字含空白但无 glob，不报。
        assert!(glob_tokens_suppressed_by_quoting("echo", &["hello world".into()]).is_empty());
    }

    #[test]
    fn nested_shell_in_argv_is_not_wrapped() {
        // `env FOO=inner bash -c "echo FOO=$FOO"`：外层若包装会抢先把 $FOO 展开成空，
        // 内层永远看不到 inner。命中内层 shell 时必须放弃包装，让 argv 原样传递。
        let mut cmd = "env".to_string();
        let mut args = vec![
            "FOO=inner".to_string(),
            "bash".to_string(),
            "-c".to_string(),
            "echo FOO=$FOO".to_string(),
        ];
        let allowed = vec!["env".to_string(), "bash".to_string()];
        assert!(
            maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed).is_none(),
            "内层已有 bash -c 时不应由外层包装"
        );
        assert_eq!(cmd, "env");
        assert_eq!(args[3], "echo FOO=$FOO");
    }

    #[test]
    fn nested_shell_keeps_wrap_and_approval_semantics() {
        // 只切断「实际包装」这一步：变量检查豁免与审批 needs_shell 仍须为 true，
        // 否则 `$FOO` 会被 check_shell_variable_references 误报为 ShellVariableDetected。
        let cmd = "env";
        let args = vec![
            "FOO=inner".to_string(),
            "bash".to_string(),
            "-c".to_string(),
            "echo FOO=$FOO".to_string(),
        ];
        assert!(argv_needs_posix_shell_wrap(cmd, &args));
        assert!(argv_invokes_nested_shell(cmd, &args));
    }

    #[test]
    fn nested_shell_detection_edges() {
        // 有意宽松：只问「argv 里有没有 shell 名 + c flag」，不建模包装器程序。
        // 代价是 `echo bash -c "$HOME"` 这类把 shell 名当参数的调用也不再被外层包装
        // （$VAR 不展开）；换来的是各种包装器形态都不会出现静默错值。
        assert!(argv_invokes_nested_shell(
            "echo",
            &["bash".into(), "-c".into()]
        ));
        assert!(argv_invokes_nested_shell(
            "env",
            &["FOO=1".into(), "sh".into(), "-c".into(), "echo".into()]
        ));
        // cmd 本身是 shell -c：统一口径也算。
        assert!(argv_invokes_nested_shell(
            "bash",
            &["-c".into(), "echo".into()]
        ));
        // 有 shell 名但没有 c flag：不是内层 shell 调用。
        assert!(!argv_invokes_nested_shell(
            "env",
            &["bash".into(), "run.sh".into()]
        ));
        // 没有 shell 名。
        assert!(!argv_invokes_nested_shell(
            "grep",
            &["a=b".into(), "file".into()]
        ));
        assert!(!argv_invokes_nested_shell("ls", &[]));
    }

    #[test]
    fn nested_shell_detected_behind_wrapper_options() {
        // 包装器的选项 / 前置数字不能掩盖内层 shell：只认「首个非赋值元素」会漏判，
        // 外层照旧抢先展开 `$VAR`（`env -u HOME bash -c "echo H=$HOME"` 曾输出真实路径）。
        assert!(argv_invokes_nested_shell(
            "env",
            &[
                "-u".into(),
                "HOME".into(),
                "bash".into(),
                "-c".into(),
                "echo H=$HOME".into()
            ]
        ));
        assert!(argv_invokes_nested_shell(
            "env",
            &[
                "-i".into(),
                "bash".into(),
                "-c".into(),
                "echo $HOME".into()
            ]
        ));
        assert!(argv_invokes_nested_shell(
            "timeout",
            &[
                "10".into(),
                "bash".into(),
                "-c".into(),
                "echo $HOME".into()
            ]
        ));
        assert!(argv_invokes_nested_shell(
            "nice",
            &["-n".into(), "5".into(), "bash".into(), "-c".into(), "x".into()]
        ));
    }

    #[test]
    fn no_wrap_without_shell_features() {
        let mut cmd = "git".to_string();
        let mut args = vec!["status".to_string()];
        let allowed = vec!["git".into(), "bash".into()];
        assert!(
            maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed).is_none(),
            "无展开/操作符时不应包装"
        );
        assert_eq!(cmd, "git");
    }

    #[test]
    fn no_double_wrap_bash_c() {
        let mut cmd = "bash".to_string();
        let mut args = vec!["-c".to_string(), "echo $HOME".to_string()];
        let allowed = vec!["bash".into()];
        assert!(
            maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed).is_none(),
            "bash -c 已是 shell 调用，不应重复包装"
        );
    }

    #[test]
    fn question_and_bracket_not_glob_unless_path_like() {
        assert!(detect_shell_expansion_token("*.rs").is_some());
        assert!(detect_shell_expansion_token("file?.c").is_some());
        assert!(detect_shell_expansion_token("src/[ab].rs").is_some());
        assert!(detect_shell_expansion_token("foo?").is_none());
        assert!(detect_shell_expansion_token("--pretty=format:[%h]").is_none());
        assert!(!argv_needs_posix_shell_wrap(
            "rg",
            &["-e".into(), "foo?".into()]
        ));
        assert!(!argv_needs_posix_shell_wrap(
            "git",
            &["log".into(), "--pretty=format:[%h]".into()]
        ));
        let mut cmd = "rg".to_string();
        let mut args = vec!["-e".to_string(), "foo?".to_string()];
        let allowed = vec!["rg".into(), "bash".into()];
        assert!(
            maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &allowed).is_none(),
            "foo? 不按 glob 处理，不应包装"
        );
    }

    #[test]
    fn cd_prefix_and_not_treated_as_operators() {
        let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(
            "cd",
            &["src".into(), "&&".into(), "git".into(), "status".into()],
        );
        assert_eq!(cmd, "git");
        assert_eq!(args, vec!["status".to_string()]);
        assert!(!argv_needs_posix_shell_wrap(&cmd, &args));
    }

    #[test]
    fn operators_need_wrap_after_peel() {
        let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(
            "cd",
            &[
                "src".into(),
                "&&".into(),
                "ls".into(),
                "&&".into(),
                "pwd".into(),
            ],
        );
        assert_eq!(cmd, "ls");
        assert!(argv_has_shell_operators(&cmd, &args));
        assert!(argv_needs_posix_shell_wrap(&cmd, &args));
    }

    #[test]
    fn allowlist_allows_all_detects_star_sentinel() {
        assert!(allowlist_allows_all(&["*".into()]));
        assert!(allowlist_allows_all(&["ls".into(), " * ".into()]));
        assert!(!allowlist_allows_all(&["ls".into(), "git".into()]));
        assert!(!allowlist_allows_all(&[]));
    }

    #[test]
    fn posix_shell_on_allowlist_prefers_bash_then_sh() {
        assert_eq!(posix_shell_on_allowlist(&["bash".into()]), Some("bash"));
        assert_eq!(posix_shell_on_allowlist(&["sh".into()]), Some("sh"));
        assert_eq!(
            posix_shell_on_allowlist(&["sh".into(), "BASH".into()]),
            Some("bash")
        );
        assert_eq!(posix_shell_on_allowlist(&["ls".into()]), None);
        assert_eq!(posix_shell_on_allowlist(&[]), None);
    }

    #[test]
    fn star_sentinel_behaves_as_bash_allowed() {
        let star = vec!["*".to_string()];
        assert_eq!(posix_shell_on_allowlist(&star), Some("bash"));
        let mut cmd = "ls".to_string();
        let mut args = vec!["*.rs".to_string()];
        assert!(maybe_wrap_argv_with_posix_shell(&mut cmd, &mut args, &star).is_some());
        assert_eq!(cmd, "bash");
        assert_eq!(args, vec!["-c".to_string(), "ls *.rs".to_string()]);
    }
}
