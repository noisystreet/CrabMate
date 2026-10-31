//! 输出层：人读表格与 `--json`。
//!
//! `--json` 是给脚本与客户端的**主契约**（状态文件只是可断线读取的缓存），
//! 字段名与状态文件保持一致，新增字段只增不删。

use serde_json::json;

use super::ops::{DownOutcome, PruneItem, UpOutcome};
use super::probe::{Observed, Probe};
use super::record::InstanceRecord;
use super::supervise::Step;

/// `started_at` → 已运行秒数。
fn uptime_secs(rec: &InstanceRecord) -> Option<i64> {
    let started = rec.started_at.as_deref()?;
    let t = chrono::DateTime::parse_from_rfc3339(started).ok()?;
    Some((chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds().max(0))
}

fn human_uptime(secs: Option<i64>) -> String {
    match secs {
        None => "-".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m{}s", s / 60, s % 60),
        Some(s) => format!("{}h{}m", s / 3600, (s % 3600) / 60),
    }
}

/// 单行 JSON（`ls --json` 的数组元素）。
pub fn row_json(rec: &InstanceRecord, p: &Probe) -> serde_json::Value {
    json!({
        "name": rec.name,
        "state": p.observed.as_str(),
        "desired_state": rec.desired_state.as_str(),
        "base_url": rec.base_url,
        "port": rec.port,
        "pid": rec.pid,
        "pgid": rec.pgid,
        "started_at": rec.started_at,
        "uptime_secs": uptime_secs(rec),
        "last_error": rec.last_error,
        "config_in_use": rec.config_in_use,
        "workspace": rec.workspace,
        "log": rec.log,
        "env_keys": rec.env_keys,
        "profile": rec.profile,
        "profile_in_use": rec.profile_in_use,
        // 退避重启进度：`--restart on-failure` 关闭时恒为 0 / null
        "restart_attempts": rec.restart_attempts,
        "next_restart_at": rec.next_restart_at,
        // 工具面的一部分：`--yes` 决定子实例的命令白名单是否生效，
        // 客户端要能展示它，不能只让人读 `status` 看得见。
        "yes": rec.yes,
    })
}

fn state_marker(observed: Observed) -> &'static str {
    match observed {
        Observed::Ready => "●",
        Observed::Starting => "◐",
        Observed::Held => "◐",
        Observed::Stale => "△",
        Observed::Stopped => "○",
        Observed::Failed => "×",
    }
}

/// `ls` 输出。
pub fn ls(rows: &[(InstanceRecord, Probe)], as_json: bool) {
    if as_json {
        let arr: Vec<serde_json::Value> = rows.iter().map(|(r, p)| row_json(r, p)).collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        return;
    }
    if rows.is_empty() {
        println!("（没有实例）");
        return;
    }
    println!(
        "{:<3} {:<16} {:<10} {:<26} {:<8} {:<8} WORKSPACE",
        "", "NAME", "STATE", "BASE_URL", "PID", "UPTIME"
    );
    for (rec, p) in rows {
        println!(
            "{:<3} {:<16} {:<10} {:<26} {:<8} {:<8} {}",
            state_marker(p.observed),
            rec.name,
            p.observed.as_str(),
            rec.base_url.as_deref().unwrap_or("-"),
            rec.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            human_uptime(uptime_secs(rec)),
            rec.workspace.as_deref().unwrap_or("-"),
        );
    }
}

/// `status --json` 的形状：`ls` 的一行 + 三项进程身份细节。
///
/// 抽成函数是为了能被契约测试直接比对键集：这份结构由 `crabmate-client` 直接消费，
/// 内联在 `println!` 里就只能靠「输出看起来对」来保证。
pub fn status_json(rec: &InstanceRecord, p: &Probe) -> serde_json::Value {
    let mut v = row_json(rec, p);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("identity".to_string(), json!(identity_str(p)));
        obj.insert("pid_alive".to_string(), json!(p.pid_alive));
        obj.insert("tcp_ok".to_string(), json!(p.tcp_ok));
    }
    v
}

