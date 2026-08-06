//! 项目池 clone：URL 校验、进度行解析、日志脱敏。

use regex::Regex;
use std::sync::LazyLock;

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
    if url.contains('\0') || url.chars().any(|c| c.is_control()) {
        return Err(CloneUrlError::Invalid);
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        if url.len() < 12 {
            return Err(CloneUrlError::Invalid);
        }
        return Ok(url);
    }
    if lower.starts_with("ssh://") {
        if url.len() < 10 {
            return Err(CloneUrlError::Invalid);
        }
        return Ok(url);
    }
    // git@host:path/to/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if rest.contains(':') && !rest.contains("://") {
            return Ok(url);
        }
        return Err(CloneUrlError::Invalid);
    }
    if lower.starts_with("file:") || lower.starts_with("ftp:") {
        return Err(CloneUrlError::UnsupportedScheme);
    }
    Err(CloneUrlError::UnsupportedScheme)
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

/// 脱敏：去掉 URL 内嵌 `user:pass@`，并截断过长行。
pub fn redact_clone_log_line(line: &str) -> String {
    let cleaned = RE_CRED_URL.replace_all(line.trim(), "${1}***:***@");
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
    fn split_cr_progress() {
        let mut buf = String::new();
        let lines =
            split_progress_chunks(&mut buf, "Receiving objects: 10%\rReceiving objects: 20%\n");
        assert_eq!(lines.len(), 2);
        assert!(buf.is_empty());
    }
}
