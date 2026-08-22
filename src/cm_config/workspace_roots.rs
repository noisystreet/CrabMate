use std::path::{Path, PathBuf};

/// 与 `crate::cm_tools::workspace::path::is_sensitive_workspace_path` 保持一致（config 不能依赖 tools）。
const SENSITIVE_WORKSPACE_PREFIXES: &[&str] = &[
    "/proc", "/sys", "/dev", "/etc", "/boot", "/root", "/bin", "/sbin", "/usr",
];

fn is_sensitive_workspace_path(path: &Path) -> bool {
    SENSITIVE_WORKSPACE_PREFIXES.iter().any(|prefix| {
        let p = Path::new(prefix);
        path == p || path.starts_with(p)
    })
}

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

fn join_pool_path(raw: &str) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前工作目录: {}", e))?;
    let p = Path::new(raw);
    Ok(if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    })
}

fn canonicalize_web_workspace_pool_dir(joined: PathBuf, raw: &str) -> Result<PathBuf, String> {
    if is_sensitive_workspace_path(&joined) {
        return Err(format!(
            "web_workspace_pool {} 命中敏感系统路径前缀，已拒绝",
            joined.display()
        ));
    }
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
    if is_sensitive_workspace_path(&canon) {
        return Err(format!(
            "web_workspace_pool {} 命中敏感系统路径前缀，已拒绝",
            canon.display()
        ));
    }
    Ok(canon)
}

/// 解析 Web 项目池根目录；若路径不存在则创建（`mkdir -p`）。
/// 配置了池根时**必须**同时配置非空 `workspace_allowed_roots`，且池根须落在白名单内、不得命中敏感前缀。
pub(super) fn resolve_web_workspace_pool(
    pool_opt: Option<String>,
    allowed_roots: &[PathBuf],
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = pool_opt.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let raw = raw.trim();
    if allowed_roots.is_empty() {
        return Err(
            "配置 web_workspace_pool 时必须同时设置非空的 workspace_allowed_roots（或 CM_WORKSPACE_ALLOWED_ROOTS）"
                .to_string(),
        );
    }
    let canon = canonicalize_web_workspace_pool_dir(join_pool_path(raw)?, raw)?;
    if !is_within_allowed_roots(&canon, allowed_roots) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn pool_requires_allowed_roots() {
        let err = resolve_web_workspace_pool(Some("/tmp/unused".into()), &[])
            .expect_err("must require roots");
        assert!(err.contains("workspace_allowed_roots"), "{err}");
    }

    #[test]
    fn pool_rejects_sensitive_prefix() {
        let root = tempfile::tempdir().expect("tempdir");
        let allowed = vec![root.path().to_path_buf()];
        let err = resolve_web_workspace_pool(Some("/etc".into()), &allowed).expect_err("sensitive");
        assert!(err.contains("敏感"), "{err}");
    }

    #[test]
    fn pool_creates_and_accepts_under_allowed() {
        let root = tempfile::tempdir().expect("tempdir");
        let pool = root.path().join("pool");
        let allowed = vec![root.path().canonicalize().expect("canon root")];
        let got = resolve_web_workspace_pool(Some(pool.display().to_string()), &allowed)
            .expect("ok")
            .expect("some");
        assert!(got.is_dir());
        assert!(got.starts_with(&allowed[0]));
        assert!(fs::metadata(&got).expect("meta").is_dir());
    }
}
