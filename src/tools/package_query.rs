//! Linux 包查询工具：统一抽象 apt/rpm 的“是否安装 / 版本 / 来源”。
//!
//! 仅做只读查询，不执行安装或删除操作。

use std::io;
use std::process::Command;

const MAX_OUTPUT_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagerPref {
    Auto,
    Apt,
    Rpm,
}

#[derive(Debug)]
struct PackageQueryResult {
    manager: &'static str,
    installed: bool,
    version: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
enum QueryError {
    ManagerMissing,
    ExecFailed(String),
}

pub fn run(args_json: &str, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let package = match v.get("package").and_then(|x| x.as_str()) {
        Some(s) => match validate_package_name(s) {
            Ok(pkg) => pkg,
            Err(e) => return e,
        },
        None => return "错误：缺少 package 参数".to_string(),
    };
    let pref = match parse_manager_pref(v.get("manager").and_then(|x| x.as_str())) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let queried = match pref {
        ManagerPref::Auto => query_auto(&package),
        ManagerPref::Apt => query_apt(&package),
        ManagerPref::Rpm => query_rpm(&package),
    };

    match queried {
        Ok(r) => {
            let out = serde_json::json!({
                "package": package,
                "manager": r.manager,
                "installed": r.installed,
                "version": r.version,
                "source": r.source,
            });
            let rendered = serde_json::to_string_pretty(&out)
                .unwrap_or_else(|_| "{\"error\":\"serialize_failed\"}".to_string());
            truncate_output(&rendered, max_output_len)
        }
        Err(QueryError::ManagerMissing) => {
            "错误：未检测到可用的包管理查询命令（需要 dpkg-query 或 rpm）".to_string()
        }
        Err(QueryError::ExecFailed(msg)) => msg,
    }
}

fn query_auto(package: &str) -> Result<PackageQueryResult, QueryError> {
    match query_apt(package) {
        Ok(v) => Ok(v),
        Err(QueryError::ManagerMissing) => query_rpm(package),
        Err(e) => Err(e),
    }
}

fn query_apt(package: &str) -> Result<PackageQueryResult, QueryError> {
    let output = Command::new("dpkg-query")
        .arg("-W")
        .arg("-f=${Status}\t${Version}\t${Source}\n")
        .arg(package)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return if e.kind() == io::ErrorKind::NotFound {
                Err(QueryError::ManagerMissing)
            } else {
                Err(QueryError::ExecFailed(format!(
                    "dpkg-query 执行失败：{}",
                    e
                )))
            };
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        let line = stdout.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let (installed, version, source) = parse_dpkg_query_line(line);
        return Ok(PackageQueryResult {
            manager: "apt",
            installed,
            version,
            source,
        });
    }
    let joined_lower = format!("{} {}", stdout, stderr).to_ascii_lowercase();
    if joined_lower.contains("no packages found") || joined_lower.contains("is not installed") {
        return Ok(PackageQueryResult {
            manager: "apt",
            installed: false,
            version: None,
            source: None,
        });
    }
    Err(QueryError::ExecFailed(format!(
        "dpkg-query 查询失败（exit={}）：{}",
        output.status.code().unwrap_or(-1),
        concise_err(&stdout, &stderr)
    )))
}

fn query_rpm(package: &str) -> Result<PackageQueryResult, QueryError> {
    let output = Command::new("rpm")
        .arg("-q")
        .arg("--qf")
        .arg("%{VERSION}-%{RELEASE}\t%{SOURCERPM}\n")
        .arg(package)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return if e.kind() == io::ErrorKind::NotFound {
                Err(QueryError::ManagerMissing)
            } else {
                Err(QueryError::ExecFailed(format!("rpm 执行失败：{}", e)))
            };
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        let line = stdout.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let (version, source) = parse_rpm_query_line(line);
        return Ok(PackageQueryResult {
            manager: "rpm",
            installed: true,
            version,
            source,
        });
    }
    let joined_lower = format!("{} {}", stdout, stderr).to_ascii_lowercase();
    if joined_lower.contains("is not installed") || joined_lower.contains("not installed") {
        return Ok(PackageQueryResult {
            manager: "rpm",
            installed: false,
            version: None,
            source: None,
        });
    }
    Err(QueryError::ExecFailed(format!(
        "rpm 查询失败（exit={}）：{}",
        output.status.code().unwrap_or(-1),
        concise_err(&stdout, &stderr)
    )))
}

