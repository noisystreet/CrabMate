use std::path::Path;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::AppState;
use crate::config::AgentConfig;
use crate::path_workspace::absolutize_workspace_subpath;

const WORKSPACE_FILE_READ_MAX_BYTES: u64 = 1_048_576;

#[derive(Serialize)]
pub struct WorkspacePickResponse {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Deserialize)]
pub struct WorkspaceQuery {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceResponse {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceSetBody {
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceSearchBody {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_insensitive: Option<bool>,
    #[serde(default)]
    pub ignore_hidden: Option<bool>,
}

#[derive(Serialize)]
pub struct WorkspaceSearchResponse {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceFileQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub struct WorkspaceFileWriteBody {
    pub path: String,
    pub content: String,
    /// 仅创建：若文件已存在则报错
    #[serde(default)]
    pub create_only: bool,
    /// 仅修改：若文件不存在则报错
    #[serde(default)]
    pub update_only: bool,
}

#[derive(Serialize)]
pub struct WorkspaceFileWriteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceFileDeleteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 校验 Web `POST /workspace` 非空 `path`：须为已存在目录，`canonicalize` 后落在 `workspace_allowed_roots` 某一根之下（见配置项 `workspace_allowed_roots` / `AGENT_WORKSPACE_ALLOWED_ROOTS`）。
pub(crate) fn validate_workspace_set_path(
    cfg: &AgentConfig,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let cwd = std::env::current_dir().map_err(|e| format!("无法获取当前目录: {}", e))?;
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("工作区路径无效或不存在: {}", e))?;
    if !canon.is_dir() {
        return Err("工作区路径必须是已存在的目录".to_string());
    }
    if !cfg
        .workspace_allowed_roots
        .iter()
        .any(|root| canon.starts_with(root))
    {
        let roots = cfg
            .workspace_allowed_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "工作区路径不在允许范围内（须位于以下根目录之一下: {}）",
            roots
        ));
    }
    Ok(canon)
}

fn ensure_within_workspace(
    base_canonical: &Path,
    candidate: std::path::PathBuf,
) -> Result<std::path::PathBuf, String> {
    if candidate.starts_with(base_canonical) {
        Ok(candidate)
    } else {
        Err("路径不能超出工作区根目录".to_string())
    }
}

fn resolve_workspace_write_path(base: &Path, sub: &str) -> Result<std::path::PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))?;
    let normalized = absolutize_workspace_subpath(&base_canonical, sub)?;

    // 防止借助工作区内 symlink 写到外部：校验最近存在祖先路径的 canonical 结果仍在工作区内
    let mut ancestor = normalized.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "路径无法解析".to_string())?;
    }
    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_within_workspace(&base_canonical, ancestor_canonical)?;
    Ok(normalized)
}

pub(crate) fn resolve_workspace_path(
    base: &Path,
    sub: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))?;
    let sub = match sub {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(base_canonical),
    };
    let normalized = absolutize_workspace_subpath(&base_canonical, sub)?;
    let canonical = normalized
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_within_workspace(&base_canonical, canonical)
}

/// 通过原生文件对话框选择工作区根目录
pub async fn workspace_pick_handler() -> Json<WorkspacePickResponse> {
    let path = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .ok()
        .and_then(|opt| opt)
        .map(|p| p.display().to_string());
    Json(WorkspacePickResponse { path })
}

/// 设置当前工作区根目录（来自前端）。非空路径须已存在、为目录，且落在配置的 `workspace_allowed_roots` 内（未配置时仅允许 `run_command_working_dir` 及其子目录）。
pub async fn workspace_set_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceSetBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let raw = body.path.as_deref().map(|s| s.trim()).unwrap_or("");
    let mut guard = state.workspace_override.write().await;
    // None 表示“从未设置过”；Some("") 表示“显式选择默认目录”；Some("...") 表示指定路径（存规范绝对路径）
    if raw.is_empty() {
        *guard = Some(String::new());
        return Ok(Json(serde_json::json!({ "ok": true, "path": "" })));
    }
    let canon = match validate_workspace_set_path(&state.cfg, raw) {
        Ok(p) => p,
        Err(msg) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "ok": false, "error": msg })),
            ));
        }
    };
    let path_str = canon.display().to_string();
    *guard = Some(path_str.clone());
    Ok(Json(serde_json::json!({ "ok": true, "path": path_str })))
}

