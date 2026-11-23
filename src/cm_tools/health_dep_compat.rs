//! 可选依赖与工具链的**版本兼容性**判定（供 `crate::cm_tools::health` 与启动日志共用）。
//!
//! 命令存在且退出码为 0 时仍可能版本过低；本模块在能解析出版本号时与最低要求比较。

use std::process::Command;

/// CrabMate 构建所需最低 Rust（与根 `Cargo.toml` edition 2024 / README 一致）。
pub const MIN_RUST_VERSION: (u64, u64, u64) = (1, 85, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepCompatKind {
    Rustc,
    Cargo,
    Python3,
    Npm,
    Gh,
    ClangFormat,
}

/// 在版本输出可解析时校验；不可解析则视为通过（仍保留原始 `detail` 供人读）。
pub fn validate_dep_version(kind: DepCompatKind, version_output: &str) -> Option<String> {
    let parsed = match kind {
        DepCompatKind::Rustc | DepCompatKind::Cargo => parse_rust_tool_version(version_output),
        DepCompatKind::Python3 => parse_python_version(version_output),
        DepCompatKind::Npm | DepCompatKind::Gh | DepCompatKind::ClangFormat => {
            parse_loose_semver(version_output)
        }
    }?;
    let min = min_version_for(kind);
    if version_at_least(parsed, min) {
        None
    } else {
        Some(format!(
            "版本 {} 低于建议最低 {}.{}.{}",
            format_triple(parsed),
            min.0,
            min.1,
            min.2
        ))
    }
}

fn min_version_for(kind: DepCompatKind) -> (u64, u64, u64) {
    match kind {
        DepCompatKind::Rustc | DepCompatKind::Cargo => MIN_RUST_VERSION,
        DepCompatKind::Python3 => (3, 8, 0),
        DepCompatKind::Npm => (6, 0, 0),
        DepCompatKind::Gh => (2, 50, 0),
        DepCompatKind::ClangFormat => (0, 0, 0),
    }
}

fn version_at_least(found: (u64, u64, u64), min: (u64, u64, u64)) -> bool {
    found.0 > min.0
        || (found.0 == min.0 && found.1 > min.1)
        || (found.0 == min.0 && found.1 == min.1 && found.2 >= min.2)
}

fn format_triple(v: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// 从 `rustc 1.85.0 (…)` / `cargo 1.85.0 (…)` 等输出中提取版本三元组。
pub fn parse_rust_tool_version(text: &str) -> Option<(u64, u64, u64)> {
    parse_loose_semver(text)
}

/// 从 `Python 3.11.2` 等输出中提取版本。
pub fn parse_python_version(text: &str) -> Option<(u64, u64, u64)> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("python")?;
    parse_loose_semver(&lower[idx..])
}

/// 在字符串中查找首个 `major.minor.patch` 数字序列。
pub fn parse_loose_semver(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts: Vec<u64> = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if !buf.is_empty() {
            if let Ok(n) = buf.parse::<u64>() {
                parts.push(n);
                buf.clear();
                if parts.len() >= 3 {
                    break;
                }
            } else {
                buf.clear();
            }
        }
    }
    if !buf.is_empty()
        && parts.len() < 3
        && let Ok(n) = buf.parse::<u64>()
    {
        parts.push(n);
    }
    match parts.as_slice() {
        [maj, min, pat] => Some((*maj, *min, *pat)),
        [maj, min] => Some((*maj, *min, 0)),
        _ => None,
    }
}

pub fn dep_compat_kind_for_health_key(dep_key: &str) -> Option<DepCompatKind> {
    match dep_key {
        "rustc" => Some(DepCompatKind::Rustc),
        "cargo" => Some(DepCompatKind::Cargo),
        "python3" => Some(DepCompatKind::Python3),
        "npm" => Some(DepCompatKind::Npm),
        "gh" => Some(DepCompatKind::Gh),
        "clang_format" => Some(DepCompatKind::ClangFormat),
        _ => None,
    }
}

/// 单项健康检查结果（与 `cm_internal` 的 `health::HealthCheckItem` 字段一致）。
#[derive(Debug, Clone)]
pub struct HealthCheckItem {
    pub ok: bool,
    pub detail: Option<String>,
}

/// 依赖探测的**结构化**结果。
///
/// 以前 `health` 侧把「未安装 / 非 0 退出」压成一条人读字符串，再由
/// `contains("No such file")` / `starts_with("exit=")` 反推状态——判定依赖文案，
/// 文案一改就静默失效。这里把状态留在结构里，展示文案只是它的渲染结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// 命令以 0 退出；字符串是展示用输出（空输出时为 `ok`）。
    Ok(String),
    /// spawn 失败且 `ErrorKind::NotFound`：命令不在 `PATH` 上。
    Missing { detail: String },
    /// spawn 失败但不是「未安装」（权限、坏解释器等）。
    SpawnFailed { detail: String },
    /// 已启动但退出码非 0，或被信号终止（此时 `code` 为 `None`）。
    Failed { code: Option<i32>, stderr: String },
}

/// 探测结论分类：`serve` 启动摘要按它分桶，**不再**读 `detail` 文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Ok,
    Missing,
    Broken,
    VersionIncompat,
}

/// 执行探测命令并保留结构化状态（`GET /health` 的依赖项用）。
pub fn capture_probe(cmd: &str, args: &[&str]) -> ProbeResult {
    match Command::new(cmd).args(args).output() {
        Ok(out) => match out.status.code() {
            Some(0) => {
                let s = if !out.stdout.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                } else {
                    String::from_utf8_lossy(&out.stderr).trim().to_string()
                };
                ProbeResult::Ok(if s.is_empty() { "ok".to_string() } else { s })
            }
            code => ProbeResult::Failed {
                code,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            },
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProbeResult::Missing {
            detail: e.to_string(),
        },
        Err(e) => ProbeResult::SpawnFailed {
            detail: e.to_string(),
        },
    }
}