/// `status` 输出。
pub fn status(rec: &InstanceRecord, p: &Probe, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status_json(rec, p)).unwrap_or_default()
        );
        return;
    }
    println!("实例: {}", rec.name);
    println!("  状态: {}{}", state_marker(p.observed), p.observed.as_str());
    println!("  期望态: {}", rec.desired_state.as_str());
    println!("  base_url: {}", rec.base_url.as_deref().unwrap_or("-"));
    println!("  端口: {}", rec.port.map(|v| v.to_string()).unwrap_or_else(|| "-".into()));
    println!("  pid/pgid: {:?}/{:?}", rec.pid, rec.pgid);
    println!("  已运行: {}", human_uptime(uptime_secs(rec)));
    println!("  进程身份: {}", identity_str(p));
    println!("  TCP 可达: {}", p.tcp_ok);
    println!("  workspace: {}", rec.workspace.as_deref().unwrap_or("-"));
    println!("  声明 config: {}", rec.config.as_deref().unwrap_or("-"));
    println!("  实际 config: {}", rec.config_in_use.as_deref().unwrap_or("-"));
    println!("  日志: {}", rec.log.as_deref().unwrap_or("-"));
    println!("  环境变量(名): {}", rec.env_keys.join(", "));
    // `--yes` 决定工具面的白名单是否生效，属于「这个实例到底是什么」的一部分；
    // 只存在记录里而不显示，用户就只能靠翻命令行历史去确认。
    println!(
        "  跳过命令白名单: {}",
        if rec.yes { "是（--yes）" } else { "否" }
    );
    println!("  profile(声明): {}", rec.profile.as_deref().unwrap_or("-"));
    println!("  profile(实际): {}", rec.profile_in_use.as_deref().unwrap_or("-"));
    if rec.profile != rec.profile_in_use {
        println!(
            "  提示: profile 已变更但实例未重启；执行 `crabmatectl restart {}` 才生效",
            rec.name
        );
    }
    if rec.restart_attempts > 0 || rec.next_restart_at.is_some() {
        println!(
            "  重启退避: 连续 {} 次，下次允许 {}（`--restart on-failure`）",
            rec.restart_attempts,
            rec.next_restart_at.as_deref().unwrap_or("-")
        );
    }
    if let Some(e) = rec.last_error.as_deref() {
        println!("  最近错误: {e}");
    }
}

fn identity_str(p: &Probe) -> &str {
    if p.identity.is_match() {
        "ok"
    } else {
        p.identity.reason().unwrap_or("—")
    }
}

/// 预检提示：走 stderr，不污染 `--json` 的 stdout。
pub fn warn(items: &[String]) {
    for w in items {
        eprintln!("WARN: {w}");
    }
}

/// `up` 输出。
pub fn up(out: &UpOutcome) {
    warn(&out.warnings);
    let rec = &out.record;
    if out.already_running {
        println!(
            "实例 {} 已在运行（未重复启动）：{}",
            rec.name,
            rec.base_url.as_deref().unwrap_or("-")
        );
        return;
    }
    match out.observed {
        Observed::Ready => println!(
            "实例 {} 已就绪：{}（pid {}）",
            rec.name,
            rec.base_url.as_deref().unwrap_or("-"),
            rec.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
        ),
        other => {
            println!("实例 {} 当前状态：{}", rec.name, other.as_str());
            if let Some(e) = rec.last_error.as_deref() {
                println!("  {e}");
            }
        }
    }
}

/// `down` 输出。
pub fn down(out: &DownOutcome) {
    if out.already_stopped {
        println!("实例 {} 未在运行（无需停止）", out.name);
        return;
    }
    if out.forced {
        println!(
            "实例 {} 已强制终止（宽限期内未退出，已用 SIGKILL）",
            out.name
        );
        println!("  提示：serve 的优雅关停可能较慢；可用 `--timeout <SECS>` 放宽宽限期（见 `ctl logs`）");
    } else {
        println!("实例 {} 已停止", out.name);
    }
}

/// `prune` 输出。
///
/// 被跳过的条目必须单独一行说明**为什么**：它们最需要被看见（记录指向活进程），
/// 混进「没清掉」里等于把最值得处理的情况静音。
pub fn prune(items: &[PruneItem], dry_run: bool) {
    if items.is_empty() {
        println!("没有可清理的记录");
        return;
    }
    for item in items {
        if let Some(reason) = item.refused.as_deref() {
            println!("跳过 {}（{}）：{reason}", item.name, item.observed.as_str());
            continue;
        }
        if dry_run {
            println!("将清理 {}（{}）", item.name, item.observed.as_str());
        } else {
            println!("已清理 {}（{}）", item.name, item.observed.as_str());
        }
    }
    if dry_run {
        println!("（dry-run：未实际删除；去掉 --dry-run 执行）");
    }
    // 被跳过时 `--force` 本来就是 false（见 `ops::prune`），所以这里不必再问它：
    // 有 refused 条目就说明用户没加 `--force`。
    if items.iter().any(|i| i.refused.is_some()) {
        println!(
            "提示：跳过项由 `crabmatectl prune --force` 强制清理（会让仍在运行的进程失去记录）"
        );
    }
}

