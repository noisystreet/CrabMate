//! 将 `run_command` 的 command+args 拼成一行脚本，并在白名单含 `bash`/`sh` 时经 `bash -c` 执行。

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

/// 白名单中优先 `bash`，否则 `sh`。
#[must_use]
pub fn posix_shell_on_allowlist(allowed: &[String]) -> Option<&'static str> {
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
        if args.len() < 3 || args[1] != "&&" {
            break;
        }
        cmd = args[2].clone();
        args = args[3..].to_vec();
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

/// glob / `$VAR` / 反引号 / `~`（不含整段 `&&`/`|` 等操作符）。
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

/// 返回第一个命中的模式标签（供错误/审批文案）。
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
                    return Some("$(...)");
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
            b'`' => return Some("`...`"),
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

fn double_quote_token(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len().saturating_add(2));
    s.push('"');
    for ch in arg.chars() {
        if ch == '"' || ch == '\\' {
            s.push('\\');
        }
        s.push(ch);
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
    if detect_shell_expansion_token(arg).is_some() {
        if arg.chars().any(char::is_whitespace) {
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

/// 拼成一行脚本（供 `bash -c` 与审批展示）。
#[must_use]
pub fn join_run_command_shell_script(cmd: &str, args: &[String]) -> String {
    let cmd = cmd.trim();
    if args.is_empty() {
        return cmd.to_string();
    }
    let mut out = String::from(cmd);
    for a in args {
        out.push(' ');
        out.push_str(&quote_script_token(a));
    }
    out
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

/// 若白名单含 bash/sh 且 argv 需要展开或含操作符，则改写为 `bash -c` / `sh -c`。
pub fn maybe_wrap_argv_with_posix_shell(
    cmd_raw: &mut String,
    cmd_args: &mut Vec<String>,
    allowed_commands: &[String],
) -> bool {
    if is_shell_dash_c_invocation(cmd_raw, cmd_args) {
        return false;
    }
    let Some(shell) = posix_shell_on_allowlist(allowed_commands) else {
        return false;
    };
    if !argv_needs_posix_shell_wrap(cmd_raw, cmd_args) {
        return false;
    }
    let script = join_run_command_shell_script(cmd_raw, cmd_args);
    *cmd_raw = shell.to_string();
    *cmd_args = vec!["-c".to_string(), script];
    true
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
    fn wrap_when_bash_allowed_and_glob() {
        let mut cmd = "ls".to_string();
        let mut args = vec!["*.rs".to_string()];
        let allowed = vec!["ls".into(), "bash".into()];
        assert!(maybe_wrap_argv_with_posix_shell(
            &mut cmd, &mut args, &allowed
        ));
        assert_eq!(cmd, "bash");
        assert_eq!(args, vec!["-c".to_string(), "ls *.rs".to_string()]);
    }

    #[test]
    fn no_wrap_without_shell_features() {
        let mut cmd = "git".to_string();
        let mut args = vec!["status".to_string()];
        let allowed = vec!["git".into(), "bash".into()];
        assert!(!maybe_wrap_argv_with_posix_shell(
            &mut cmd, &mut args, &allowed
        ));
        assert_eq!(cmd, "git");
    }

    #[test]
    fn no_double_wrap_bash_c() {
        let mut cmd = "bash".to_string();
        let mut args = vec!["-c".to_string(), "echo $HOME".to_string()];
        let allowed = vec!["bash".into()];
        assert!(!maybe_wrap_argv_with_posix_shell(
            &mut cmd, &mut args, &allowed
        ));
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
        assert!(!maybe_wrap_argv_with_posix_shell(
            &mut cmd, &mut args, &allowed
        ));
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
}