/// `Failed` 的展示文案：保留历史 `exit=N` 措辞，有 stderr 时附首行便于定位。
fn failed_probe_detail(code: Option<i32>, stderr: &str) -> String {
    let mut out = format!("exit={}", code.unwrap_or(-1));
    if let Some(line) = stderr.lines().map(str::trim).find(|l| !l.is_empty()) {
        out.push_str(": ");
        out.push_str(&crate::text_util::truncate_chars_with_ellipsis(line, 160));
    }
    out
}

/// 将一次探测与其兼容性结论合并为 [`HealthCheckItem`] + [`ProbeKind`]。
///
/// `detail` 仍是**对外展示**字段（形状与文案口径不变）；分桶只认 [`ProbeKind`]。
pub fn health_item_from_probe(
    internal_key: &str,
    probe: ProbeResult,
) -> (HealthCheckItem, ProbeKind) {
    match probe {
        ProbeResult::Ok(detail) => {
            if let Some(kind) = dep_compat_kind_for_health_key(internal_key)
                && let Some(reason) = validate_dep_version(kind, &detail)
            {
                return (
                    HealthCheckItem {
                        ok: false,
                        detail: Some(format!("{detail}（{reason}）")),
                    },
                    ProbeKind::VersionIncompat,
                );
            }
            (
                HealthCheckItem {
                    ok: true,
                    detail: Some(detail),
                },
                ProbeKind::Ok,
            )
        }
        ProbeResult::Missing { detail } => (
            HealthCheckItem {
                ok: false,
                detail: Some(detail),
            },
            ProbeKind::Missing,
        ),
        ProbeResult::SpawnFailed { detail } => (
            HealthCheckItem {
                ok: false,
                detail: Some(detail),
            },
            ProbeKind::Broken,
        ),
        ProbeResult::Failed { code, stderr } => (
            HealthCheckItem {
                ok: false,
                detail: Some(failed_probe_detail(code, &stderr)),
            },
            ProbeKind::Broken,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DepCompatKind, MIN_RUST_VERSION, ProbeKind, ProbeResult, capture_probe,
        health_item_from_probe, parse_loose_semver, parse_rust_tool_version, validate_dep_version,
        version_at_least,
    };

    #[test]
    fn parse_rustc_version_from_v_output() {
        assert_eq!(
            parse_rust_tool_version("rustc 1.85.0 (abc)"),
            Some((1, 85, 0))
        );
    }

    #[test]
    fn rejects_rust_below_min() {
        let reason = validate_dep_version(DepCompatKind::Rustc, "rustc 1.84.0 (abc)").unwrap();
        assert!(reason.contains("1.85"));
        assert!(validate_dep_version(DepCompatKind::Rustc, "rustc 1.85.0 (abc)").is_none());
    }

    #[test]
    fn python_min_version() {
        assert!(validate_dep_version(DepCompatKind::Python3, "Python 3.7.0").is_some());
        assert!(validate_dep_version(DepCompatKind::Python3, "Python 3.8.1").is_none());
    }

    #[test]
    fn loose_semver_extracts_first_triple() {
        assert_eq!(parse_loose_semver("cmake version 3.28.1"), Some((3, 28, 1)));
    }

    #[test]
    fn version_at_least_cmp() {
        assert!(version_at_least((1, 85, 0), MIN_RUST_VERSION));
        assert!(!version_at_least((1, 84, 99), MIN_RUST_VERSION));
    }

    #[test]
    fn absent_command_probes_missing_regardless_of_message() {
        // 判定来自 `io::ErrorKind::NotFound`，与 stderr 文案无关。
        let probe = capture_probe("crabmate-no-such-cli-for-probe-test", &["--version"]);
        assert!(matches!(probe, ProbeResult::Missing { .. }));
        let (item, kind) = health_item_from_probe("gh", probe);
        assert_eq!(kind, ProbeKind::Missing);
        assert!(!item.ok);
    }

    #[test]
    fn non_zero_exit_probes_broken_not_missing() {
        let (item, kind) = health_item_from_probe(
            "gh",
            ProbeResult::Failed {
                code: Some(3),
                stderr: "boom: cannot run\n".to_string(),
            },
        );
        assert_eq!(kind, ProbeKind::Broken);
        assert!(!item.ok);
        assert_eq!(item.detail.as_deref(), Some("exit=3: boom: cannot run"));
    }

    #[test]
    fn signal_terminated_probe_keeps_legacy_exit_placeholder() {
        let (item, _) = health_item_from_probe(
            "npm",
            ProbeResult::Failed {
                code: None,
                stderr: String::new(),
            },
        );
        assert_eq!(item.detail.as_deref(), Some("exit=-1"));
    }

    #[test]
    fn version_incompat_is_classified_structurally() {
        let (item, kind) = health_item_from_probe(
            "rustc",
            ProbeResult::Ok("rustc 1.80.0 (abc)".to_string()),
        );
        assert_eq!(kind, ProbeKind::VersionIncompat);
        assert!(!item.ok);
        assert!(item.detail.unwrap_or_default().contains("低于建议最低"));
    }

    #[test]
    fn healthy_probe_is_ok_kind() {
        let (item, kind) = health_item_from_probe("npm", ProbeResult::Ok("10.9.0".to_string()));
        assert_eq!(kind, ProbeKind::Ok);
        assert!(item.ok);
        assert_eq!(item.detail.as_deref(), Some("10.9.0"));
    }
}