/// `supervise` 输出：一行一个实例，先给动作再给原因。
pub fn supervise(steps: &[Step], dry_run: bool) {
    if dry_run {
        println!("（dry-run：只打印计划，不启动也不回收）");
    }
    if steps.is_empty() {
        println!("（声明为空，且没有需要回收的实例）");
        return;
    }
    for step in steps {
        println!("{}", supervise_line(step, dry_run));
    }
}

/// 声明不可用时的告警（走 stderr，并明确说「已暂停收敛」）。
pub fn supervise_refused(reason: &str) {
    eprintln!("WARN: 声明文件当前不可用，已暂停收敛（不启动、不回收）：{reason}");
}

/// 健康巡检的结果：**只在真有动作时输出**。
///
/// 被退避挡下的尝试刻意静默（`ctl status` 里能看到 `next_restart_at`）：
/// 崩溃循环最糟的输出是每 30 秒一条一模一样的日志。
pub fn restart(steps: &[Step]) {
    for step in steps {
        println!("{}", supervise_line(step, false));
    }
}

fn supervise_line(step: &Step, dry_run: bool) -> String {
    match step {
        Step::Start {
            name,
            spec,
            restart,
            reason,
        } => {
            let verb = if *restart { "重启" } else { "启动" };
            let port = spec
                .port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "自动".to_string());
            let mark = if dry_run { "+" } else { "●" };
            format!(
                "{mark} {name:<16} {verb}：端口 {port}，workspace {}（{reason}）",
                spec.workspace.as_deref().unwrap_or("（待绑定：等 client 设置）")
            )
        }
        Step::Stop { name, reason } => {
            let mark = if dry_run { "−" } else { "○" };
            format!("{mark} {name:<16} 回收（{reason}）")
        }
        Step::Running { name } => format!("● {name:<16} 已就绪（无动作）"),
        Step::Started {
            name,
            restarted,
            reason,
        } => {
            // 显式回显「重启了什么、为什么」，否则用户只能从日志里猜
            let verb = if *restarted { "已重启" } else { "已启动" };
            format!("● {name:<16} {verb}（{reason}）")
        }
        Step::Stopped { name } => format!("○ {name:<16} 已回收"),
        Step::Skipped { name, reason } => format!("· {name:<16} 跳过：{reason}"),
        Step::Failed { name, error } => format!("× {name:<16} 失败：{error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::cm_ctl::supervise::{StartSpec, Step};

    use super::supervise_line;

    /// 计划行对待绑定实例必须显示「待绑定」，不能借 `run_command_working_dir`
    /// 或任何占位路径把它伪装成已选目录（#1183 §2 ②：`ls` / `status` / `--dry-run`
    /// 都要让「未绑定」对人可见）。
    #[test]
    fn dry_run_marks_pending_workspace() {
        let spec = StartSpec {
            workspace: None,
            serve_config: None,
            port: Some(8081),
            env: BTreeMap::new(),
            profile: None,
            profile_fingerprint: None,
            no_tools: false,
        };
        let line = supervise_line(
            &Step::Start {
                name: "alpha".to_string(),
                spec,
                restart: false,
                reason: "声明新增".to_string(),
            },
            true,
        );

        assert!(line.starts_with('+'), "dry-run 用 `+` 前缀：{line}");
        assert!(
            line.contains("待绑定"),
            "待绑定实例的计划行必须写明「等 client 设置」，否则用户会以为它已有工作区：{line}"
        );
    }

    /// 对照组：有工作区时照原样回显路径——否则上一条的占位文案会把真实路径也吃掉。
    #[test]
    fn dry_run_shows_bound_workspace_path() {
        let spec = StartSpec {
            workspace: Some("/tmp/proj-a".to_string()),
            serve_config: None,
            port: None,
            env: BTreeMap::new(),
            profile: None,
            profile_fingerprint: None,
            no_tools: false,
        };
        let line = supervise_line(
            &Step::Start {
                name: "alpha".to_string(),
                spec,
                restart: true,
                reason: "profile 变更".to_string(),
            },
            true,
        );

        assert!(line.contains("/tmp/proj-a"), "{line}");
        assert!(
            !line.contains("待绑定"),
            "已绑定的实例不应显示待绑定：{line}"
        );
    }
}
