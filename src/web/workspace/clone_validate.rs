//! 项目池 clone：URL 校验、进度行解析、日志脱敏。

use regex::Regex;
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

/// Clone 墙钟超时（规划已拍板；MVP 不可配）。
pub const WORKSPACE_CLONE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// 单条推送给前端的日志行最大字符数。
pub const CLONE_LOG_LINE_MAX_CHARS: usize = 500;

static RE_RECEIVING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Receiving objects:\s+(\d+)%").expect("regex"));
static RE_RESOLVING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Resolving deltas:\s+(\d+)%").expect("regex"));
static RE_COMPRESSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Compressing objects:\s+(\d+)%").expect("regex"));
static RE_CRED_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(https?://)([^/@\s]+):([^/@\s]+)@").expect("regex"));
static RE_BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._\-+/=]{8,}\b").expect("regex"));
static RE_BASIC_AUTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bBasic\s+[A-Za-z0-9+/=]{8,}\b").expect("regex"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneUrlError {
    Empty,
    UnsupportedScheme,
    Invalid,
}

impl CloneUrlError {
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Empty => "仓库 URL 不能为空",
            Self::UnsupportedScheme => {
                "仅支持 https://、http://、git@host:path 或 ssh:// 形式的仓库 URL"
            }
            Self::Invalid => "仓库 URL 无效",
        }
    }
}

/// 校验远程 clone URL（MVP：允许 http(s)、ssh、git@；拒绝 file 等）。
pub fn validate_clone_repo_url(raw: &str) -> Result<&str, CloneUrlError> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(CloneUrlError::Empty);
    }
    if url_has_illegal_chars(url) {
        return Err(CloneUrlError::Invalid);
    }
    let lower = url.to_ascii_lowercase();
    if let Some(r) = try_validate_http_clone_url(url, &lower) {
        return r;
    }
    if let Some(r) = try_validate_ssh_clone_url(url, &lower) {
        return r;
    }
    if let Some(r) = try_validate_git_at_clone_url(url) {
        return r;
    }
    if lower.starts_with("file:") || lower.starts_with("ftp:") {
        return Err(CloneUrlError::UnsupportedScheme);
    }
    Err(CloneUrlError::UnsupportedScheme)
}

fn url_has_illegal_chars(url: &str) -> bool {
    url.contains('\0') || url.chars().any(|c| c.is_control())
}

fn try_validate_http_clone_url<'a>(
    url: &'a str,
    lower: &str,
) -> Option<Result<&'a str, CloneUrlError>> {
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return None;
    }
    Some(if url.len() < 12 {
        Err(CloneUrlError::Invalid)
    } else {
        Ok(url)
    })
}

fn try_validate_ssh_clone_url<'a>(
    url: &'a str,
    lower: &str,
) -> Option<Result<&'a str, CloneUrlError>> {
    if !lower.starts_with("ssh://") {
        return None;
    }
    Some(if url.len() < 10 {
        Err(CloneUrlError::Invalid)
    } else {
        Ok(url)
    })
}

fn try_validate_git_at_clone_url(url: &str) -> Option<Result<&str, CloneUrlError>> {
    let rest = url.strip_prefix("git@")?;
    Some(if rest.contains(':') && !rest.contains("://") {
        Ok(url)
    } else {
        Err(CloneUrlError::Invalid)
    })
}

/// 可选分支名：拒绝空（调用方已滤）、控制字符、以及以 `-` 开头（避免歧义）。
pub fn validate_clone_branch(raw: &str) -> Result<&str, &'static str> {
    let b = raw.trim();
    if b.is_empty() {
        return Err("分支名不能为空");
    }
    if b.len() > 255 {
        return Err("分支名过长");
    }
    if b.starts_with('-') {
        return Err("分支名不能以 '-' 开头");
    }
    if b.contains('\0') || b.chars().any(|c| c.is_control()) {
        return Err("分支名含非法字符");
    }
    if b.contains("..") || b.contains('\\') {
        return Err("分支名无效");
    }
    Ok(b)
}

/// 从 git `--progress` 行尽力解析百分比。
pub fn parse_clone_progress_percent(line: &str) -> Option<(u8, &'static str)> {
    if let Some(c) = RE_RECEIVING.captures(line) {
        let p: u8 = c.get(1)?.as_str().parse().ok()?;
        return Some((p.min(100), "Receiving objects"));
    }
    if let Some(c) = RE_RESOLVING.captures(line) {
        let p: u8 = c.get(1)?.as_str().parse().ok()?;
        return Some((p.min(100), "Resolving deltas"));
    }
    if let Some(c) = RE_COMPRESSING.captures(line) {
        let p: u8 = c.get(1)?.as_str().parse().ok()?;
        return Some((p.min(100), "Compressing objects"));
    }
    None
}

