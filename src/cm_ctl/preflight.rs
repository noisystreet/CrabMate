//! `up` / `run` 的 workspace 预检：workspace 是否落在**子实例** config 的
//! `workspace_allowed_roots` 内。
//!
//! 三条边界决定实现形态：
//!
//! 1. **只做友好预检，不做授权判断**：预检通过不等于 serve 会接受该 workspace，
//!    最终仍以 serve 启动结果为准；任何情况下**不自动把 workspace 加进白名单**，
//!    否则 ctl 就成了绕过 `workspace_allowed_roots` 的后门。
//! 2. **只在显式给出 `--serve-config` 时判定**：未给出时子实例会按它自己的 cwd
//!    （实例目录）走 serve 的配置解析顺序，而 ctl 侧的
//!    `resolve_interactive_cli_config_path` 依赖 **ctl 的** cwd 且会写盘 seed
//!    用户配置 —— 拿它推断子实例会加载哪个文件只会得到错误答案，故此时只提示、不阻断。
//! 3. **比较前必须 canonicalize**：白名单项在 config finalize 时已 canonicalize，
//!    而 `workspace` 只是 absolutize（保留符号链接），直接比会把软链接工作区误判越界。

use std::path::{Path, PathBuf};

use super::CtlError;
use super::paths;

/// 预检结果：`warnings` 是可以继续、但值得提示的情形。
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub warnings: Vec<String>,
}

/// 预检入口：`serve_config` 是交给子 serve 的 `--config` 原始值（不是 ctl 自己的）。
pub fn check(serve_config: Option<&str>, workspace: &Path) -> Result<Outcome, CtlError> {
    let mut warnings = Vec::new();
    let Some(raw) = serve_config.map(str::trim).filter(|s| !s.is_empty()) else {
        warnings.push(
            "未指定 --serve-config：workspace 是否在 workspace_allowed_roots 内由 serve 自行判断"
                .to_string(),
        );
        return Ok(Outcome { warnings });
    };

    let cfg_path = paths::absolutize(raw)?;
    let roots = load_roots(&cfg_path)?;
    if roots.is_empty() {
        warnings.push(format!(
            "子实例配置 {} 未配置 workspace_allowed_roots：允许任意 workspace",
            cfg_path.display()
        ));
        return Ok(Outcome { warnings });
    }
    check_with_roots(workspace, &roots)?;
    Ok(Outcome { warnings })
}

fn load_roots(cfg_path: &Path) -> Result<Vec<PathBuf>, CtlError> {
    let path_str = cfg_path.to_string_lossy().into_owned();
    let cfg = crate::config::load_config_for_cli(Some(&path_str)).map_err(|e| {
        CtlError::general(
            "bad_serve_config",
            format!("无法加载子实例配置 {}：{e}", cfg_path.display()),
        )
    })?;
    Ok(cfg.workspace_roots.workspace_allowed_roots)
}

/// 越界即拒绝（`workspace_out_of_roots`），并在消息里说清 ctl 不会加白。
fn check_with_roots(workspace: &Path, roots: &[PathBuf]) -> Result<(), CtlError> {
    if within(workspace, roots) {
        return Ok(());
    }
    let list = roots
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(CtlError::general(
        "workspace_out_of_roots",
        format!(
            "workspace {} 不在子实例配置的 workspace_allowed_roots 内（{list}）；ctl 不会自动加白，请改配置或换 workspace",
            workspace.display()
        ),
    ))
}

fn within(workspace: &Path, roots: &[PathBuf]) -> bool {
    let candidate = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    crate::config::is_within_allowed_roots(&candidate, roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_serve_config_only_warns() {
        let ws = tempfile::tempdir().expect("tempdir");
        let out = check(None, ws.path()).expect("未给 serve-config 不应失败");
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("--serve-config"), "{:?}", out.warnings);
    }

    #[test]
    fn workspace_outside_roots_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let other = tempfile::tempdir().expect("other");
        let roots = vec![root.path().canonicalize().expect("canon")];
        let candidate = other.path().canonicalize().expect("canon");
        let err = check_with_roots(&candidate, &roots).expect_err("越界必须拒绝");
        assert_eq!(err.error_code, "workspace_out_of_roots");
        assert!(err.message.contains("不会自动加白"), "{}", err.message);
    }

    #[test]
    fn workspace_inside_roots_passes() {
        let root = tempfile::tempdir().expect("root");
        let ws = root.path().join("proj");
        std::fs::create_dir_all(&ws).expect("mkdir");
        let roots = vec![root.path().canonicalize().expect("canon")];
        check_with_roots(&ws, &roots).expect("白名单内应通过");
    }

    #[test]
    fn missing_serve_config_file_is_reported() {
        let ws = tempfile::tempdir().expect("tempdir");
        let err = check(Some("no-such-config-xyz.toml"), ws.path()).expect_err("不存在应报错");
        assert_eq!(err.error_code, "bad_serve_config");
    }

    #[test]
    fn symlinked_workspace_is_canonicalized() {
        // 软链接工作区不能被误判越界：roots 已 canonicalize，workspace 只有 absolutize
        let root = tempfile::tempdir().expect("root");
        let real = root.path().join("proj");
        std::fs::create_dir_all(&real).expect("mkdir");
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let roots = vec![root.path().canonicalize().expect("canon")];
        check_with_roots(&link, &roots).expect("软链接应 canonicalize 后通过");
    }
}
