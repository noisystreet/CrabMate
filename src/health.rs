//! 运行状况检查：与 **`GET /health`** JSON 形状一致，供 Axum handler 与 TUI（F10）共用。
//!
//! 默认**不**请求上游 LLM；可选（配置 [`crate::config::AgentConfig::health_llm_models_probe`]）对当前 `api_base` 发起 **GET …/models**（与 `crabmate probe` 同源，无 chat/completions 计费），并带进程内缓存以降低探活频率。

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use reqwest::Client;

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

/// 进程内缓存的上一次 **GET …/models** 健康探测结果（供 [`append_llm_models_endpoint_probe`] 复用）。
#[derive(Clone)]
pub struct CachedLlmModelsHealthProbe {
    pub checked_at: Instant,
    pub item: HealthCheckItem,
}

/// [`append_llm_models_endpoint_probe`] 的输入（减少参数个数，满足 clippy）。
pub struct LlmModelsEndpointProbeParams<'a> {
    pub enabled: bool,
    pub cache_secs: u64,
    pub cache_cell: &'a Mutex<Option<CachedLlmModelsHealthProbe>>,
    pub client: &'a Client,
    pub api_base: &'a str,
    pub api_key: &'a str,
    pub auth_mode: LlmHttpAuthMode,
}

fn health_report_status(checks: &BTreeMap<String, HealthCheckItem>) -> String {
    let required_ok = checks.get("api_key").map(|c| c.ok).unwrap_or(false)
        && checks
            .get("workspace_writable")
            .map(|c| c.ok)
            .unwrap_or(false);
    if required_ok && checks.values().all(|c| c.ok) {
        "ok".to_string()
    } else {
        "degraded".to_string()
    }
}

/// 在 [`build_health_report`] 之后可选追加 **`llm_models_endpoint`** 检查项并重算 **`status`**。
///
/// 使用与 **`crabmate models` / `crabmate probe`** 相同的 [`crate::llm::fetch_models_report`]；**`bearer` 且无 `API_KEY`** 时跳过探测（检查项 `ok: true`，说明中标注跳过）。
pub async fn append_llm_models_endpoint_probe(
    report: &mut HealthReport,
    p: LlmModelsEndpointProbeParams<'_>,
) {
    if !p.enabled {
        return;
    }

    let item = if p.auth_mode == LlmHttpAuthMode::Bearer && p.api_key.trim().is_empty() {
        HealthCheckItem {
            ok: true,
            detail: Some("跳过（bearer 且无 API_KEY）".to_string()),
        }
    } else {
        let cache_ttl = std::time::Duration::from_secs(p.cache_secs.max(1));
        let from_cache = p.cache_cell.lock().ok().and_then(|guard| {
            guard.as_ref().and_then(|c| {
                if c.checked_at.elapsed() < cache_ttl {
                    Some(c.item.clone())
                } else {
                    None
                }
            })
        });

        if let Some(cached) = from_cache {
            cached
        } else {
            let fresh =
                probe_llm_models_endpoint(p.client, p.api_base, p.api_key, p.auth_mode).await;
            if let Ok(mut guard) = p.cache_cell.lock() {
                *guard = Some(CachedLlmModelsHealthProbe {
                    checked_at: Instant::now(),
                    item: fresh.clone(),
                });
            }
            fresh
        }
    };

    report
        .checks
        .insert("llm_models_endpoint".to_string(), item);
    report.status = health_report_status(&report.checks);
}

async fn probe_llm_models_endpoint(
    client: &Client,
    api_base: &str,
    api_key: &str,
    auth_mode: LlmHttpAuthMode,
) -> HealthCheckItem {
    match crate::llm::fetch_models_report(client, api_base, api_key, auth_mode).await {
        Ok(rep) => {
            let ok = (200..300).contains(&rep.http_status)
                && rep.note.is_none()
                && !rep.model_ids.is_empty();
            let detail = if ok {
                Some(format!(
                    "HTTP {} · {}ms · {} 个模型 id（仅列表，无 completion）",
                    rep.http_status,
                    rep.elapsed_ms,
                    rep.model_ids.len()
                ))
            } else {
                Some(
                    rep.note
                        .unwrap_or_else(|| format!("HTTP {}（无可用模型 id）", rep.http_status)),
                )
            };
            HealthCheckItem { ok, detail }
        }
        Err(e) => HealthCheckItem {
            ok: false,
            detail: Some(format!("请求失败: {e}")),
        },
    }
}

/// 构建健康报告（阻塞工作放在 `spawn_blocking` 内）。
///
/// `include_frontend_static`：为 true 时检查 `frontend-leptos/dist`（与 Web 服务模式一致）；TUI 也可为 true 以便本地开发时看到前端构建是否缺失。
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
        // GitHub CLI：默认 run_command 白名单含 `gh`；未安装时工具调用会失败
        m.insert("gh", check_cmd("gh", &["version"]));

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
            "gh" => "dep_gh",
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

    HealthReport {
        status: health_report_status(&checks),
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

#[cfg(test)]
mod health_status_tests {
    use super::{HealthCheckItem, health_report_status};
    use std::collections::BTreeMap;

    #[test]
    fn status_ok_when_required_and_all_checks_ok() {
        let mut checks = BTreeMap::new();
        checks.insert(
            "api_key".into(),
            HealthCheckItem {
                ok: true,
                detail: None,
            },
        );
        checks.insert(
            "workspace_writable".into(),
            HealthCheckItem {
                ok: true,
                detail: None,
            },
        );
        checks.insert(
            "dep_bc".into(),
            HealthCheckItem {
                ok: true,
                detail: Some("ok".into()),
            },
        );
        assert_eq!(health_report_status(&checks), "ok");
    }

    #[test]
    fn status_degraded_when_optional_dep_fails() {
        let mut checks = BTreeMap::new();
        checks.insert(
            "api_key".into(),
            HealthCheckItem {
                ok: true,
                detail: None,
            },
        );
        checks.insert(
            "workspace_writable".into(),
            HealthCheckItem {
                ok: true,
                detail: None,
            },
        );
        checks.insert(
            "dep_bc".into(),
            HealthCheckItem {
                ok: false,
                detail: Some("missing".into()),
            },
        );
        assert_eq!(health_report_status(&checks), "degraded");
    }
}
