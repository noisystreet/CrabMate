//! 进程身份校验：发信号**之前**必须确认「这个 pid 还是我们起的那个进程」。
//!
//! 状态文件里的 `pid` / `pgid` 是历史值，而 PID 会被系统复用。若不加校验，
//! 一次 `down` 就可能对**毫不相关的进程组**发 `SIGTERM`。三重判据与
//! systemd 的 `MainPID` + `ExecMainStartTimestamp` 同构：
//!
//! 1. `boot_id`：不一致 ⇒ 记录来自上次开机，直接判失效，不碰任何进程；
//! 2. `pid_start_ticks`：`/proc/<pid>/stat` 第 22 字段，同 pid 换了进程就不等；
//! 3. `cmdline`：包含 `serve` 且包含该实例的 workspace / port。
//!
//! **平台前提：仅 Linux。** 三重判据全部读 `/proc`；macOS 没有 procfs，本模块的
//! 每个探针都会返回 `None` / `false`，而那只意味着「读不到事实」，**不等于**
//! 「进程已死」。把两者混为一谈的后果很具体：`down` 会清掉 `pid` 并把记录写成
//! `stopped`，进程却在后台继续跑。因此非 Linux 平台由 `cm_ctl::PLATFORM_SUPPORTED`
//! 在入口拦下，不允许走到本模块的判定上。

use std::fs;
use std::path::PathBuf;

use super::record::InstanceRecord;

/// 身份校验结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// 记录的进程就是本人。
    Match,
    /// 记录里没有 pid（尚未 spawn 或已清理）。
    NoPid,
    /// 不一致：附可读原因（直接面向用户）。
    Mismatch(String),
}

impl Identity {
    pub fn is_match(&self) -> bool {
        matches!(self, Self::Match)
    }

    /// 供 `error_code` 与展示层复用。
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Mismatch(r) => Some(r.as_str()),
            _ => None,
        }
    }
}

/// 当前开机标识；非 Linux 或不可读时为 `None`。
pub fn boot_id() -> Option<String> {
    let raw = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn proc_stat_path(pid: u32) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}/stat"))
}

/// 解析 `/proc/<pid>/stat`，返回 (state 字段首字符, starttime)。
///
/// 注意 `comm` 字段可能含空格与括号，因此必须从**最后一个** `)` 之后开始切分：
/// 其后的第 1 个 token 是 field 3（state），field 22（starttime）对应下标 19。
fn parse_stat(text: &str) -> Option<(char, u64)> {
    let after_comm = text.rfind(')').map(|i| &text[i + 1..])?;
    let mut it = after_comm.split_whitespace();
    let state = it.next()?.chars().next()?;
    let starttime = it.nth(18)?.parse::<u64>().ok()?;
    Some((state, starttime))
}

/// 进程启动时刻（`/proc/<pid>/stat` 第 22 字段，单位 clock tick）。
pub fn pid_start_ticks(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(proc_stat_path(pid)).ok()?;
    parse_stat(&text).map(|(_, st)| st)
}

/// 进程命令行 token 列表（`/proc/<pid>/cmdline` 以 NUL 分隔）。
///
/// **必须按 token 比对**而不是子串：工作区路径里可能含 `server` 之类字样
/// （`/home/u/crabmate/server` 就包含 `serve`），子串匹配会让任何同前缀路径的
/// 无关进程都通过校验，等于没有校验。
pub fn cmdline_tokens(pid: u32) -> Option<Vec<String>> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    let tokens: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

/// 该值是否作为参数出现：独立 token 或 `--flag=value` 形式。
///
/// 前提是调用方已确认 `serve` 子命令存在；这里只回答「这个值在不在参数里」，
/// 不做前缀/子串匹配（`/w` 不应匹配 `/w/sub`）。
fn has_value_token(tokens: &[String], flag: &str, value: &str) -> bool {
    let inline = format!("{flag}={value}");
    tokens.iter().any(|t| t == value || t == &inline)
}

/// 进程是否仍活着（僵尸与已退出都算「不活」）。
///
/// ctl 会在 readiness 等待期间轮询它：子进程若已崩溃，`up` 要立刻失败而不是干等超时。
pub fn pid_alive(pid: u32) -> bool {
    let Ok(text) = fs::read_to_string(proc_stat_path(pid)) else {
        return false;
    };
    match parse_stat(&text).map(|(s, _)| s) {
        Some('Z') | Some('X') | Some('x') => false,
        Some(_) => true,
        None => false,
    }
}

