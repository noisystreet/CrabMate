use std::path::{Path, PathBuf};

/// 解析 Web 工作区白名单：未配置或空列表时允许任意路径（返回空列表）；
/// 否则每项须为已存在目录的绝对或相对路径（相对路径相对**进程当前目录**）。
pub(super) fn resolve_workspace_allowed_roots(
    roots_opt: Option<Vec<String>>,
    _run_root: &Path,
) -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let Some(roots_in) = roots_opt.filter(|v| !v.is_empty()) else {
        // 未配置时返回空列表，表示允许任意路径
        return Ok(vec![]);
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for s in roots_in {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let p = Path::new(s);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        let canon = joined
            .canonicalize()
            .map_err(|e| format!("workspace_allowed_roots 项 {:?} 无法解析为目录: {}", s, e))?;
        if !canon.is_dir() {
            return Err(format!(
                "workspace_allowed_roots 项 {} 不是目录",
                canon.display()
            ));
        }
        out.push(canon);
    }
    if out.is_empty() {
        return Err(
            "workspace_allowed_roots 配置为空：请省略该项（允许任意路径）或至少填写一个有效路径"
                .to_string(),
        );
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// 解析 Web 项目池根目录；若路径不存在则创建（`mkdir -p`）。须落在 `workspace_allowed_roots` 内（未配置白名单时不校验）。
pub(super) fn resolve_web_workspace_pool(
    pool_opt: Option<String>,
    allowed_roots: &[PathBuf],
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = pool_opt.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let raw = raw.trim();
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    if !joined.exists() {
        std::fs::create_dir_all(&joined)
            .map_err(|e| format!("web_workspace_pool {:?} 无法创建: {}", raw, e))?;
    }
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("web_workspace_pool {:?} 无法解析为目录: {}", raw, e))?;
    if !canon.is_dir() {
        return Err(format!("web_workspace_pool {} 不是目录", canon.display()));
    }
    if !allowed_roots.is_empty() && !is_within_allowed_roots(&canon, allowed_roots) {
        let roots_display = allowed_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "web_workspace_pool {} 不在 workspace_allowed_roots 允许范围内（{roots_display}）",
            canon.display()
        ));
    }
    Ok(Some(canon))
}

/// 供配置 finalize 复用：候选路径是否在允许根列表内（空列表表示不限制）。
pub fn is_within_allowed_roots(candidate: &Path, allowed_roots: &[PathBuf]) -> bool {
    if allowed_roots.is_empty() {
        return true;
    }
    allowed_roots
        .iter()
        .any(|root| candidate == root.as_path() || candidate.starts_with(root))
}