/// 脱敏：去掉 URL 内嵌 `user:pass@`、Bearer / Basic 形态，并截断过长行。
pub fn redact_clone_log_line(line: &str) -> String {
    let cleaned = RE_CRED_URL.replace_all(line.trim(), "${1}***:***@");
    let cleaned = RE_BEARER.replace_all(cleaned.as_ref(), "Bearer <redacted>");
    let cleaned = RE_BASIC_AUTH.replace_all(cleaned.as_ref(), "Basic <redacted>");
    let s = cleaned.as_ref();
    if s.chars().count() <= CLONE_LOG_LINE_MAX_CHARS {
        return s.to_string();
    }
    let truncated: String = s.chars().take(CLONE_LOG_LINE_MAX_CHARS).collect();
    format!("{truncated}…")
}

/// 按 `\n` / `\r` 切分管道块，保留未完成尾部。
pub fn split_progress_chunks(buf: &mut String, chunk: &str) -> Vec<String> {
    buf.push_str(chunk);
    let mut out = Vec::new();
    while let Some(pos) = buf.find(['\n', '\r']) {
        let mut line = buf[..pos].to_string();
        buf.drain(..=pos);
        // 吞掉 \r\n 的多余一侧
        if buf.starts_with('\n') {
            buf.remove(0);
        }
        line = line.trim_end_matches('\r').to_string();
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// SSE `error.message` 附带的 stderr 尾部：最多 N 行 / 总字符上限（**采集时已脱敏**）。
///
/// 只进内存与 SSE 帧、不落盘；失败路径会把半成品目录删掉，故摘要不是唯一信息源
/// （逐行 `log` 事件已推过），只是让「只看错误提示」的用户也能看到原因。
pub const STDERR_TAIL_LINES: usize = 3;
pub const STDERR_TAIL_TOTAL_CHARS: usize = 400;

/// 摘要前缀：点明来源（git 的 stderr），避免用户以为是本服务自己的文案。
pub const TAIL_SUMMARY_PREFIX: &str = "git 输出（尾部）：";

/// 记录 stderr 尾部（**已脱敏**），只保留最后 [`STDERR_TAIL_LINES`] 行。
pub fn push_stderr_tail(tail: &Mutex<VecDeque<String>>, line: &str) {
    if line.trim().is_empty() {
        return;
    }
    let Ok(mut q) = tail.lock() else {
        return;
    };
    q.push_back(line.to_string());
    while q.len() > STDERR_TAIL_LINES {
        q.pop_front();
    }
}

/// 只有**非进度**行才进尾部。
///
/// `git clone --progress` 的进度也走 **stderr**（`Receiving objects: 45% …`），放任它
/// 会把 [`STDERR_TAIL_LINES`] 的配额占满，真正的 `fatal:` 反而被挤出去——那等于本功能
/// 白做。进度信息另走 `progress` 事件，不需要在摘要里重复。
pub fn should_keep_stderr_tail_line(line: &str) -> bool {
    parse_clone_progress_percent(line).is_none()
}

/// 退出码短语（**不含外层括号**，由调用方包一次，避免「失败（被信号终止（无退出码））」这种嵌套）。
pub fn format_clone_exit_code(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(c) => format!("退出码 {c}"),
        None => "进程被信号终止，无退出码".to_string(),
    }
}

/// 把 stderr 尾部拼成一段可读摘要（输入**必须**已脱敏）。
///
/// 总长按 [`STDERR_TAIL_TOTAL_CHARS`] 截断；空输入返回 `None`（不追加空行）。
/// 分隔符用 ASCII ` / `：`⏎` 之类符号在部分字体下会渲染成方块。
pub fn stderr_tail_summary(tail: &[String]) -> Option<String> {
    let joined = tail
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    if joined.is_empty() {
        return None;
    }
    let mut chars = joined.chars();
    let truncated: String = chars.by_ref().take(STDERR_TAIL_TOTAL_CHARS).collect();
    let suffix = if chars.next().is_some() { "…" } else { "" };
    Some(format!("{TAIL_SUMMARY_PREFIX}{truncated}{suffix}"))
}

/// 把摘要追加到 message 末尾（摘要为空时不追加）。
pub fn with_stderr_tail(mut message: String, tail: &[String]) -> String {
    if let Some(s) = stderr_tail_summary(tail) {
        message.push('\n');
        message.push_str(&s);
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_and_git_at() {
        assert!(validate_clone_repo_url("https://github.com/a/b.git").is_ok());
        assert!(validate_clone_repo_url("git@github.com:a/b.git").is_ok());
        assert!(validate_clone_repo_url("ssh://git@host/a/b.git").is_ok());
    }

    #[test]
    fn rejects_file_and_empty() {
        assert_eq!(
            validate_clone_repo_url("file:///tmp/x"),
            Err(CloneUrlError::UnsupportedScheme)
        );
        assert_eq!(validate_clone_repo_url("  "), Err(CloneUrlError::Empty));
    }

    #[test]
    fn rejects_bad_branch() {
        assert!(validate_clone_branch("-main").is_err());
        assert!(validate_clone_branch("ma\nin").is_err());
        assert_eq!(validate_clone_branch("feature/x").unwrap(), "feature/x");
    }

    #[test]
    fn parses_receiving_percent() {
        assert_eq!(
            parse_clone_progress_percent("Receiving objects:  42% (12/28)"),
            Some((42, "Receiving objects"))
        );
    }

    #[test]
    fn redacts_embedded_basic_auth() {
        let s = redact_clone_log_line("fatal: https://user:secret@github.com/a/b.git");
        assert!(!s.contains("secret"));
        assert!(s.contains("***:***@"));
    }

    #[test]
    fn redacts_bearer_token() {
        let s = redact_clone_log_line("header Authorization: Bearer ghp_abcdefghijklmnop");
        assert!(s.contains("Bearer <redacted>"));
        assert!(!s.contains("ghp_"));
    }

    #[test]
    fn redacts_basic_auth() {
        let s = redact_clone_log_line(
            "header Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2h1X3Rlc3RfdG9rZW4=",
        );
        assert!(s.contains("Basic <redacted>"));
        assert!(!s.contains("eC1hY2Nlc3M"));
    }

    #[test]
    fn split_cr_progress() {
        let mut buf = String::new();
        let lines =
            split_progress_chunks(&mut buf, "Receiving objects: 10%\rReceiving objects: 20%\n");
        assert_eq!(lines.len(), 2);
        assert!(buf.is_empty());
    }

    /// 摘要边界：空输入不追加、超长截断、多行用 ASCII 分隔。
    #[test]
    fn stderr_tail_summary_is_bounded() {
        assert_eq!(stderr_tail_summary(&[]), None);
        assert_eq!(stderr_tail_summary(&["   ".to_string()]), None);

        let long = vec!["x".repeat(STDERR_TAIL_TOTAL_CHARS + 50)];
        let s = stderr_tail_summary(&long).expect("非空输入应有摘要");
        assert!(s.starts_with(TAIL_SUMMARY_PREFIX), "{s}");
        assert!(s.ends_with('…'), "超长应截断：{s}");
        assert!(
            s.chars().count() <= STDERR_TAIL_TOTAL_CHARS + TAIL_SUMMARY_PREFIX.chars().count() + 1
        );

        // 多行用 ASCII 分隔符（`⏎` 等符号在部分字体下渲染成方块）。
        let two = stderr_tail_summary(&["a".to_string(), "b".to_string()]).expect("非空");
        assert_eq!(two, "git 输出（尾部）：a / b");
    }

    /// `--progress` 的进度行也走 stderr；不得占用尾部配额，否则 `error.message`
    /// 只会出现 `Receiving objects: 45%` 而不是 `fatal:`。
    #[test]
    fn progress_lines_do_not_consume_tail_budget() {
        let tail = Mutex::new(VecDeque::new());
        for line in [
            "Receiving objects:  45% (5/12), 1.20 MiB | 500.00 KiB/s",
            "Resolving deltas: 99% (9/10)",
            "fatal: could not read from remote repository",
        ] {
            if should_keep_stderr_tail_line(line) {
                push_stderr_tail(&tail, line);
            }
        }
        let q = tail.lock().expect("lock");
        let kept: Vec<&str> = q.iter().map(String::as_str).collect();
        assert_eq!(kept, ["fatal: could not read from remote repository"]);
    }

    /// 尾部只保留最后 `STDERR_TAIL_LINES` 行；空白行不入队。
    #[test]
    fn push_stderr_tail_keeps_last_lines() {
        let tail = Mutex::new(VecDeque::new());
        for i in 0..(STDERR_TAIL_LINES + 3) {
            push_stderr_tail(&tail, &format!("line{i}"));
        }
        push_stderr_tail(&tail, "   ");
        let q = tail.lock().expect("lock");
        assert_eq!(q.len(), STDERR_TAIL_LINES);
        let kept: Vec<&str> = q.iter().map(String::as_str).collect();
        assert_eq!(kept, ["line3", "line4", "line5"]);
    }

    /// 退出码短语：不得出现 `Some(128)` / `None` 的 Debug 形态，也不得自带括号。
    #[test]
    fn exit_code_phrase_has_no_debug_or_parens() {
        assert_eq!(format_clone_exit_code(Some(128)), "退出码 128");
        assert_eq!(format_clone_exit_code(None), "进程被信号终止，无退出码");
        for phrase in [
            format_clone_exit_code(Some(1)),
            format_clone_exit_code(None),
        ] {
            assert!(!phrase.contains("Some("), "{phrase}");
            assert!(!phrase.contains('（'), "{phrase}");
        }
    }
}