/// 三重校验：`boot_id` → `pid_start_ticks` → `cmdline`。
pub fn verify(rec: &InstanceRecord) -> Identity {
    let Some(pid) = rec.pid else {
        return Identity::NoPid;
    };

    if let (Some(recorded), Some(current)) = (rec.boot_id.as_deref(), boot_id().as_deref())
        && recorded != current
    {
        return Identity::Mismatch(
            "记录来自上次开机（boot_id 已变化），不操作任何进程".to_string(),
        );
    }

    let Some(current_ticks) = pid_start_ticks(pid) else {
        return Identity::Mismatch(format!("pid {pid} 已不存在"));
    };
    if let Some(recorded) = rec.pid_start_ticks
        && recorded != current_ticks
    {
        return Identity::Mismatch(format!(
            "pid {pid} 已被其它进程复用（启动时刻不符），拒绝发信号"
        ));
    }

    let Some(tokens) = cmdline_tokens(pid) else {
        return Identity::Mismatch(format!("无法读取 pid {pid} 的 cmdline"));
    };
    // 子命令 token 必须**恰好**是 `serve`（不做子串判断，见 `cmdline_tokens` 注释）
    if !tokens.iter().any(|t| t == "serve") {
        return Identity::Mismatch(format!(
            "pid {pid} 的 cmdline 不含独立的 serve 子命令，疑似已被复用：{}",
            tokens.join(" ")
        ));
    }
    let ws_ok = rec
        .workspace
        .as_deref()
        .map(|ws| has_value_token(&tokens, "--workspace", ws))
        .unwrap_or(false);
    let port_ok = rec
        .port
        .map(|p| has_value_token(&tokens, "--port", &p.to_string()))
        .unwrap_or(false);
    if (rec.workspace.is_some() || rec.port.is_some()) && !ws_ok && !port_ok {
        return Identity::Mismatch(format!(
            "pid {pid} 的 cmdline 与该实例的 workspace/port 均不匹配：{}",
            tokens.join(" ")
        ));
    }

    Identity::Match
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_handles_comm_with_spaces_and_parens() {
        // comm 里含空格与括号也不能错位
        let text = "1234 (crab mate) (x) S 1 1234 1234 0 -1 4194560 100 0 0 0 \
                    5 6 0 0 20 0 1 0 918273 1000 100 18446744073709551615";
        let (state, starttime) = parse_stat(text).expect("应可解析");
        assert_eq!(state, 'S');
        assert_eq!(starttime, 918273);
    }

    #[test]
    fn parse_stat_rejects_garbage() {
        assert!(parse_stat("no parens here").is_none());
        assert!(parse_stat("(x) S 1").is_none());
    }

    #[test]
    fn self_pid_is_alive_and_has_cmdline() {
        let me = std::process::id();
        assert!(pid_alive(me), "自身进程应判活");
        assert!(pid_start_ticks(me).is_some(), "自身应有 starttime");
    }

    #[test]
    fn verify_without_pid_is_no_pid() {
        let rec = InstanceRecord::new("x");
        assert_eq!(verify(&rec), Identity::NoPid);
    }

    #[test]
    fn verify_rejects_when_boot_id_differs() {
        let mut rec = InstanceRecord::new("x");
        rec.pid = Some(std::process::id());
        rec.boot_id = Some("definitely-not-the-current-boot-id".to_string());
        let got = verify(&rec);
        assert!(matches!(got, Identity::Mismatch(_)));
        assert!(got.reason().unwrap_or_default().contains("boot_id"));
    }

    #[test]
    fn verify_rejects_reused_pid_by_start_ticks() {
        let me = std::process::id();
        let mut rec = InstanceRecord::new("x");
        rec.pid = Some(me);
        rec.boot_id = boot_id();
        // 真实的 ticks 之上加一个偏移，模拟「同 pid 不同进程」
        rec.pid_start_ticks = pid_start_ticks(me).map(|t| t.wrapping_add(1));
        let got = verify(&rec);
        assert!(
            matches!(&got, Identity::Mismatch(r) if r.contains("复用")),
            "应判为复用，实际 {got:?}"
        );
    }

    /// 回归：路径里含 `server` 的进程（如本测试二进制所在的 `.../crabmate/server/...`）
    /// 不能被子串匹配当成 `serve` 而通过校验 —— 否则身份校验形同虚设。
    #[test]
    fn verify_rejects_process_whose_cmdline_merely_contains_serve_substring() {
        let me = std::process::id();
        let mut rec = InstanceRecord::new("x");
        rec.pid = Some(me);
        rec.boot_id = boot_id();
        rec.pid_start_ticks = pid_start_ticks(me);
        let got = verify(&rec);
        assert!(
            matches!(&got, Identity::Mismatch(r) if r.contains("独立的 serve")),
            "非 serve 进程应被拒绝，实际 {got:?}"
        );
    }

    #[test]
    fn has_value_token_accepts_both_forms() {
        let sep = vec!["serve".to_string(), "--workspace".into(), "/w".into()];
        let inline = vec!["serve".to_string(), "--workspace=/w".into()];
        assert!(has_value_token(&sep, "--workspace", "/w"));
        assert!(has_value_token(&inline, "--workspace", "/w"));
        assert!(!has_value_token(&sep, "--workspace", "/other"));
        // 不做前缀匹配：`/w` 不应命中 `/w/sub`
        assert!(!has_value_token(&sep, "--workspace", "/w/sub"));
    }
}
