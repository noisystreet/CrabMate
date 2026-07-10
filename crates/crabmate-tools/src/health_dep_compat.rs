//! 可选依赖与工具链的**版本兼容性**判定（供 [`crate::health`] 与启动日志共用）。
//!
//! 命令存在且退出码为 0 时仍可能版本过低；本模块在能解析出版本号时与最低要求比较。

/// CrabMate 构建所需最低 Rust（与根 `Cargo.toml` edition 2024 / README 一致）。
pub const MIN_RUST_VERSION: (u64, u64, u64) = (1, 85, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepCompatKind {
    Rustc,
    Cargo,
    Python3,
    Npm,
    Gh,
    Bc,
    ClangFormat,
}

/// 在版本输出可解析时校验；不可解析则视为通过（仍保留原始 `detail` 供人读）。
pub fn validate_dep_version(kind: DepCompatKind, version_output: &str) -> Option<String> {
    let parsed = match kind {
        DepCompatKind::Rustc | DepCompatKind::Cargo => parse_rust_tool_version(version_output),
        DepCompatKind::Python3 => parse_python_version(version_output),
        DepCompatKind::Npm | DepCompatKind::Gh | DepCompatKind::Bc | DepCompatKind::ClangFormat => {
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
        DepCompatKind::Gh => (2, 0, 0),
        DepCompatKind::Bc | DepCompatKind::ClangFormat => (0, 0, 0),
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
        "bc" => Some(DepCompatKind::Bc),
        "clang_format" => Some(DepCompatKind::ClangFormat),
        _ => None,
    }
}

/// 单项健康检查结果（与 [`crabmate-internal::health::HealthCheckItem`] 字段一致）。
#[derive(Debug, Clone)]
pub struct HealthCheckItem {
    pub ok: bool,
    pub detail: Option<String>,
}

/// 将 `check_cmd` 成功输出与兼容性结论合并为 [`HealthCheckItem`]。
pub fn health_item_from_cmd_result(
    internal_key: &str,
    cmd_result: Result<String, String>,
) -> HealthCheckItem {
    match cmd_result {
        Ok(detail) => {
            if let Some(kind) = dep_compat_kind_for_health_key(internal_key)
                && let Some(reason) = validate_dep_version(kind, &detail)
            {
                return HealthCheckItem {
                    ok: false,
                    detail: Some(format!("{detail}（{reason}）")),
                };
            }
            HealthCheckItem {
                ok: true,
                detail: Some(detail),
            }
        }
        Err(err) => HealthCheckItem {
            ok: false,
            detail: Some(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DepCompatKind, MIN_RUST_VERSION, parse_loose_semver, parse_rust_tool_version,
        validate_dep_version, version_at_least,
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
}