fn parse_manager_pref(raw: Option<&str>) -> Result<ManagerPref, String> {
    match raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .as_str()
    {
        "auto" => Ok(ManagerPref::Auto),
        "apt" => Ok(ManagerPref::Apt),
        "rpm" => Ok(ManagerPref::Rpm),
        _ => Err("错误：manager 仅支持 auto / apt / rpm".to_string()),
    }
}

fn validate_package_name(raw: &str) -> Result<String, String> {
    let pkg = raw.trim();
    if pkg.is_empty() {
        return Err("错误：package 不能为空".to_string());
    }
    if pkg.len() > 200 {
        return Err("错误：package 过长（最多 200 字符）".to_string());
    }
    if !pkg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_' | ':' | '@'))
    {
        return Err("错误：package 仅允许字母、数字及 . + - _ : @".to_string());
    }
    Ok(pkg.to_string())
}

fn parse_dpkg_query_line(line: &str) -> (bool, Option<String>, Option<String>) {
    let mut parts = line.splitn(3, '\t');
    let status = parts.next().unwrap_or("").trim();
    let version = parts.next().and_then(non_empty_trimmed);
    let source = parts.next().and_then(non_empty_trimmed);
    let installed = status == "install ok installed";
    if installed {
        (true, version, source)
    } else {
        (false, None, None)
    }
}

fn parse_rpm_query_line(line: &str) -> (Option<String>, Option<String>) {
    let mut parts = line.splitn(2, '\t');
    let version = parts.next().and_then(non_empty_trimmed);
    let source = parts.next().and_then(non_empty_trimmed);
    (version, source)
}

fn non_empty_trimmed(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn concise_err(stdout: &str, stderr: &str) -> String {
    let s = if !stderr.trim().is_empty() {
        stderr
    } else if !stdout.trim().is_empty() {
        stdout
    } else {
        "(无详细输出)"
    };
    let t = s.trim();
    if t.chars().count() > 180 {
        format!("{}…", t.chars().take(180).collect::<String>())
    } else {
        t.to_string()
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= MAX_OUTPUT_LINES && s.len() <= max_bytes {
        return s.to_string();
    }
    let kept_lines = lines.len().min(MAX_OUTPUT_LINES);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        truncate_to_char_boundary(&joined, max_bytes)
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_pref_defaults_to_auto() {
        assert!(matches!(parse_manager_pref(None), Ok(ManagerPref::Auto)));
        assert!(matches!(
            parse_manager_pref(Some(" ")),
            Ok(ManagerPref::Auto)
        ));
    }

    #[test]
    fn package_name_validation_rejects_invalid_chars() {
        assert!(validate_package_name("bash").is_ok());
        assert!(validate_package_name("libc6:amd64").is_ok());
        assert!(validate_package_name("curl@latest").is_ok());
        assert!(validate_package_name("bad/pkg").is_err());
        assert!(validate_package_name("../x").is_err());
    }

    #[test]
    fn parse_dpkg_line_installed() {
        let (installed, version, source) =
            parse_dpkg_query_line("install ok installed\t1.2.3-1\tmypkg");
        assert!(installed);
        assert_eq!(version.as_deref(), Some("1.2.3-1"));
        assert_eq!(source.as_deref(), Some("mypkg"));
    }

    #[test]
    fn parse_dpkg_line_not_installed() {
        let (installed, version, source) =
            parse_dpkg_query_line("deinstall ok config-files\t1.2.3-1\tmypkg");
        assert!(!installed);
        assert!(version.is_none());
        assert!(source.is_none());
    }

    #[test]
    fn parse_rpm_line_installed() {
        let (version, source) = parse_rpm_query_line("1.2.3-4.el9\tsource-1.2.3-4.el9.src.rpm");
        assert_eq!(version.as_deref(), Some("1.2.3-4.el9"));
        assert_eq!(source.as_deref(), Some("source-1.2.3-4.el9.src.rpm"));
    }
}
