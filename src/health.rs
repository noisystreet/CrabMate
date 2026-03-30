//! 运行状况检查：与 **`GET /health`** JSON 形状一致，供 Axum handler 与 TUI（F10）共用。
//!
//! 不发起网络请求；仅检查本进程可见的 API Key 是否非空、工作区可写、前端静态目录（可选）与若干本地 CLI。

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::config::LlmHttpAuthMode;

/// 单项检查结果（与 HTTP JSON 字段一致）。
#[derive(Debug, Clone, Serialize)]
pub struct HealthCheckItem {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 与 `GET /health` 响应体一致（`status` + `checks`）。
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub status: String,
    pub checks: BTreeMap<String, HealthCheckItem>,
}

/// 构建健康报告（阻塞工作放在 `spawn_blocking` 内）。
///
/// `include_frontend_static`：为 true 时检查 `frontend/dist`（与 Web 服务模式一致）；TUI 也可为 true 以便本地开发时看到前端构建是否缺失。
pub async fn build_health_report(
    workspace_dir: &Path,
    api_key: &str,
    llm_http_auth_mode: LlmHttpAuthMode,
    include_frontend_static: bool,
) -> HealthReport {
    let mut checks: BTreeMap<String, HealthCheckItem> = BTreeMap::new();

    let api_key_ok = llm_http_auth_mode == LlmHttpAuthMode::None || !api_key.trim().is_empty();
    checks.insert(
        "api_key".to_string(),
        HealthCheckItem {
            ok: api_key_ok,
            detail: if api_key_ok {
                None
            } else if llm_http_auth_mode == LlmHttpAuthMode::Bearer {
                Some("未设置 API_KEY（llm_http_auth_mode=bearer）".to_string())
            } else {
                Some("未设置 API_KEY".to_string())
            },
        },
    );

    if include_frontend_static {
        let static_dir = crate::web_static_dir::resolve_web_static_dir();
        let static_ok = static_dir.is_dir();
        checks.insert(
            "frontend_static_dir".to_string(),
            HealthCheckItem {
                ok: static_ok,
                detail: if static_ok {
                    None
                } else {
                    Some(format!("目录不存在：{}", static_dir.display()))
                },
            },
        );
    }

    let work_dir = workspace_dir.to_path_buf();
    let writable = tokio::task::spawn_blocking({
        let work_dir = work_dir.clone();
        move || {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let pid = std::process::id();
            let p = work_dir.join(format!(".crabmate_healthcheck_{}_{}.tmp", pid, ts));
            match std::fs::write(&p, b"") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&p);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
    })
    .await
    .ok()
    .and_then(|r| r.err())
    .map(|e| format!("不可写：{}（{}）", work_dir.display(), e));

    checks.insert(
        "workspace_writable".to_string(),
        HealthCheckItem {
            ok: writable.is_none(),
            detail: writable,
        },
    );

    let deps = tokio::task::spawn_blocking(|| {
        fn check_cmd(cmd: &str, args: &[&str]) -> Result<String, String> {
            match std::process::Command::new(cmd).args(args).output() {
                Ok(out) => {
                    let status = out.status.code().unwrap_or(-1);
                    if status == 0 {
                        let s = if !out.stdout.is_empty() {
                            String::from_utf8_lossy(&out.stdout).trim().to_string()
                        } else {
                            String::from_utf8_lossy(&out.stderr).trim().to_string()
                        };
                        Ok(if s.is_empty() { "ok".to_string() } else { s })
                    } else {
                        Err(format!("exit={}", status))
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }

        let mut m = BTreeMap::new();

        let bc = check_cmd("bc", &["--version"])
            .or_else(|_| check_cmd("bc", &["-v"]))
            .or_else(|_| check_cmd("bc", &["-V"]));
        m.insert("bc", bc);

        m.insert("rustfmt", check_cmd("rustfmt", &["--version"]));
        m.insert("clang_format", check_cmd("clang-format", &["--version"]));
        m.insert("cmake", check_cmd("cmake", &["--version"]));
        m.insert("ctest", check_cmd("ctest", &["--version"]));
        m.insert("cxxfilt", check_cmd("c++filt", &["--version"]));
        // GNU Binutils（或兼容实现）：供 run_command 白名单内 ELF/目标文件只读分析
        m.insert("objdump", check_cmd("objdump", &["--version"]));
        m.insert("nm", check_cmd("nm", &["--version"]));
        m.insert("readelf", check_cmd("readelf", &["--version"]));
        m.insert("strings_binutils", check_cmd("strings", &["--version"]));
        m.insert("size", check_cmd("size", &["--version"]));
        m.insert("ar", check_cmd("ar", &["--version"]));
        m.insert("npm", check_cmd("npm", &["--version"]));
        m.insert("python3", check_cmd("python3", &["--version"]));
        m.insert("mvn", check_cmd("mvn", &["--version"]));
        m.insert("gradle", check_cmd("gradle", &["--version"]));
        m.insert("docker", check_cmd("docker", &["--version"]));
        m.insert("podman", check_cmd("podman", &["--version"]));

        m.insert("typos", check_cmd("typos", &["--version"]));
        m.insert("codespell", check_cmd("codespell", &["--version"]));
        m.insert("ast_grep", check_cmd("ast-grep", &["--version"]));

        m.insert(
            "cargo_machete",
            check_cmd("cargo", &["machete", "--version"]),
        );
        m.insert("cargo_udeps", check_cmd("cargo", &["udeps", "--version"]));

        m.insert("shellcheck", check_cmd("shellcheck", &["--version"]));
        m.insert("cppcheck", check_cmd("cppcheck", &["--version"]));
        m.insert("semgrep", check_cmd("semgrep", &["--version"]));
        m.insert("hadolint", check_cmd("hadolint", &["--version"]));
        m.insert("bandit", check_cmd("bandit", &["--version"]));
        m.insert("lizard", check_cmd("lizard", &["--version"]));

        m
    })
    .await
    .ok()
    .unwrap_or_default();

    for (k, v) in deps {
        let key = match k {
            "bc" => "dep_bc",
            "rustfmt" => "dep_rustfmt",
            "clang_format" => "dep_clang_format",
            "cmake" => "dep_cmake",
            "ctest" => "dep_ctest",
            "cxxfilt" => "dep_cxxfilt",
            "objdump" => "dep_objdump",
            "nm" => "dep_nm",
            "readelf" => "dep_readelf",
            "strings_binutils" => "dep_strings_binutils",
            "size" => "dep_size",
            "ar" => "dep_ar",
            "npm" => "dep_npm",
            "python3" => "dep_python3",
            "mvn" => "dep_mvn",
            "gradle" => "dep_gradle",
            "docker" => "dep_docker_cli",
            "podman" => "dep_podman",
            "typos" => "dep_typos",
            "codespell" => "dep_codespell",
            "ast_grep" => "dep_ast_grep",
            "cargo_machete" => "dep_cargo_machete",
            "cargo_udeps" => "dep_cargo_udeps",
            "shellcheck" => "dep_shellcheck",
            "cppcheck" => "dep_cppcheck",
            "semgrep" => "dep_semgrep",
            "hadolint" => "dep_hadolint",
            "bandit" => "dep_bandit",
            "lizard" => "dep_lizard",
            _ => continue,
        };
        match v {
            Ok(detail) => {
                checks.insert(
                    key.to_string(),
                    HealthCheckItem {
                        ok: true,
                        detail: Some(detail),
                    },
                );
            }
            Err(err) => {
                checks.insert(
                    key.to_string(),
                    HealthCheckItem {
                        ok: false,
                        detail: Some(err),
                    },
                );
            }
        }
    }

    let required_ok = checks.get("api_key").map(|c| c.ok).unwrap_or(false)
        && checks
            .get("workspace_writable")
            .map(|c| c.ok)
            .unwrap_or(false);
    let status = if required_ok && checks.values().all(|c| c.ok) {
        "ok"
    } else {
        "degraded"
    };

    HealthReport {
        status: status.to_string(),
        checks,
    }
}

/// 终端多行展示用（多行纯文本）；当前无调用方，保留供后续 CLI/TUI 复用。
#[allow(dead_code)]
pub fn format_health_report_terminal(report: &HealthReport) -> String {
    let mut s = String::new();
    s.push_str("与 GET /health 一致的本地检查\n");
    s.push_str("status: ");
    s.push_str(&report.status);
    s.push_str("\n\n");
    for (k, v) in &report.checks {
        let mark = if v.ok { "ok" } else { "!!" };
        s.push_str(&format!("[{}] {}\n", mark, k));
        if let Some(ref d) = v.detail {
            for line in d.lines() {
                s.push_str("    ");
                s.push_str(line);
                s.push('\n');
            }
        }
    }
    s.push_str("\n按 Esc 或 F10 关闭");
    s
}
