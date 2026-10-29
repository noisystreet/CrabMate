//! 输出层：人读表格与 `--json`。
//!
//! `--json` 是给脚本与客户端的**主契约**（状态文件只是可断线读取的缓存），
//! 字段名与状态文件保持一致，新增字段只增不删。

use serde_json::json;

use super::ops::{DownOutcome, UpOutcome};
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

/// `status` 输出。
pub fn status(rec: &InstanceRecord, p: &Probe, as_json: bool) {
    if as_json {
        let mut v = row_json(rec, p);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("identity".to_string(), json!(identity_str(p)));
            obj.insert("pid_alive".to_string(), json!(p.pid_alive));
            obj.insert("tcp_ok".to_string(), json!(p.tcp_ok));
        }
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
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
pub fn prune(items: &[(String, Observed)], dry_run: bool) {
    if items.is_empty() {
        println!("没有可清理的记录");
        return;
    }
    for (name, obs) in items {
        if dry_run {
            println!("将清理 {name}（{}）", obs.as_str());
        } else {
            println!("已清理 {name}（{}）", obs.as_str());
        }
    }
    if dry_run {
        println!("（dry-run：未实际删除；去掉 --dry-run 执行）");
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
                spec.workspace
            )
        }
        Step::Stop { name, reason } => {
            let mark = if dry_run { "−" } else { "○" };
            format!("{mark} {name:<16} 回收（{reason}）")
        }
        Step::Running { name } => format!("● {name:<16} 已就绪（无动作）"),
        Step::Stopped { name } => format!("○ {name:<16} 已回收"),
        Step::Skipped { name, reason } => format!("· {name:<16} 跳过：{reason}"),
        Step::Failed { name, error } => format!("× {name:<16} 失败：{error}"),
    }
}
