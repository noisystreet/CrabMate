//! 运行时环境快照：首轮注入「我在什么环境里」的只读事实（#1526 A1a）。
//!
//! **为什么需要**：全仓原本只有 `diagnostic_summary` 输出 `std::env::consts::OS`，而它是**工具返回**
//! ——模型得先想到去调才知道。默认可见之后，它就不会写出 Windows 路径、假设机器上没有的工具链、
//! 或把「进程 cwd」与「会话工作区根」当成同一件事。
//!
//! **脱敏是本模块的硬约束**（快照会随请求发往第三方网关）：
//! - `cwd` 与工作区根：会话根内给相对路径，根外**只留末段**（`basename`）——`/home/<user>/…`
//!   等于把账户名带出去。
//! - 允许根只报**数量**，不列路径。
//! - 不含主机名、不含任何环境变量取值。
//!
//! 探测复用 [`crate::cm_tool_exec::capture_trimmed`]，按
//! `runtime_env_snapshot_probe_cache_ttl_secs` 做进程内缓存：`rustc -V` 在一个进程生命周期内
//! 基本不变，每次首轮都起子进程是白付的启动成本。

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::cm_config::AgentConfig;
use crate::cm_tool_exec::capture_trimmed;

/// 探测结果的进程内缓存：`(取样时刻, 已渲染的「工具链」一行)`。
///
/// 用 `Mutex` 而非 `OnceLock`：TTL 到期需要重探，`OnceLock` 只能写一次。
static PROBE_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// 渲染快照正文；关闭（`_enabled = false` 或预算 `0`）时返回 `None`。
///
/// 双闸与其它首轮注入项一致：`_enabled` 表达意图，`max_chars = 0` 是运行期硬关。
#[must_use]
pub fn build_runtime_env_snapshot_markdown(
    workspace_root: &Path,
    cfg: &AgentConfig,
) -> Option<String> {
    let c = &cfg.context_bootstrap_inject;
    if !c.runtime_env_snapshot_enabled || c.runtime_env_snapshot_max_chars == 0 {
        return None;
    }
    let mut out = render_body(
        workspace_root,
        cfg,
        c.runtime_env_snapshot_probe_cache_ttl_secs,
    );
    truncate_chars(&mut out, c.runtime_env_snapshot_max_chars);
    Some(out)
}

fn render_body(workspace_root: &Path, cfg: &AgentConfig, ttl_secs: u64) -> String {
    let root = Some(workspace_root).filter(|p| !p.as_os_str().is_empty());
    let mut lines = vec![
        "## 运行时环境快照（首轮注入；只读事实）".to_string(),
        String::new(),
        format!(
            "- 平台: {} / {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!(
            "- 会话工作区根: {}",
            root.map_or_else(|| "(未设置)".to_string(), workspace_root_label)
        ),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        lines.push(format!(
            "- 进程 cwd: {}",
            root.map_or_else(|| basename_or_unknown(&cwd), |r| display_in_root(&cwd, r))
        ));
    }
    lines.push(format!(
        "- 工作区允许根: {} 个",
        cfg.workspace_roots.workspace_allowed_roots.len()
    ));
    lines.push(format!("- 工具链: {}", probe_toolchain(ttl_secs)));
    lines.push(format!(
        "- 权限等级（进程默认）: {}",
        cfg.permission.permission_level.as_str()
    ));
    lines.push(String::new());
    lines.push(
        "路径已脱敏：会话根内用相对路径，根外只留末段；不含主机名与环境变量取值。".to_string(),
    );
    lines.join("\n")
}

/// 会话根的展示形态。
///
/// **不能**用 [`display_in_root`]：`display_in_root(root, root)` 恒为 `.`，而「我在哪个项目里」
/// 恰恰是这条快照最该回答的问题。改为给**末段**（项目目录名）——既让模型认得出工作区，
/// 又不把 `/home/<user>/...` 的上级目录带出去。
fn workspace_root_label(root: &Path) -> String {
    format!("./（{}）", basename_or_unknown(root))
}

/// `path` 相对 `root` 的展示形态；不在根内则退化为末段。
fn display_in_root(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        Ok(rest) => format!("./{}", rest.display()),
        Err(_) => basename_or_unknown(path),
    }
}

fn basename_or_unknown(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(未知)".to_string())
}

/// 工具链版本一行；`ttl_secs = 0` 表示禁用缓存（每次重探）。
fn probe_toolchain(ttl_secs: u64) -> String {
    if ttl_secs == 0 {
        return render_toolchain();
    }
    let ttl = Duration::from_secs(ttl_secs);
    if let Ok(guard) = PROBE_CACHE.lock()
        && let Some((at, body)) = guard.as_ref()
        && at.elapsed() < ttl
    {
        return body.clone();
    }
    let body = render_toolchain();
    if let Ok(mut guard) = PROBE_CACHE.lock() {
        *guard = Some((Instant::now(), body.clone()));
    }
    body
}