/// 列出当前工作区或子目录
pub async fn workspace_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Json<WorkspaceResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("工作目录无法解析: {}", e);
            log::warn!("{}", msg);
            return Json(WorkspaceResponse {
                path: String::new(),
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    let canonical = match resolve_workspace_path(&base_canonical, query.path.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceResponse {
                path: base_canonical.display().to_string(),
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    let path_str = canonical.display().to_string();
    let mut entries = Vec::new();
    let mut read_dir = match tokio::fs::read_dir(&canonical).await {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("无法读取工作目录: {}", e);
            log::warn!("{}", msg);
            return Json(WorkspaceResponse {
                path: path_str,
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    loop {
        let entry = match read_dir.next_entry().await {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => {
                let msg = format!("读取目录项失败: {}", e);
                log::warn!("{}", msg);
                break;
            }
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false);
        entries.push(WorkspaceEntry { name, is_dir });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Json(WorkspaceResponse {
        path: path_str,
        entries,
        error: None,
    })
}

/// 在当前工作区内搜索文件内容（基于 search_in_files/grep 工具），返回纯文本结果
pub async fn workspace_search_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceSearchBody>,
) -> Json<WorkspaceSearchResponse> {
    let pattern = body.pattern.trim();
    if pattern.is_empty() {
        return Json(WorkspaceSearchResponse {
            output: String::new(),
            error: Some("pattern 不能为空".to_string()),
        });
    }
    let base_str = state.effective_workspace_path().await;
    let base = Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceSearchResponse {
                output: String::new(),
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    // 将来自前端的绝对路径（当前目录 data.path）转换为相对于工作区根目录的相对路径，供 grep 工具使用
    let rel_path = match body
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(p) => {
            let p_path = Path::new(p);
            let canonical = match p_path.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    return Json(WorkspaceSearchResponse {
                        output: String::new(),
                        error: Some(format!("搜索路径无法解析: {}", e)),
                    });
                }
            };
            if !canonical.starts_with(&base_canonical) {
                return Json(WorkspaceSearchResponse {
                    output: String::new(),
                    error: Some("搜索路径不能超出工作区根目录".to_string()),
                });
            }
            match canonical.strip_prefix(&base_canonical) {
                Ok(r) => Some(r.to_string_lossy().to_string()),
                Err(_) => None,
            }
        }
    };
    let mut args = serde_json::json!({ "pattern": pattern });
    if let Some(p) = rel_path {
        args["path"] = serde_json::Value::String(p);
    }
    if let Some(m) = body.max_results {
        args["max_results"] = serde_json::json!(m);
    }
    if let Some(ci) = body.case_insensitive {
        args["case_insensitive"] = serde_json::json!(ci);
    }
    if let Some(ih) = body.ignore_hidden {
        args["ignore_hidden"] = serde_json::json!(ih);
    }
    let ctx =
        crate::tools::tool_context_for(&state.cfg, &state.cfg.allowed_commands, &base_canonical);
    let output = crate::tools::run_tool("search_in_files", &args.to_string(), &ctx);
    Json(WorkspaceSearchResponse {
        output,
        error: None,
    })
}

#[derive(Serialize)]
pub struct WorkspaceFileReadResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 工作区文件读取：按 path 返回文件内容（path 为工作区内文件路径）
pub async fn workspace_file_read_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileReadResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = query.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_path(&base_canonical, Some(path)) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(msg),
            });
        }
    };
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(format!("无法读取文件信息: {}", e)),
            });
        }
    };
    if meta.is_dir() {
        return Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some("路径是目录，无法读取为文件".to_string()),
        });
    }
    if meta.len() > WORKSPACE_FILE_READ_MAX_BYTES {
        return Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some(format!(
                "文件过大（{} 字节），当前最多读取 {} 字节",
                meta.len(),
                WORKSPACE_FILE_READ_MAX_BYTES
            )),
        });
    }
    match tokio::fs::read_to_string(&canonical).await {
        Ok(content) => Json(WorkspaceFileReadResponse {
            content,
            error: None,
        }),
        Err(e) => Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some(format!("读取文件失败: {}", e)),
        }),
    }
}

/// 删除工作区内的文件：path 为工作区内文件路径，不能删除目录
pub async fn workspace_file_delete_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileDeleteResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = query.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileDeleteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_path(&base_canonical, Some(path)) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileDeleteResponse { error: Some(msg) });
        }
    };
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(format!("无法读取文件信息: {}", e)),
            });
        }
    };
    if meta.is_dir() {
        return Json(WorkspaceFileDeleteResponse {
            error: Some("不支持删除目录".to_string()),
        });
    }
    match tokio::fs::remove_file(&canonical).await {
        Ok(()) => Json(WorkspaceFileDeleteResponse { error: None }),
        Err(e) => Json(WorkspaceFileDeleteResponse {
            error: Some(format!("删除文件失败: {}", e)),
        }),
    }
}

/// 工作区文件写入：支持创建、写入（创建或覆盖）、仅创建、仅修改
pub async fn workspace_file_write_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceFileWriteBody>,
) -> Json<WorkspaceFileWriteResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileWriteResponse {
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = body.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileWriteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_write_path(&base_canonical, path) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileWriteResponse { error: Some(msg) });
        }
    };

    let exists = tokio::fs::try_exists(&canonical).await.unwrap_or(false);
    if body.create_only && exists {
        return Json(WorkspaceFileWriteResponse {
            error: Some("文件已存在，无法仅创建".to_string()),
        });
    }
    if body.update_only && !exists {
        return Json(WorkspaceFileWriteResponse {
            error: Some("文件不存在，无法仅修改".to_string()),
        });
    }

    if let Some(parent) = canonical.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        return Json(WorkspaceFileWriteResponse {
            error: Some(format!("创建目录失败: {}", e)),
        });
    }
    match tokio::fs::write(&canonical, body.content.as_bytes()).await {
        Ok(()) => Json(WorkspaceFileWriteResponse { error: None }),
        Err(e) => Json(WorkspaceFileWriteResponse {
            error: Some(format!("写入文件失败: {}", e)),
        }),
    }
}