fn render_toolchain() -> String {
    let rustc = capture_trimmed("rustc", &["-V"]).unwrap_or_else(|| "rustc 不可用".to_string());
    let cargo = capture_trimmed("cargo", &["-V"]).unwrap_or_else(|| "cargo 不可用".to_string());
    format!("{rustc} · {cargo}")
}

/// 软上限：截断提示本身不计入 `max_chars`（与 `living_docs` / 依赖摘要同一约定），
/// 因此被截断的正文会比预算多出约 40 字符。
fn truncate_chars(out: &mut String, max_chars: usize) {
    if out.chars().count() <= max_chars {
        return;
    }
    let keep = max_chars.saturating_sub(24);
    let head: String = out.chars().take(keep).collect();
    *out = format!("{head}\n\n[... 已按 runtime_env_snapshot_max_chars 截断 ...]");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cfg_with(enabled: bool, max_chars: usize) -> AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default config");
        cfg.context_bootstrap_inject.runtime_env_snapshot_enabled = enabled;
        cfg.context_bootstrap_inject.runtime_env_snapshot_max_chars = max_chars;
        cfg
    }

    /// 双闸：`_enabled = false` 或预算 `0` 都不产出（与其它注入项同一套语义）。
    #[test]
    fn disabled_or_zero_budget_yields_none() {
        let root = Path::new(".");
        assert!(
            build_runtime_env_snapshot_markdown(root, &cfg_with(false, 2000)).is_none(),
            "enabled=false 时不得注入"
        );
        assert!(
            build_runtime_env_snapshot_markdown(root, &cfg_with(true, 0)).is_none(),
            "max_chars=0 时不得注入"
        );
        assert!(
            build_runtime_env_snapshot_markdown(root, &cfg_with(true, 2000)).is_some(),
            "默认配置应产出正文"
        );
    }

    /// 脱敏口径：会话根内的路径给相对形态，根外**只留末段**（账户名不得外发）。
    ///
    /// 这是本模块唯一不可逆的约束（快照发往第三方网关），故单独钉住。
    #[test]
    fn paths_outside_root_lose_their_parents() {
        let root = PathBuf::from("/srv/workspace");
        assert_eq!(display_in_root(&root, &root), ".");
        assert_eq!(
            display_in_root(&root.join("src/main.rs"), &root),
            "./src/main.rs"
        );
        // 根外：只保留末段，`/home/alice/...` 里的 `alice` 不得出现。
        let outside = display_in_root(Path::new("/home/alice/secret"), &root);
        assert_eq!(outside, "secret");
        assert!(
            !outside.contains("alice"),
            "根外路径泄露了上级目录: {outside}"
        );
    }

    /// 会话根必须给出**末段**（项目名），而不是恒定的 `.`。
    ///
    /// 回归守卫：初版误用 `display_in_root(root, root)`，那一行恒为 `.`——等于没回答
    /// 「我在哪个项目里」，而这是整条快照最该回答的问题。
    #[test]
    fn workspace_root_label_keeps_the_project_name() {
        let label = workspace_root_label(Path::new("/srv/acme/crabmate"));
        assert_eq!(label, "./（crabmate）");
        assert!(!label.contains("acme"), "不得带出上级目录: {label}");
    }

    /// 正文包含关键事实且**不含**绝对路径的上级片段。
    #[test]
    fn body_reports_platform_and_toolchain() {
        let cfg = cfg_with(true, 4000);
        let root = std::env::current_dir().expect("cwd");
        let body = build_runtime_env_snapshot_markdown(&root, &cfg).expect("应产出正文");
        assert!(body.contains("运行时环境快照"), "{body}");
        assert!(body.contains(std::env::consts::OS), "应报告平台: {body}");
        assert!(body.contains("工具链:"), "{body}");
        assert!(body.contains("工作区允许根:"), "{body}");
    }

    /// 预算极小时截断（`max_chars` 是**软**上限：截断提示本身不计入预算）。
    #[test]
    fn tiny_budget_is_truncated() {
        let cfg = cfg_with(true, 64);
        let root = std::env::current_dir().expect("cwd");
        let body = build_runtime_env_snapshot_markdown(&root, &cfg).expect("应产出正文");
        assert!(
            body.contains("已按 runtime_env_snapshot_max_chars 截断"),
            "应出现截断标记（而不是静默丢弃）: {body}"
        );
        // 上界只为挡住「无界增长」；`+64` 容纳截断提示本身（见 `truncate_chars` 的软上限说明）。
        assert!(body.chars().count() <= 64 + 64, "截断后仍无界增长: {body}");
    }
}
